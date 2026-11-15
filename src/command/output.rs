use super::{create_output, failure, HandlerResult, Toggle, XkbLayoutTarget};
use crate::swayward::State;

pub(super) fn create(state: &mut State) -> HandlerResult {
    let State { backend, swayward } = state;
    let headless = match backend {
        crate::backend::Backend::Headless(headless) => Some(headless),
        crate::backend::Backend::Tty(_) | crate::backend::Backend::Winit(_) => None,
    };
    super::handled_outcome(create_output(headless, swayward))
}

pub(super) fn switch_layout(
    state: &mut State,
    identifier: &str,
    target: XkbLayoutTarget,
) -> HandlerResult {
    let matches_keyboard = state.swayward.ipc_input_devices.values().any(|device| {
        device.device_type == "keyboard"
            && (matches!(identifier, "*" | "type:keyboard") || device.identifier == identifier)
    });
    if let (true, Some(keyboard)) = (matches_keyboard, state.swayward.seat.get_keyboard()) {
        keyboard.with_xkb_state(state, |mut context| match target {
            XkbLayoutTarget::Next => context.cycle_next_layout(),
            XkbLayoutTarget::Prev => context.cycle_prev_layout(),
            XkbLayoutTarget::Index(index) => {
                let count = context.xkb().lock().map_or(0, |xkb| xkb.layouts().count());
                if (index as usize) < count {
                    context.set_layout(smithay::input::keyboard::Layout(index));
                }
            }
        });
        state.ipc_refresh_keyboard_layout_index();
    }
    Ok(None)
}

/// Whether `target` names an output that `toggle` would switch off.
///
/// Sway looks the name up among every output it knows, disabled ones
/// included, and disables the output unless its stored configuration already
/// does (`sway/sway/commands/output/toggle.c:17-30`).
fn toggle_disables(state: &State, target: &str) -> Option<bool> {
    let name = state
        .backend
        .ipc_outputs()
        .lock()
        .unwrap()
        .values()
        .map(swayward_config::OutputName::from_ipc_output)
        .find(|name| name.matches(target))?;
    let config = state.swayward.config.borrow();
    Some(!config.outputs.find(&name).is_some_and(|output| output.off))
}

pub(super) fn configure(
    state: &mut State,
    target: String,
    mut actions: Vec<swayward_ipc::OutputAction>,
) -> HandlerResult {
    if actions.contains(&swayward_ipc::OutputAction::Toggle) {
        let Some(disables) = toggle_disables(state, &target) else {
            return Err(failure(format!(
                "Cannot apply toggle to unknown output {target}"
            )));
        };
        // Every subcommand of one command reads the stored configuration, so
        // each toggle resolves to the same state and the last
        // enable, disable, or toggle wins.
        for action in &mut actions {
            if *action == swayward_ipc::OutputAction::Toggle {
                *action = if disables {
                    swayward_ipc::OutputAction::Off
                } else {
                    swayward_ipc::OutputAction::On
                };
            }
        }
    }
    if actions
        .iter()
        .any(|action| matches!(action, swayward_ipc::OutputAction::Off))
    {
        state.ipc_suppress_workspace_moves();
    }
    let targets = if target == "*" {
        state
            .swayward
            .global_space
            .outputs()
            .map(|output| output.name())
            .collect::<Vec<_>>()
    } else {
        vec![target]
    };
    let has_output_event = actions.iter().any(|action| {
        matches!(
            action,
            swayward_ipc::OutputAction::On
                | swayward_ipc::OutputAction::Off
                | swayward_ipc::OutputAction::Power { .. }
        )
    });
    for target in targets {
        for action in &actions {
            if let swayward_ipc::OutputAction::Power { power } = action {
                let output = state.swayward.output_by_name_match(&target).cloned();
                if *power == Toggle::Toggle && output.is_none() {
                    return Err(failure(format!(
                        "Cannot apply toggle to unknown output {target}"
                    )));
                }
                let target = output
                    .as_ref()
                    .map_or_else(|| target.clone(), |output| output.name());
                let current = state
                    .swayward
                    .output_power
                    .get(&target)
                    .copied()
                    .unwrap_or(true);
                let enabled = match power {
                    Toggle::Enable => true,
                    Toggle::Disable => false,
                    Toggle::Toggle => !current,
                };
                state.swayward.output_power.insert(target, enabled);
                if let Some(output) = output {
                    state.backend.set_output_power(&output, enabled);
                    if enabled {
                        state.swayward.queue_redraw(&output);
                    }
                }
            }
        }
        let config_actions = actions
            .iter()
            .filter(|action| !matches!(action, swayward_ipc::OutputAction::Power { .. }))
            .cloned()
            .collect::<Vec<_>>();
        if !config_actions.is_empty() {
            state.apply_transient_output_config(&target, &config_actions);
        }
    }
    if has_output_event {
        // One event per configuration change, like sway's
        // update_output_manager_config (sway/desktop/output.c:377-399), even
        // when the change also repositioned surviving outputs.
        state.swayward.ipc_outputs_changed = true;
        state.refresh_ipc_outputs();
    }
    Ok(None)
}
