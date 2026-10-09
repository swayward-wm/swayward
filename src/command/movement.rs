use swayward_ipc::legacy::PositionChange;
use swayward_ipc::CommandOutcome;

use super::{
    failure, success, CommandTarget, Direction, MovePosition, OutputTarget, ResizeAmount,
    ResizeUnit, SwapTarget, WorkspaceTarget,
};
use crate::swayward::State;

fn parse_output_direction(value: &str) -> Option<Direction> {
    match value.to_ascii_lowercase().as_str() {
        "left" => Some(Direction::Left),
        "right" => Some(Direction::Right),
        "up" => Some(Direction::Up),
        "down" => Some(Direction::Down),
        _ => None,
    }
}

pub(super) fn output_target_by_name_or_direction(
    state: &State,
    identifier: &str,
) -> Result<Option<smithay::output::Output>, CommandOutcome> {
    if let Some(output) = state.swayward.output_by_name_match(identifier) {
        return Ok(Some(output.clone()));
    }
    let Some(direction) = parse_output_direction(identifier) else {
        return Err(swayward_ipc::command::parse_error(
            "There is no output with that name.",
        ));
    };
    let Some(reference) = state.swayward.layout.active_output() else {
        return Err(failure("No focused workspace to base directions off of."));
    };
    Ok(match direction {
        Direction::Left => state.swayward.output_left_of(reference),
        Direction::Right => state.swayward.output_right_of(reference),
        Direction::Up => state.swayward.output_up_of(reference),
        Direction::Down => state.swayward.output_down_of(reference),
    })
}

pub(super) fn output_target(
    state: &State,
    target: &OutputTarget,
    reference: Option<&smithay::output::Output>,
    reference_point: Option<smithay::utils::Point<i32, smithay::utils::Logical>>,
) -> Result<smithay::output::Output, String> {
    let output = match target {
        OutputTarget::Name(name) if name.eq_ignore_ascii_case("current") => {
            state.swayward.layout.active_output().cloned()
        }
        OutputTarget::Name(name) => state.swayward.output_by_name_match(name).cloned(),
        OutputTarget::Direction(direction) => match (direction, reference) {
            (direction, Some(output)) => {
                let reference = match reference_point {
                    Some(point) => point,
                    None => state
                        .swayward
                        .global_space
                        .output_geometry(output)
                        .map(crate::utils::center)
                        .ok_or("Reference output has no geometry")?,
                };
                let (horizontal, positive) = match direction {
                    Direction::Left => (true, false),
                    Direction::Right => (true, true),
                    Direction::Up => (false, false),
                    Direction::Down => (false, true),
                };
                state
                    .swayward
                    .output_in_direction_wrapping(output, reference, horizontal, positive)
            }
            (Direction::Left, None) => state.swayward.output_left(),
            (Direction::Right, None) => state.swayward.output_right(),
            (Direction::Up, None) => state.swayward.output_up(),
            (Direction::Down, None) => state.swayward.output_down(),
        },
    };
    output.ok_or_else(|| {
        format!(
            "Can't find output with name/direction '{}'",
            output_target_name(target)
        )
    })
}

fn output_target_name(target: &OutputTarget) -> &str {
    match target {
        OutputTarget::Name(name) => name,
        OutputTarget::Direction(Direction::Left) => "left",
        OutputTarget::Direction(Direction::Right) => "right",
        OutputTarget::Direction(Direction::Up) => "up",
        OutputTarget::Direction(Direction::Down) => "down",
    }
}

pub(crate) fn clamp_pointer_position(
    mut position: smithay::utils::Point<f64, smithay::utils::Logical>,
    size: smithay::utils::Size<f64, smithay::utils::Logical>,
    output: Option<smithay::utils::Rectangle<f64, smithay::utils::Logical>>,
) -> smithay::utils::Point<f64, smithay::utils::Logical> {
    let Some(output) = output else {
        return position;
    };
    let right = output.loc.x + output.size.w;
    let bottom = output.loc.y + output.size.h;
    position.x = position.x.max(output.loc.x);
    position.y = position.y.max(output.loc.y);
    if position.x + size.w > right {
        position.x = right - size.w;
    }
    if position.y + size.h > bottom {
        position.y = bottom - size.h;
    }
    position
}

type LogicalSize = smithay::utils::Size<f64, smithay::utils::Logical>;
type LogicalPoint = smithay::utils::Point<f64, smithay::utils::Logical>;

fn move_target_geometry(
    state: &State,
    window: Option<&smithay::desktop::Window>,
) -> Option<(LogicalSize, LogicalPoint)> {
    let id = window.or_else(|| state.swayward.layout.focus().map(|mapped| &mapped.window))?;
    state
        .swayward
        .layout
        .workspaces()
        .find_map(|(monitor, _, workspace)| {
            let size = workspace
                .floating_tree_root_for_window(id)
                .and_then(|root| workspace.floating().tree_rect(root))
                .map(|rect| rect.size)
                .or_else(|| {
                    workspace
                        .tiles_with_ipc_layouts()
                        .find(|(tile, _)| &tile.window().window == id)
                        .map(|(tile, _)| tile.tile_size())
                })?;
            let output_origin = monitor.map_or_else(Default::default, |monitor| {
                monitor.output().current_location().to_f64()
            });
            Some((size, output_origin + workspace.working_area().loc))
        })
}

fn coordinate(amount: ResizeAmount, extent: f64) -> f64 {
    match amount.unit {
        ResizeUnit::Default | ResizeUnit::Pixels => f64::from(amount.amount),
        ResizeUnit::PercentagePoints => extent * f64::from(amount.amount) / 100.,
    }
}

fn move_to_pointer(
    state: &mut State,
    window: Option<&smithay::desktop::Window>,
) -> Result<(), &'static str> {
    let pointer = state
        .swayward
        .seat
        .get_pointer()
        .ok_or("No cursor device")?
        .current_location();
    let id = window.cloned().or_else(|| {
        state
            .swayward
            .layout
            .focus()
            .map(|mapped| mapped.window.clone())
    });
    let Some(id) = id else {
        return Err("Only floating containers can be moved to an absolute position");
    };
    let Some((tile_size, workspace_origin)) = move_target_geometry(state, window) else {
        return Err("Only floating containers can be moved to an absolute position");
    };
    let cursor_output = state
        .swayward
        .global_space
        .output_under(pointer)
        .next()
        .cloned();
    let output_geometry = cursor_output
        .as_ref()
        .and_then(|output| state.swayward.global_space.output_geometry(output));
    let position = clamp_pointer_position(
        pointer - tile_size.downscale(2.),
        tile_size,
        output_geometry.map(|output| output.to_f64()),
    );
    let window_output = state
        .swayward
        .layout
        .windows()
        .find_map(|(monitor, mapped)| {
            (mapped.window == id)
                .then(|| monitor.map(|monitor| monitor.output().clone()))
                .flatten()
        });
    if let Some(output) = cursor_output
        .as_ref()
        .filter(|output| window_output.as_ref() != Some(*output))
    {
        state.swayward.layout.move_to_output(
            Some(&id),
            output,
            None,
            crate::layout::ActivateWindow::Yes,
        );
    }
    let workspace_origin = output_geometry
        .map(|output| output.loc.to_f64())
        .unwrap_or(workspace_origin);
    let position = position - workspace_origin;
    state.swayward.layout.move_floating_window(
        Some(&id),
        PositionChange::SetFixed(position.x),
        PositionChange::SetFixed(position.y),
        true,
    );
    Ok(())
}

pub(super) fn move_position(
    state: &mut State,
    target: Option<CommandTarget>,
    position: &MovePosition,
) -> Result<(), &'static str> {
    let (window, floating_root) = resolve_move_target(state, target)?;
    let workspace = state
        .swayward
        .layout
        .workspaces()
        .find_map(|(_, _, workspace)| workspace.has_window(&window).then_some(workspace))
        .ok_or("Only floating containers can be moved to an absolute position")?;
    // A fullscreen floater lives in the tiling layer here but is floating in sway
    // (`container_is_floating`, sway/commands/move.c:785-789).
    if !floating_root
        && !workspace.floating().has_window(&window)
        && workspace.is_floating_for_ipc(&window)
        && !matches!(position, MovePosition::Pointer)
    {
        return move_fullscreen_floater(state, &window, position);
    }
    if !(floating_root || workspace.is_floating(&window)) {
        return Err("Only floating containers can be moved to an absolute position");
    }
    let (x, y) = match *position {
        MovePosition::Coordinates { x, y, absolute } => {
            if absolute
                && (x.unit == ResizeUnit::PercentagePoints
                    || y.unit == ResizeUnit::PercentagePoints)
            {
                return Err("Cannot move to absolute positions by ppt");
            }
            let offset: LogicalPoint = if absolute {
                let Some((_, workspace_origin)) = move_target_geometry(state, Some(&window)) else {
                    return Err("Only floating containers can be moved to an absolute position");
                };
                (-workspace_origin.x, -workspace_origin.y).into()
            } else {
                Default::default()
            };
            (
                PositionChange::SetFixed(coordinate(x, workspace.working_area().size.w) + offset.x),
                PositionChange::SetFixed(coordinate(y, workspace.working_area().size.h) + offset.y),
            )
        }
        MovePosition::Center { absolute: false } => {
            state.swayward.layout.center_window(Some(&window));
            return Ok(());
        }
        MovePosition::Center { absolute: true } => {
            let root = state
                .swayward
                .global_space
                .outputs()
                .filter_map(|output| state.swayward.global_space.output_geometry(output))
                .reduce(|root, output| root.merge(output));
            let Some(root) = root else { return Ok(()) };
            let Some((tile_size, workspace_origin)) = move_target_geometry(state, Some(&window))
            else {
                return Err("Only floating containers can be moved to an absolute position");
            };
            let root_center = crate::utils::center(root).to_f64();
            let position = root_center - tile_size.downscale(2.) - workspace_origin;
            (
                PositionChange::SetFixed(position.x),
                PositionChange::SetFixed(position.y),
            )
        }
        MovePosition::Pointer => return move_to_pointer(state, Some(&window)),
    };
    state
        .swayward
        .layout
        .move_floating_window(Some(&window), x, y, true);
    state.swayward.layout.rehome_floating_window(&window);
    Ok(())
}

/// `move position` on a fullscreen floater: its box is the output's, so only the output under
/// the moved box's centre matters (`cmd_move_to_position`, sway/commands/move.c:810-918).
fn move_fullscreen_floater(
    state: &mut State,
    window: &smithay::desktop::Window,
    position: &MovePosition,
) -> Result<(), &'static str> {
    let Some((size, workspace_origin)) = move_target_geometry(state, Some(window)) else {
        return Err("Only floating containers can be moved to an absolute position");
    };
    let workspace_size = state
        .swayward
        .layout
        .workspaces()
        .find_map(|(_, _, workspace)| {
            workspace
                .has_window(window)
                .then(|| workspace.working_area().size)
        })
        .unwrap_or_default();
    let loc: LogicalPoint = match *position {
        MovePosition::Coordinates { x, y, absolute } => {
            if absolute
                && (x.unit == ResizeUnit::PercentagePoints
                    || y.unit == ResizeUnit::PercentagePoints)
            {
                return Err("Cannot move to absolute positions by ppt");
            }
            let loc = LogicalPoint::from((
                coordinate(x, workspace_size.w),
                coordinate(y, workspace_size.h),
            ));
            if absolute {
                loc
            } else {
                loc + workspace_origin
            }
        }
        MovePosition::Center { absolute: false } => {
            workspace_origin + (workspace_size.to_point() - size.to_point()).downscale(2.)
        }
        MovePosition::Center { absolute: true } => {
            let root = state
                .swayward
                .global_space
                .outputs()
                .filter_map(|output| state.swayward.global_space.output_geometry(output))
                .reduce(|root, output| root.merge(output));
            let Some(root) = root else { return Ok(()) };
            crate::utils::center(root).to_f64() - size.downscale(2.)
        }
        MovePosition::Pointer => return Ok(()),
    };
    state.swayward.layout.rehome_fullscreen_floater(window, loc);
    Ok(())
}

/// The window a `move position` acts on, and whether the command addressed
/// its floating group's root rather than the window itself.
fn resolve_move_target(
    state: &State,
    target: Option<CommandTarget>,
) -> Result<(smithay::desktop::Window, bool), &'static str> {
    Ok(match target {
        Some(CommandTarget::Window(target)) => (
            state
                .swayward
                .layout
                .windows()
                .find_map(|(_, mapped)| (mapped.id() == target).then(|| mapped.window.clone()))
                .ok_or("No matching node.")?,
            false,
        ),
        Some(CommandTarget::Container(workspace, node)) => {
            let workspace = state
                .swayward
                .layout
                .find_workspace_by_id(workspace)
                .map(|(_, workspace)| workspace)
                .ok_or("No matching node.")?;
            if workspace.floating().tree_root_for_node(node) != Some(node) {
                return Err("command requires a window target");
            }
            let window = workspace
                .floating()
                .tree(node)
                .and_then(crate::layout::tiling_tree::TilingTree::active_window)
                .map(|mapped| mapped.window.clone())
                .ok_or("No matching node.")?;
            (window, true)
        }
        None => {
            let workspace = state
                .swayward
                .layout
                .active_workspace()
                .ok_or("Only floating containers can be moved to an absolute position")?;
            let floating_root = workspace
                .focused_floating_tree_root()
                .is_some_and(|root| workspace.focused_container_node() == Some(root));
            let window = workspace
                .active_window()
                .map(|mapped| mapped.window.clone())
                .ok_or("Only floating containers can be moved to an absolute position")?;
            (window, floating_root)
        }
    })
}

pub(super) fn select_resize_amount(
    first: ResizeAmount,
    second: Option<ResizeAmount>,
    floating: bool,
) -> ResizeAmount {
    let preferred = if floating {
        ResizeUnit::Pixels
    } else {
        ResizeUnit::PercentagePoints
    };
    [Some(first), second]
        .into_iter()
        .flatten()
        .find(|amount| amount.unit == preferred)
        .or_else(|| {
            [Some(first), second]
                .into_iter()
                .flatten()
                .find(|amount| amount.unit == ResizeUnit::Default)
        })
        .unwrap_or(first)
}

fn workspace_to_move(
    state: &State,
    target: Option<CommandTarget>,
) -> Result<Option<crate::layout::workspace::WorkspaceId>, CommandOutcome> {
    Ok(match target {
        Some(CommandTarget::Window(target)) => {
            let Some((window, workspace)) = state
                .swayward
                .layout
                .windows()
                .find_map(|(_, mapped)| {
                    (mapped.id() == target).then(|| {
                        (
                            mapped.window.clone(),
                            state.swayward.layout.window_workspace_id(&mapped.window),
                        )
                    })
                })
                .or_else(|| {
                    state
                        .swayward
                        .layout
                        .scratchpad_windows()
                        .find(|mapped| mapped.id() == target)
                        .map(|mapped| (mapped.window.clone(), None))
                })
            else {
                return Err(failure("No matching node."));
            };
            if state.swayward.layout.is_scratchpad_hidden(&window) {
                return Ok(None);
            }
            workspace
        }
        Some(CommandTarget::Container(workspace, _)) => Some(workspace),
        None => state
            .swayward
            .layout
            .active_workspace()
            .map(|workspace| workspace.id()),
    })
}

pub(super) fn move_workspace_to_output(
    state: &mut State,
    target: Option<CommandTarget>,
    output_target_name: &OutputTarget,
) -> CommandOutcome {
    // `workspace_move_to_output` names the emptied output's replacement with
    // workspace_next_name (sway/sway/tree/workspace.c:1146-1154); a workspace
    // a switch already destroyed in sway must not hold its number here.
    state.swayward.layout.finish_all_sway_workspace_switches();
    let workspace_id = match workspace_to_move(state, target) {
        Ok(Some(workspace)) => workspace,
        Ok(None) => return success(),
        Err(error) => return error,
    };
    let Some(reference) = state
        .swayward
        .layout
        .workspaces()
        .find(|(_, _, workspace)| workspace.id() == workspace_id)
        .and_then(|(monitor, _, _)| monitor.map(|monitor| monitor.output().clone()))
    else {
        return failure("No workspace to move");
    };
    let reference_point = state
        .swayward
        .global_space
        .output_geometry(&reference)
        .map(crate::utils::center);
    let output = match output_target(state, output_target_name, Some(&reference), reference_point) {
        Ok(output) => output,
        Err(error) => return failure(error),
    };
    state
        .swayward
        .layout
        .move_workspace_to_output_by_id(workspace_id, Some(reference), &output);
    state.swayward.queue_redraw_all();
    success()
}

pub(super) fn move_tiling_subtree_to_output(
    state: &mut State,
    source_workspace: crate::layout::workspace::WorkspaceId,
    node: crate::layout::tiling_tree::NodeId,
    output: &smithay::output::Output,
) -> CommandOutcome {
    let Some(target_workspace) = state.swayward.layout.active_workspace_id_for_output(output)
    else {
        return failure("target output has no workspace");
    };
    let target = WorkspaceTarget::Name(
        state
            .swayward
            .layout
            .find_workspace_by_id(target_workspace)
            .and_then(|(_, workspace)| workspace.sway_name())
            .unwrap_or_else(|| target_workspace.get().to_string()),
    );
    move_target_to_workspace(
        state,
        CommandTarget::Container(source_workspace, node),
        target,
        false,
        false,
    )
}

pub(super) fn move_target_to_workspace(
    state: &mut State,
    target: CommandTarget,
    workspace_target: WorkspaceTarget,
    preserve_empty_workspace: bool,
    auto_back_and_forth: bool,
) -> CommandOutcome {
    if let Err(refused) = refuse_global_fullscreen_move(state, target) {
        return refused;
    }
    let focused_before = focused_view(state);
    let result = match target {
        CommandTarget::Window(target) => {
            let window = state
                .swayward
                .layout
                .windows()
                .find_map(|(_, mapped)| (mapped.id() == target).then(|| mapped.window.clone()));
            let Some(window) = window else {
                return failure("No matching node.");
            };
            if let Err(error) = state.swayward.layout.refuse_sticky_move_on_same_output(
                &window,
                &workspace_target,
                auto_back_and_forth,
            ) {
                return failure(error);
            }
            // A floating child moved to its own workspace while that workspace has no tiling
            // children stays put: sway's destination is the workspace itself, and
            // `container_move_to_workspace` returns early (sway/commands/move.c:198-202).
            if state
                .swayward
                .layout
                .move_targets_own_workspace_without_tiling(
                    &window,
                    &workspace_target,
                    auto_back_and_forth,
                )
            {
                raise_refocused_view(state, focused_before);
                state.swayward.queue_redraw_all();
                return success();
            }
            let old_group = state.swayward.layout.floating_group_of_child(&window);
            state.swayward.layout.detach_floating_group_child(&window);
            let moved = state
                .swayward
                .layout
                .move_window_to_sway_workspace(&window, workspace_target, auto_back_and_forth)
                .map(|_| ());
            // A focused child hands focus to its old parent's focus-inactive view
            // (sway/commands/move.c:589-597), which stays in the floating group even when the
            // child tiled on its own workspace.
            if let Some((workspace, root, parent)) =
                old_group.filter(|_| moved.is_ok() && focused_before.as_ref() == Some(&target))
            {
                state
                    .swayward
                    .layout
                    .focus_floating_group_after_child_left(workspace, root, parent, &window);
            }
            moved
        }
        CommandTarget::Container(workspace, node) => {
            let (_, remapped) = match state.swayward.layout.move_tiling_subtree_to_sway_workspace(
                workspace,
                node,
                workspace_target,
                preserve_empty_workspace,
                auto_back_and_forth,
            ) {
                Ok(moved) => moved,
                Err(error) => return failure(error),
            };
            state.swayward.remap_container_marks(remapped);
            Ok(())
        }
    };
    if let Err(error) = result {
        return failure(error);
    }
    raise_refocused_view(state, focused_before);
    state.swayward.queue_redraw_all();
    success()
}

/// Sway's `seat_set_focus` puts the view that takes over focus after a move
/// at the head of the seat-wide focus stack straight away
/// (sway/commands/move.c:598-608), so a later match in the same criteria run
/// that moves it lands it ahead of the others on the destination
/// (`seat_get_focus_inactive_tiling`, sway/input/seat.c:1374-1389).
fn raise_refocused_view(
    state: &mut State,
    focused_before: Option<crate::window::mapped::MappedId>,
) {
    let Some(focused) = focused_view(state).filter(|focused| Some(*focused) != focused_before)
    else {
        return;
    };
    let stamp = crate::utils::get_monotonic_time();
    state.swayward.layout.with_windows_mut(|mapped, _| {
        if mapped.id() == focused {
            mapped.set_focus_timestamp(stamp);
        }
    });
}

/// The view holding seat focus; none while a split or the workspace does.
fn focused_view(state: &State) -> Option<crate::window::mapped::MappedId> {
    (super::targeted::focused_node(state) == swayward_ipc::command::FocusedNode::View)
        .then(|| state.swayward.layout.focus().map(|mapped| mapped.id()))
        .flatten()
}

fn swap_kind_value(target: &SwapTarget) -> (&'static str, &str) {
    match target {
        SwapTarget::Id(value) => ("id", value),
        SwapTarget::ConId(value) => ("con_id", value),
        SwapTarget::Mark(value) => ("mark", value),
    }
}

fn swap_destination(state: &State, target: &SwapTarget) -> Result<CommandTarget, CommandOutcome> {
    let (kind, value, destination) = match target {
        SwapTarget::Id(_) => {
            return Err(failure(
                "swap container with id is unsupported because X11 window IDs are unavailable",
            ));
        }
        SwapTarget::ConId(value) => (
            "con_id",
            value,
            value.parse::<i64>().ok().and_then(|id| {
                state
                    .swayward
                    .layout
                    .workspaces()
                    .find_map(|(_, _, workspace)| {
                        workspace
                            .ipc_tiling_tree()
                            .nodes()
                            .into_iter()
                            .find_map(|(node, _)| {
                                (crate::ipc::tree::container_id(node) == id)
                                    .then_some(CommandTarget::Container(workspace.id(), node))
                            })
                    })
                    .or_else(|| {
                        state.swayward.layout.windows().find_map(|(_, mapped)| {
                            (crate::ipc::tree::window_id(mapped.id()) == id)
                                .then_some(CommandTarget::Window(mapped.id()))
                        })
                    })
            }),
        ),
        SwapTarget::Mark(value) => ("mark", value, marked_target(state, value)),
    };
    destination.ok_or_else(|| failure(format!("Failed to find {kind} '{value}'")))
}

type TilingEndpoint = (
    crate::layout::workspace::WorkspaceId,
    crate::layout::tiling_tree::NodeId,
);

fn resolve_swap_endpoint(
    state: &State,
    target: CommandTarget,
    source: bool,
    missing: impl FnOnce() -> CommandOutcome,
) -> Result<TilingEndpoint, CommandOutcome> {
    match target {
        CommandTarget::Container(workspace, node)
            if if source {
                state
                    .swayward
                    .layout
                    .active_workspace()
                    .is_some_and(|active| {
                        active.id() == workspace && active.contains_swap_node(node)
                    })
            } else {
                state.swayward.layout.workspaces().any(|(_, _, candidate)| {
                    candidate.id() == workspace && candidate.contains_swap_node(node)
                })
            } =>
        {
            Ok((workspace, node))
        }
        CommandTarget::Container(_, _) => Err(missing()),
        CommandTarget::Window(window) => {
            let Some(mapped) = state
                .swayward
                .layout
                .windows()
                .find_map(|(_, mapped)| (mapped.id() == window).then(|| mapped.window.clone()))
            else {
                return Err(missing());
            };
            state
                .swayward
                .layout
                .swap_target_for_window(&mapped)
                .ok_or_else(|| failure("Can only swap with containers and views"))
        }
    }
}

pub(super) fn swap_target(
    state: &mut State,
    source: CommandTarget,
    target: &SwapTarget,
) -> CommandOutcome {
    // `swap_places` re-parents both containers without
    // `container_set_floating`, so a CSD view keeps its stored border: a
    // floating one stays `csd` in a tiled slot and a tiled one keeps its
    // style while floating (sway/tree/container.c:1718-1764).
    state.swayward.layout.pin_sway_csd(true);
    let outcome = swap_target_unpinned(state, source, target);
    state.swayward.layout.pin_sway_csd(false);
    outcome
}

fn swap_target_unpinned(
    state: &mut State,
    source: CommandTarget,
    target: &SwapTarget,
) -> CommandOutcome {
    let destination = match swap_destination(state, target) {
        Ok(destination) => destination,
        Err(error) => return error,
    };
    if source == destination {
        return failure("Cannot swap a container with itself");
    }
    if let Some(outcome) = swap_with_floating_window(state, source, destination, target) {
        return outcome;
    }
    let (source_workspace, source_node) = match resolve_swap_endpoint(state, source, true, || {
        failure("Can only swap with containers and views")
    }) {
        Ok(endpoint) => endpoint,
        Err(error) => return error,
    };
    let (destination_workspace, destination_node) =
        match resolve_swap_endpoint(state, destination, false, || {
            let (kind, value) = swap_kind_value(target);
            failure(format!("Failed to find {kind} '{value}'"))
        }) {
            Ok(endpoint) => endpoint,
            Err(error) => return error,
        };
    if state
        .swayward
        .layout
        .is_tiling_root(source_workspace, source_node)
        || state
            .swayward
            .layout
            .is_tiling_root(destination_workspace, destination_node)
    {
        return failure("Can only swap with containers and views");
    }
    let swapped = if source_workspace == destination_workspace {
        state
            .swayward
            .layout
            .swap_tiling_nodes(source_workspace, source_node, destination_node)
            .map_err(failure)
    } else {
        swap_across_workspaces(
            state,
            (source_workspace, source_node),
            (destination_workspace, destination_node),
        )
    };
    if let Err(error) = swapped {
        return error;
    }
    state.swayward.queue_redraw_all();
    success()
}

/// A standalone floating view and the workspace it floats on.
fn floating_root_window(
    state: &State,
    target: CommandTarget,
) -> Option<(
    smithay::desktop::Window,
    crate::layout::workspace::WorkspaceId,
)> {
    let CommandTarget::Window(id) = target else {
        return None;
    };
    let window = super::mapped_window(state, id)?;
    let workspace = state
        .swayward
        .layout
        .workspaces()
        .find(|(_, _, workspace)| workspace.floating().window_is_floating_root(&window))?
        .2
        .id();
    Some((window, workspace))
}

/// Sway swaps a floating container with a tiled one, each taking the other's
/// place (`swap_places`, sway/tree/container.c:1718-1764). `None` when
/// neither endpoint is a standalone floating view.
fn swap_with_floating_window(
    state: &mut State,
    source: CommandTarget,
    destination: CommandTarget,
    target: &SwapTarget,
) -> Option<CommandOutcome> {
    let (floater, other, other_is_source) = match (
        floating_root_window(state, source),
        floating_root_window(state, destination),
    ) {
        (Some(floater), None) => (floater, destination, false),
        (None, Some(floater)) => (floater, source, true),
        _ => return None,
    };
    let (workspace, node) = match resolve_swap_endpoint(state, other, other_is_source, || {
        let (kind, value) = swap_kind_value(target);
        failure(format!("Failed to find {kind} '{value}'"))
    }) {
        Ok(endpoint) => endpoint,
        Err(error) => return Some(error),
    };
    if !state
        .swayward
        .layout
        .workspace_contains_tiling_node(workspace, node)
    {
        return Some(failure("Can only swap with containers and views"));
    }
    let layout = &mut state.swayward.layout;
    let swapped = if workspace == floater.1 {
        layout.swap_floating_window_with_tiling_node(&floater.0, workspace, node)
    } else {
        layout.swap_floating_window_with_tiling_node_across_workspaces(
            &floater.0, floater.1, workspace, node,
        )
    };
    let remapped = match swapped {
        Ok(remapped) => remapped,
        Err(error) => return Some(failure(error)),
    };
    state.swayward.remap_container_marks(remapped);
    state.swayward.queue_redraw_all();
    Some(success())
}

/// Swap two tiled containers on different workspaces, carrying container
/// marks to the nodes that replace them.
fn swap_across_workspaces(
    state: &mut State,
    (source_workspace, source_node): (
        crate::layout::workspace::WorkspaceId,
        crate::layout::tiling_tree::NodeId,
    ),
    (destination_workspace, destination_node): (
        crate::layout::workspace::WorkspaceId,
        crate::layout::tiling_tree::NodeId,
    ),
) -> Result<(), CommandOutcome> {
    let layout = &state.swayward.layout;
    if !layout.workspace_contains_tiling_node(source_workspace, source_node)
        || !layout.workspace_contains_tiling_node(destination_workspace, destination_node)
    {
        return Err(failure("Can only swap with containers and views"));
    }
    let remapped = state
        .swayward
        .layout
        .swap_tiling_nodes_between_workspaces(
            source_workspace,
            source_node,
            destination_workspace,
            destination_node,
        )
        .map_err(failure)?;
    state
        .swayward
        .remap_container_marks(remapped.first.into_iter().chain(remapped.second));
    Ok(())
}

fn marked_target(state: &State, mark: &str) -> Option<CommandTarget> {
    state
        .swayward
        .marks_by_window
        .iter()
        .find_map(|(window, marks)| {
            marks
                .iter()
                .any(|existing| existing == mark)
                .then_some(*window)
        })
        .map(CommandTarget::Window)
        .or_else(|| {
            state
                .swayward
                .marks_by_container
                .iter()
                .find_map(|(&node, marks)| {
                    marks
                        .iter()
                        .any(|existing| existing == mark)
                        .then_some(node)
                })
                .and_then(|node| {
                    let workspace = state.swayward.layout.workspace_containing_node(node)?;
                    Some(CommandTarget::Container(workspace, node))
                })
        })
}

enum MarkDestination {
    Scratchpad,
    Floating(crate::layout::workspace::WorkspaceId),
    Tiling(TilingEndpoint),
}

fn resolve_mark_destination(
    state: &State,
    destination: CommandTarget,
) -> Result<MarkDestination, CommandOutcome> {
    let CommandTarget::Window(window) = destination else {
        let CommandTarget::Container(workspace, node) = destination else {
            unreachable!()
        };
        return Ok(MarkDestination::Tiling((workspace, node)));
    };
    let mapped = super::mapped_window(state, window).or_else(|| {
        state
            .swayward
            .layout
            .scratchpad_windows()
            .find_map(|mapped| (mapped.id() == window).then(|| mapped.window.clone()))
    });
    let Some(mapped) = mapped else {
        return Err(failure("No matching node."));
    };
    if state.swayward.layout.is_scratchpad_hidden(&mapped) {
        return Ok(MarkDestination::Scratchpad);
    }
    if let Some(target) = state.swayward.layout.tiling_target_for_window(&mapped) {
        return Ok(MarkDestination::Tiling(target));
    }
    state
        .swayward
        .layout
        .window_workspace_id(&mapped)
        .map(MarkDestination::Floating)
        .ok_or_else(|| failure("No matching node."))
}

fn move_window_to_mark_workspace(
    state: &mut State,
    source: CommandTarget,
    workspace: Option<crate::layout::workspace::WorkspaceId>,
) -> CommandOutcome {
    let CommandTarget::Window(source) = source else {
        return failure(if workspace.is_some() {
            "moving container subtrees to floating marks is not implemented yet"
        } else {
            "moving container subtrees to scratchpad is not implemented yet"
        });
    };
    let Some(source) = super::mapped_window(state, source) else {
        return failure("No matching node.");
    };
    let result = match workspace {
        Some(workspace) => state
            .swayward
            .layout
            .move_window_to_workspace_id(&source, workspace),
        None => {
            state.swayward.layout.move_to_scratchpad(Some(&source));
            Ok(())
        }
    };
    if let Err(error) = result {
        return failure(error);
    }
    state.swayward.queue_redraw_all();
    success()
}

/// The wrapper a move leaves when its destination lookup fails; see
/// [`crate::layout::Layout::wrap_moved_workspace_root`].
fn wrap_moved_workspace_root(state: &mut State, source: CommandTarget) {
    if let CommandTarget::Container(workspace, node) = source {
        state
            .swayward
            .layout
            .wrap_moved_workspace_root(workspace, node, true);
        state.swayward.queue_redraw_all();
    }
}

/// A tiled container moved onto a floating view's mark becomes floating
/// beside it (`container_move_to_container`, sway/commands/move.c:243-261).
/// Returns `None` for the cases this does not cover: a floating source, or a
/// mark on a floating group's child.
fn move_tiling_to_floating_mark(
    state: &mut State,
    source: CommandTarget,
    destination: CommandTarget,
) -> Option<CommandOutcome> {
    let CommandTarget::Window(anchor) = destination else {
        return None;
    };
    let anchor = super::mapped_window(state, anchor)?;
    let layout = &state.swayward.layout;
    if !layout
        .workspaces()
        .any(|(_, _, workspace)| workspace.is_floating(&anchor))
    {
        return None;
    }
    let (workspace, node) = match source {
        CommandTarget::Container(workspace, node) => (workspace, node),
        CommandTarget::Window(window) => {
            let window = super::mapped_window(state, window)?;
            layout.tiling_target_for_window(&window)?
        }
    };
    if !layout.workspace_contains_tiling_node(workspace, node)
        || layout.is_tiling_root(workspace, node)
    {
        return None;
    }
    if let Err(error) = state
        .swayward
        .layout
        .move_tiling_node_to_floating_window(workspace, node, &anchor)
    {
        return Some(failure(error));
    }
    state.swayward.queue_redraw_all();
    Some(success())
}

/// Whether the marked view lies inside the moved container.
fn mark_is_inside_source(state: &State, source: CommandTarget, destination: CommandTarget) -> bool {
    let (CommandTarget::Container(workspace, node), CommandTarget::Window(window)) =
        (source, destination)
    else {
        return false;
    };
    let Some(window) = super::mapped_window(state, window) else {
        return false;
    };
    state
        .swayward
        .layout
        .container_windows(workspace, node)
        .is_some_and(|windows| windows.contains(&window))
}

/// `move container to output` targets the output's focus-inactive node
/// (`seat_get_focus_inactive`, sway/commands/move.c:519-525). When that is a
/// floating view, `container_move_to_container` makes a tiled source its
/// floating sibling, exactly as a move onto a floating view's mark
/// (move.c:241-261). `None` when the output's focus-inactive node is tiled.
fn move_tiling_to_output_floating_focus(
    state: &mut State,
    source: CommandTarget,
    output: &smithay::output::Output,
) -> Option<CommandOutcome> {
    let layout = &state.swayward.layout;
    let workspace = layout.workspace(layout.active_workspace_id_for_output(output)?)?;
    if !workspace.floating_is_active() {
        return None;
    }
    let anchor = workspace.floating().active_window()?;
    if !workspace.is_floating(&anchor.window) {
        return None;
    }
    let anchor = CommandTarget::Window(anchor.id());
    move_tiling_to_floating_mark(state, source, anchor)
}

pub(super) fn move_target_to_mark(
    state: &mut State,
    source: CommandTarget,
    mark: &str,
) -> CommandOutcome {
    let Some(destination) = marked_target(state, mark) else {
        wrap_moved_workspace_root(state, source);
        return failure(format!("Mark '{mark}' not found"));
    };
    if destination == source {
        // A container moved to its own mark stays put
        // (`container_move_to_container`, sway/commands/move.c:243-246).
        return success();
    }
    let destination = match resolve_mark_destination(state, destination) {
        Ok(MarkDestination::Scratchpad) => {
            return move_window_to_mark_workspace(state, source, None)
        }
        Ok(MarkDestination::Floating(workspace)) => {
            if mark_is_inside_source(state, source, destination) {
                // A floating split moved to a mark on one of its own views
                // stays put (`container_has_ancestor`,
                // sway/commands/move.c:243-246).
                return success();
            }
            if let Some(outcome) = move_tiling_to_floating_mark(state, source, destination) {
                return outcome;
            }
            return move_window_to_mark_workspace(state, source, Some(workspace));
        }
        Ok(MarkDestination::Tiling(destination)) => destination,
        Err(error) => return error,
    };
    let source = match source {
        CommandTarget::Container(workspace, node) => (workspace, node),
        CommandTarget::Window(window) => {
            let Some(mapped) = super::mapped_window(state, window) else {
                return failure("No matching node.");
            };
            if state
                .swayward
                .layout
                .workspaces()
                .any(|(_, _, ws)| ws.is_floating(&mapped))
            {
                if let Err(error) = state
                    .swayward
                    .layout
                    .move_window_to_workspace_id(&mapped, destination.0)
                {
                    return failure(error);
                }
                state.swayward.queue_redraw_all();
                return success();
            }
            let Some(source) = state.swayward.layout.tiling_target_for_window(&mapped) else {
                return failure("No matching node.");
            };
            source
        }
    };
    if source.0 == destination.0 && state.swayward.layout.is_tiling_root(source.0, source.1) {
        // Sway wraps the workspace's children first, then the move is a no-op
        // because the mark is inside the wrapper; the old workspace is still
        // arranged (sway/commands/move.c:430-436, 243-246 and 628-635).
        state
            .swayward
            .layout
            .wrap_moved_workspace_root(source.0, source.1, false);
        state.swayward.queue_redraw_all();
        return success();
    }
    let remapped = match state.swayward.layout.move_tiling_subtree_to_node(
        source.0,
        source.1,
        destination.0,
        destination.1,
    ) {
        Ok(remapped) => remapped,
        Err(error) => return failure(error),
    };
    state.swayward.remap_container_marks(remapped);
    state.swayward.queue_redraw_all();
    success()
}

pub(super) fn swap_focused(state: &mut State, target: SwapTarget) -> super::HandlerResult {
    let Some(source) = super::targeted::focused_target(state) else {
        // Sway looks the target up before it checks for a focused container
        // (`sway/sway/commands/swap.c:54-74`).
        return Err(swap_destination(state, &target)
            .err()
            .unwrap_or_else(|| failure("Can only swap with containers and views")));
    };
    super::handled_outcome(swap_target(state, source, &target))
}

pub(super) fn direction_focused(
    state: &mut State,
    direction: Direction,
    pixels: Option<i32>,
) -> super::HandlerResult {
    let Some(workspace) = state.swayward.layout.active_workspace() else {
        return Err(failure("Cannot move workspaces in a direction"));
    };
    let Some(target) = super::targeted::focused_target(state) else {
        return Err(super::failure("Cannot move workspaces in a direction"));
    };
    if matches!(target, CommandTarget::Container(workspace, node)
        if state.swayward.layout.is_tiling_root(workspace, node))
    {
        return Err(super::failure("Cannot move workspaces in a direction"));
    }
    // A fullscreen floating group root is a fullscreen floating container
    // too (`cmd_move_in_direction`, sway/commands/move.c:688-692).
    let fullscreen_floating = workspace.active_floating_is_fullscreen()
        || workspace.focused_floating_tree_root_is_fullscreen();
    if workspace.floating_is_active() || fullscreen_floating {
        if fullscreen_floating {
            return Err(failure("Cannot move fullscreen floating container"));
        }
        if state
            .swayward
            .layout
            .focused_leaf_is_only_child_of_floating_tree_root()
        {
            return Err(success());
        }
        // A floating group's child is not floating, so it moves inside the
        // group like a tiled child; only the floating root moves by pixels
        // (`container_is_floating`, sway/commands/move.c:326-330,
        // 722-728).
        if workspace.focused_floating_tree_child() {
            let direction = match direction {
                Direction::Left => crate::layout::tiling_tree::Direction::Left,
                Direction::Right => crate::layout::tiling_tree::Direction::Right,
                Direction::Up => crate::layout::tiling_tree::Direction::Up,
                Direction::Down => crate::layout::tiling_tree::Direction::Down,
            };
            state
                .swayward
                .layout
                .move_focused_floating_tree_child(direction);
            state.swayward.queue_redraw_all();
            return Ok(None);
        }
        let pixels = f64::from(pixels.unwrap_or(10));
        let (x, y) = match direction {
            Direction::Left => (-pixels, 0.),
            Direction::Right => (pixels, 0.),
            Direction::Up => (0., -pixels),
            Direction::Down => (0., pixels),
        };
        let moved = state
            .swayward
            .layout
            .focus()
            .map(|mapped| mapped.window.clone());
        state.swayward.layout.move_floating_window(
            None,
            PositionChange::AdjustFixed(x),
            PositionChange::AdjustFixed(y),
            true,
        );
        if let Some(moved) = moved {
            state.swayward.layout.rehome_floating_window(&moved);
        }
        state.swayward.queue_redraw_all();
        Ok(None)
    } else {
        super::handled_outcome(super::targeted::move_direction(
            state,
            target,
            direction,
            pixels,
            crate::layout::ActivateWindow::Smart,
            true,
        ))
    }
}

pub(super) fn position_focused(state: &mut State, position: MovePosition) -> super::HandlerResult {
    move_position(state, None, &position).map_err(failure)?;
    state.swayward.queue_redraw_all();
    Ok(None)
}

pub(super) fn to_workspace_focused(
    state: &mut State,
    target: WorkspaceTarget,
    auto_back_and_forth: bool,
) -> super::HandlerResult {
    let Some(focused) = super::targeted::focused_target(state) else {
        return Err(super::failure("Can't move an empty workspace"));
    };
    let auto_back_and_forth = auto_back_and_forth
        && state
            .swayward
            .config
            .borrow()
            .input
            .workspace_auto_back_and_forth;
    super::handled_outcome(move_target_to_workspace(
        state,
        focused,
        target,
        false,
        auto_back_and_forth,
    ))
}

/// `move container|window to workspace|output|mark` refuses a container that
/// is itself global fullscreen before it resolves the destination
/// (sway/commands/move.c:438-441). A child of a global fullscreen split holds
/// no mode of its own and moves.
pub(super) fn refuse_global_fullscreen_move(
    state: &State,
    target: CommandTarget,
) -> Result<(), CommandOutcome> {
    let layout = &state.swayward.layout;
    let mode = match target {
        CommandTarget::Container(workspace, node) => {
            layout.node_own_fullscreen_mode(workspace, node)
        }
        CommandTarget::Window(window) => super::mapped_window(state, window)
            .and_then(|window| layout.window_own_fullscreen_mode(&window)),
    };
    if mode == Some(crate::layout::tiling_tree::FullscreenMode::Global) {
        return Err(failure("Can't move fullscreen global container"));
    }
    Ok(())
}

pub(super) fn to_mark_focused(state: &mut State, mark: &str) -> super::HandlerResult {
    // Sway rejects an empty focused workspace before it resolves the mark
    // (sway/commands/move.c:430-434).
    let Some(source) = super::targeted::focused_target(state) else {
        return Err(failure("Can't move an empty workspace"));
    };
    refuse_global_fullscreen_move(state, source)?;
    super::handled_outcome(move_target_to_mark(state, source, mark))
}

pub(super) fn to_output_focused(state: &mut State, target: &OutputTarget) -> super::HandlerResult {
    // Sway rejects an empty focused workspace before it resolves the output
    // (sway/commands/move.c:430-434).
    let Some(focused_target) = super::targeted::focused_target(state) else {
        return Err(failure("Can't move an empty workspace"));
    };
    refuse_global_fullscreen_move(state, focused_target)?;
    let focused = state
        .swayward
        .layout
        .focus_with_output()
        .map(|(window, output)| (window.window.clone(), output.clone()));
    let reference = focused
        .as_ref()
        .and_then(|(window, _)| state.swayward.layout.window_center(window));
    let reference_output = focused.as_ref().map(|(_, output)| output);
    let output = output_target(state, target, reference_output, reference).map_err(|error| {
        wrap_moved_workspace_root(state, focused_target);
        failure(error)
    })?;
    if let Some(outcome) = move_tiling_to_output_floating_focus(state, focused_target, &output) {
        return super::handled_outcome(outcome);
    }
    if let CommandTarget::Container(workspace, node) = focused_target {
        super::handled_outcome(move_tiling_subtree_to_output(
            state, workspace, node, &output,
        ))?;
    } else {
        // A focused floating-group child moves alone, as a tiled container (sway's
        // `container_is_floating` is root-only, sway/tree/container.c:1041-1049).
        if let Some((window, _)) = &focused {
            state.swayward.layout.detach_floating_group_child(window);
        }
        state.swayward.layout.move_to_output(
            focused.as_ref().map(|(window, _)| window),
            &output,
            None,
            crate::layout::ActivateWindow::No,
        );
    }
    state.swayward.queue_redraw_all();
    Ok(None)
}

pub(super) fn workspace_to_output_focused(
    state: &mut State,
    target: &OutputTarget,
) -> super::HandlerResult {
    super::handled_outcome(move_workspace_to_output(state, None, target))
}

pub(super) fn to_output_targeted(
    state: &mut State,
    target: CommandTarget,
    output_target_name: &OutputTarget,
) -> super::HandlerResult {
    refuse_global_fullscreen_move(state, target)?;
    let CommandTarget::Window(target) = target else {
        return Err(failure("command requires a window target"));
    };
    let window = state
        .swayward
        .layout
        .windows()
        .find_map(|(monitor, mapped)| {
            (mapped.id() == target).then(|| {
                (
                    monitor.map(|monitor| monitor.output()),
                    mapped.window.clone(),
                )
            })
        });
    let Some((reference, window)) = window else {
        return Err(failure("No matching node."));
    };
    let reference_point = state.swayward.layout.window_center(&window);
    let output =
        output_target(state, output_target_name, reference, reference_point).map_err(failure)?;
    if let Some(outcome) =
        move_tiling_to_output_floating_focus(state, CommandTarget::Window(target), &output)
    {
        return super::handled_outcome(outcome);
    }
    state.swayward.layout.move_to_output(
        Some(&window),
        &output,
        None,
        crate::layout::ActivateWindow::No,
    );
    state.swayward.queue_redraw_all();
    Ok(None)
}

#[cfg(test)]
mod tests {
    use smithay::utils::{Point, Rectangle, Size};

    use super::clamp_pointer_position;

    #[test]
    fn pointer_position_clamps_to_cursor_output_but_not_outside_outputs() {
        let output = Rectangle::new(Point::from((1000., 50.)), Size::from((800., 600.)));
        let size = Size::from((300., 200.));
        assert_eq!(
            clamp_pointer_position(Point::from((900., -50.)), size, Some(output)),
            Point::from((1000., 50.))
        );
        assert_eq!(
            clamp_pointer_position(Point::from((1700., 550.)), size, Some(output)),
            Point::from((1500., 450.))
        );
        assert_eq!(
            clamp_pointer_position(Point::from((850., 300.)), size, None),
            Point::from((850., 300.))
        );
    }
}
