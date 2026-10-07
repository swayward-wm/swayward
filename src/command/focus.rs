use swayward_config::Action;
use swayward_ipc::CommandOutcome;

use super::{failure, output_target_by_name_or_direction, CommandTarget, Direction};
use crate::layout::LayoutElement as _;
use crate::swayward::State;

pub(super) fn direction(state: &mut State, direction: Direction) -> Option<Action> {
    directional(state, direction, |layout, local_wrap| {
        match (direction, local_wrap) {
            (Direction::Left, true) => layout.focus_left(),
            (Direction::Right, true) => layout.focus_right(),
            (Direction::Up, true) => layout.focus_up(),
            (Direction::Down, true) => layout.focus_down(),
            (Direction::Left, false) => layout.focus_left_without_wrap(),
            (Direction::Right, false) => layout.focus_right_without_wrap(),
            (Direction::Up, false) => layout.focus_up_without_wrap(),
            (Direction::Down, false) => layout.focus_down_without_wrap(),
        }
    })
}

/// sway's `focus <direction>` with `local` as the in-workspace step. It falls through to the
/// output in that direction before it takes a plain `focus_wrapping yes` wrap candidate
/// (`node_get_in_direction_tiling`, sway/commands/focus.c:207-223).
fn directional(
    state: &mut State,
    direction: Direction,
    local: impl FnOnce(&mut crate::layout::Layout<crate::window::Mapped>, bool) -> bool,
) -> Option<Action> {
    let action = match direction {
        Direction::Left => Action::FocusColumnOrMonitorLeft,
        Direction::Right => Action::FocusColumnOrMonitorRight,
        Direction::Up => Action::FocusWindowOrMonitorUp,
        Direction::Down => Action::FocusWindowOrMonitorDown,
    };
    let wrapping = state.swayward.config.borrow().layout.focus_wrapping;
    let local_wrap = matches!(
        wrapping,
        swayward_config::FocusWrapping::Force | swayward_config::FocusWrapping::Workspace
    );
    if !state.swayward.layout.global_fullscreen_active()
        && state.swayward.layout.focused_fullscreen_mode()
            == Some(crate::layout::tiling_tree::FullscreenMode::Workspace)
    {
        // sway walks up from the focused view first, so a child of a fullscreen split moves
        // among its siblings; only reaching the fullscreen container goes to the output
        // (`node_get_in_direction_tiling`, sway/commands/focus.c:143-155).
        let floating = state
            .swayward
            .layout
            .active_workspace()
            .is_some_and(|workspace| workspace.floating_is_active());
        if !floating && local(&mut state.swayward.layout, local_wrap) {
            state.swayward.queue_redraw_all();
            return None;
        }
        let output = match direction {
            Direction::Left => state.swayward.adjacent_output_left(),
            Direction::Right => state.swayward.adjacent_output_right(),
            Direction::Up => state.swayward.adjacent_output_up(),
            Direction::Down => state.swayward.adjacent_output_down(),
        };
        if let Some(output) = output {
            state.swayward.layout.focus_output(&output);
            state.swayward.queue_redraw_all();
        }
        return None;
    }
    // A floating root moves only among floaters, without wrapping, and never leaves its
    // workspace (`node_get_in_direction_floating`, sway/commands/focus.c:226-258, 457-460).
    if state
        .swayward
        .layout
        .active_workspace()
        .is_some_and(|workspace| {
            workspace.floating_is_active() && !workspace.focused_floating_tree_child()
        })
    {
        let direction = match direction {
            Direction::Left => crate::layout::tiling_tree::Direction::Left,
            Direction::Right => crate::layout::tiling_tree::Direction::Right,
            Direction::Up => crate::layout::tiling_tree::Direction::Up,
            Direction::Down => crate::layout::tiling_tree::Direction::Down,
        };
        if state
            .swayward
            .layout
            .active_workspace_mut()
            .is_some_and(|workspace| workspace.focus_floating_direction(direction))
        {
            state.swayward.queue_redraw_all();
        }
        return None;
    }
    let workspace_focused = state
        .swayward
        .layout
        .active_workspace()
        .is_some_and(|workspace| workspace.is_workspace_focused());
    let changed = local(&mut state.swayward.layout, local_wrap);
    if changed {
        state.swayward.queue_redraw_all();
        None
    } else if state.swayward.layout.global_fullscreen_active()
        || wrapping == swayward_config::FocusWrapping::Workspace && !workspace_focused
    {
        None
    } else {
        Some(action)
    }
}

pub(super) fn output(state: &mut State, identifier: &str) -> Result<(), CommandOutcome> {
    let output = output_target_by_name_or_direction(state, identifier)?;
    if let Some(output) = output {
        state.swayward.layout.focus_output(&output);
        state.swayward.queue_redraw_all();
    }
    Ok(())
}

pub(super) fn parent(state: &mut State) {
    state.swayward.layout.focus_parent();
    state.swayward.queue_redraw_all();
}

pub(super) fn child(state: &mut State) {
    let before = seat_focused_view(state);
    state.swayward.layout.focus_child();
    refocus_clears_urgency(state, before);
    state.swayward.queue_redraw_all();
}

/// The view holding sway's seat focus: none while a split or the workspace
/// itself is focused, even though the keyboard stays on a view.
fn seat_focused_view(state: &State) -> Option<crate::window::mapped::MappedId> {
    match super::targeted::focused_node(state) {
        swayward_ipc::command::FocusedNode::View => match super::targeted::focused_target(state)? {
            CommandTarget::Window(id) => Some(id),
            CommandTarget::Container(..) => None,
        },
        _ => None,
    }
}

/// Seat focus moved onto the keyboard-focused view from its parent or
/// workspace. No keyboard focus change follows, so clear its urgency here
/// as sway's seat focus change does (sway/sway/input/seat.c:1223-1240).
fn refocus_clears_urgency(state: &mut State, before: Option<crate::window::mapped::MappedId>) {
    let Some(after) = seat_focused_view(state) else {
        return;
    };
    let keyboard = state.swayward.keyboard_focus.surface().cloned();
    let holds_keyboard = keyboard.is_some_and(|surface| {
        state
            .swayward
            .layout
            .windows()
            .any(|(_, mapped)| mapped.id() == after && mapped.is_wl_surface(&surface))
    });
    if before != Some(after) && holds_keyboard {
        state.swayward.seat_refocus_clears_urgency(after);
    }
}

/// The tiling direction of `focus next|prev`, from the focused container's parent layout
/// (`get_direction_from_next_prev`, sway/commands/focus.c:17-58).
fn next_prev_direction(state: &State, next: bool) -> Option<Direction> {
    use crate::layout::tiling_tree::Direction as TreeDirection;
    Some(
        match state.swayward.layout.tiling_next_prev_direction(next)? {
            TreeDirection::Left => Direction::Left,
            TreeDirection::Right => Direction::Right,
            TreeDirection::Up => Direction::Up,
            TreeDirection::Down => Direction::Down,
        },
    )
}

/// `focus next|prev sibling`: `focus <direction>` that stops at the sibling container instead
/// of descending into it, and still crosses to the next output at the workspace edge
/// (sway/commands/focus.c:194-196, 207-213). A floating window ignores `sibling` and moves
/// among floaters (sway/commands/focus.c:457-460).
pub(super) fn next_prev_sibling(state: &mut State, next: bool) -> Option<Action> {
    let Some(direction) = next_prev_direction(state, next) else {
        return next_or_prev(state, next);
    };
    directional(state, direction, |layout, local_wrap| {
        layout.focus_next_prev_sibling(next, local_wrap)
    })
}

/// `focus next|prev`. A tiling container moves like `focus <direction>`, crossing outputs; a
/// floating one moves among floaters. Sway answers success whether or not focus moved
/// (sway/commands/focus.c:434-475).
pub(super) fn next_or_prev(state: &mut State, next: bool) -> Option<Action> {
    if let Some(direction) = next_prev_direction(state, next) {
        return self::direction(state, direction);
    }
    state.swayward.layout.focus_next_or_prev(next);
    state.swayward.queue_redraw_all();
    None
}

/// `focus floating|tiling` (`focus_mode`, sway/commands/focus.c:262-307).
/// The target is the layer's most recently focused view, and a fullscreen
/// view counts on the layer it restores to. Sway never leaves fullscreen
/// here: `seat_set_focus` refuses a target a fullscreen container hides
/// (`container_obstructing_fullscreen_container`, sway/input/seat.c:1148-1151),
/// so the command succeeds and nothing changes.
pub(super) fn mode(state: &mut State, floating: bool) -> Result<Option<Action>, CommandOutcome> {
    use crate::layout::tiling_tree::FullscreenMode;
    use crate::layout::LayoutElement as _;

    let layout = &state.swayward.layout;
    let Some(workspace) = layout.active_workspace() else {
        return Err(failure("Target container is not in a workspace"));
    };
    let Some(target) = workspace
        .windows()
        .filter(|mapped| workspace.is_floating_for_ipc(&mapped.window) == floating)
        .max_by_key(|mapped| mapped.focus_timestamp())
        .map(|mapped| mapped.window.clone())
    else {
        let layer = if floating { "floating" } else { "tiling" };
        return Err(failure(format!(
            "Failed to find a {layer} container in workspace."
        )));
    };
    let hidden_by_workspace =
        workspace.fullscreen_mode().is_some() && !workspace.fullscreen_contains_window(&target);
    let hidden_by_global = layout.workspaces().any(|(_, _, candidate)| {
        candidate.fullscreen_mode() == Some(FullscreenMode::Global)
            && !candidate.fullscreen_contains_window(&target)
    });
    if hidden_by_workspace || hidden_by_global {
        return Ok(None);
    }
    // A fullscreen floating view lives in the tiling tree until it leaves
    // fullscreen, so the floating layer's focus action cannot reach it.
    if floating && !workspace.floating().has_window(&target) {
        state.swayward.layout.activate_window(&target);
        state.swayward.queue_redraw_all();
        return Ok(None);
    }
    Ok(Some(if floating {
        Action::FocusFloating
    } else {
        Action::FocusTiling
    }))
}

pub(super) fn targeted(state: &mut State, target: CommandTarget) -> Result<(), CommandOutcome> {
    match target {
        CommandTarget::Window(target) => {
            let window = state
                .swayward
                .layout
                .windows()
                .find_map(|(_, mapped)| (mapped.id() == target).then(|| mapped.window.clone()));
            let Some(window) = window else {
                return Err(failure("No matching node."));
            };
            if state.swayward.layout.is_scratchpad_hidden(&window) {
                state.swayward.layout.show_scratchpad(Some(&window));
            } else {
                let in_floating_group =
                    state.swayward.layout.workspaces().any(|(_, _, workspace)| {
                        workspace.floating_tree_root_for_window(&window).is_some()
                    });
                let before = seat_focused_view(state);
                state.swayward.layout.activate_window(&window);
                refocus_clears_urgency(state, before);
                // Each criteria match takes seat focus in turn (sway/commands.c:305-326), so
                // stamp it now: the keyboard focus update after the command only sees the last.
                if let Some(mapped) = state
                    .swayward
                    .layout
                    .workspaces_mut()
                    .flat_map(|workspace| workspace.windows_mut())
                    .find(|mapped| mapped.id() == target)
                {
                    mapped.set_focus_timestamp(crate::utils::get_monotonic_time());
                }
                if in_floating_group {
                    state.ipc_refresh_layout();
                    state.ipc_emit_window_change(
                        "focus",
                        crate::ipc::tree::window_id(target),
                        |_| {},
                    );
                }
            }
        }
        CommandTarget::Container(workspace, node) => {
            if !state.swayward.layout.focus_tiling_node(workspace, node) {
                return Err(failure("No matching node."));
            }
        }
    }
    Ok(())
}

pub(super) fn targeted_workspace(
    state: &mut State,
    target: CommandTarget,
) -> Result<(), CommandOutcome> {
    let CommandTarget::Window(target_id) = target else {
        return Err(failure("No container to focus was specified."));
    };
    let window = state
        .swayward
        .layout
        .windows()
        .find_map(|(_, mapped)| (mapped.id() == target_id).then(|| mapped.window.clone()));
    let Some(window) = window else {
        return Err(failure("No matching node."));
    };
    let target_workspace = state
        .swayward
        .layout
        .workspaces()
        .find(|(_, _, workspace)| workspace.has_window(&window))
        .map(|(_, _, workspace)| workspace.id());
    let active_workspace = state
        .swayward
        .layout
        .active_workspace()
        .map(|workspace| workspace.id());
    let auto_back_and_forth = state
        .swayward
        .config
        .borrow()
        .input
        .workspace_auto_back_and_forth;
    if auto_back_and_forth && target_workspace == active_workspace {
        let previous = crate::command::WorkspaceTarget::BackAndForth;
        state
            .swayward
            .layout
            .activate_sway_workspace(previous)
            .map_err(failure)
    } else {
        targeted(state, target)
    }
}

pub(super) fn targeted_direction(
    state: &mut State,
    target: CommandTarget,
    direction: Direction,
) -> Result<(), CommandOutcome> {
    let CommandTarget::Window(target) = target else {
        return Err(failure("directional focus requires a window target"));
    };
    let window = state
        .swayward
        .layout
        .windows()
        .find_map(|(_, mapped)| (mapped.id() == target).then(|| mapped.window.clone()));
    let Some(window) = window else {
        return Err(failure("No matching node."));
    };
    state.swayward.layout.activate_window(&window);
    state.do_action(
        match direction {
            Direction::Left => Action::FocusColumnLeft,
            Direction::Right => Action::FocusColumnRight,
            Direction::Up => Action::FocusWindowUp,
            Direction::Down => Action::FocusWindowDown,
        },
        false,
    );
    Ok(())
}

pub(super) fn mode_toggle(state: &mut State) -> Result<Option<Action>, CommandOutcome> {
    let floating = state
        .swayward
        .layout
        .active_workspace()
        .is_some_and(|workspace| {
            workspace.floating_is_active() || workspace.active_floating_is_fullscreen()
        });
    mode(state, !floating)
}
