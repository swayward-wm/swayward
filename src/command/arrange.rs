//! The `arrange_workspace` and `arrange_root` calls that end sway's command handlers, for
//! handlers whose layout change swayward models without recording the arrange.
//!
//! An arrange puts a fullscreen container back at the output box, or the root box for global
//! fullscreen (sway/tree/arrange.c:310-316, 349-355). That undoes the pending box a px resize
//! of a fullscreen floater moved (sway/commands/resize.c:180-230), so every handler that ends
//! in one must record it. Handlers that already arrange through the layout (`split`, `layout`,
//! `floating`, `workspace`, gaps and the layout settings, view map and unmap) are not listed.

use super::{Command, CommandTarget, SwapTarget};
use crate::layout::workspace::WorkspaceId;
use crate::swayward::State;

/// What a handler will arrange, measured before it runs, because the arranged workspaces
/// depend on where the containers were.
pub(super) struct PendingArrange {
    workspaces: Vec<WorkspaceId>,
    root: bool,
    /// `move <direction>` arranges only when the container moved
    /// (sway/commands/move.c:715-718).
    unless_unchanged: Option<Vec<Fingerprint>>,
    /// `move to mark` arranges the parent of the moved container's new place
    /// (sway/commands/move.c:617-625).
    moved: Option<CommandTarget>,
}

type Fingerprint = (WorkspaceId, Vec<crate::layout::tiling_tree::NodeId>);

pub(super) fn before(
    state: &State,
    command: &Command,
    target: Option<CommandTarget>,
) -> Option<PendingArrange> {
    let target = target?;
    let pending = |workspaces| PendingArrange {
        workspaces,
        root: false,
        unless_unchanged: None,
        moved: None,
    };
    match command {
        // `cmd_fullscreen` ends in `arrange_root` whenever it has a container
        // (sway/commands/fullscreen.c:22-55).
        Command::Fullscreen { .. } => Some(PendingArrange {
            root: true,
            ..pending(Vec::new())
        }),
        Command::Swap(swap) => Some(pending(swap_workspaces(state, target, swap))),
        // A tiled `move <direction>` arranges the old and new workspaces
        // (sway/commands/move.c:712-736); a floating one only moves the floater.
        Command::MoveDirection { .. } => {
            let workspace = tiled_workspace(state, target)?;
            Some(PendingArrange {
                unless_unchanged: Some(fingerprints(state)),
                moved: Some(target),
                ..pending(vec![workspace])
            })
        }
        // `move to mark` arranges the old workspace and the destination's parent
        // (sway/commands/move.c:617-625).
        Command::MoveToMark(_) => {
            let workspace = tiled_workspace(state, target)?;
            Some(PendingArrange {
                moved: Some(target),
                ..pending(vec![workspace])
            })
        }
        _ => None,
    }
}

pub(super) fn after(state: &mut State, pending: PendingArrange) {
    if pending
        .unless_unchanged
        .is_some_and(|before| before == fingerprints(state))
    {
        return;
    }
    let mut workspaces = pending.workspaces;
    if let Some(workspace) = pending
        .moved
        .and_then(|target| workspace_child_workspace(state, target))
    {
        workspaces.push(workspace);
    }
    let layout = &mut state.swayward.layout;
    // Every handler here ends in `arrange_root` under a global fullscreen container.
    let root = pending.root || layout.global_fullscreen_active();
    layout.note_sway_arrange(&workspaces, root);
}

/// The workspace holding tiled `target`, outside any floating group.
fn tiled_workspace(state: &State, target: CommandTarget) -> Option<WorkspaceId> {
    let layout = &state.swayward.layout;
    match target {
        CommandTarget::Container(workspace, node) => layout
            .workspace(workspace)
            .is_some_and(|candidate| candidate.tiling().contains(node))
            .then_some(workspace),
        CommandTarget::Window(id) => {
            let window = super::mapped_window(state, id)?;
            layout.workspaces().find_map(|(_, _, workspace)| {
                (workspace.tiling().node_for_window(&window).is_some()
                    && !workspace.is_floating_for_ipc(&window))
                .then(|| workspace.id())
            })
        }
    }
}

/// The workspace `target` sits directly on, as a tiled child of the workspace or a floating
/// root: `node_get_parent` is then the workspace, and `arrange_node` arranges it.
fn workspace_child_workspace(state: &State, target: CommandTarget) -> Option<WorkspaceId> {
    let layout = &state.swayward.layout;
    match target {
        CommandTarget::Container(workspace, node) => {
            let candidate = layout.workspace(workspace)?;
            let on_workspace = candidate
                .tiling()
                .parent_of_node(node)
                .is_some_and(|parent| candidate.tiling().is_root(parent))
                || candidate.floating().tree_roots().any(|root| root == node);
            on_workspace.then_some(workspace)
        }
        CommandTarget::Window(id) => {
            let window = super::mapped_window(state, id)?;
            layout.workspaces().find_map(|(_, _, workspace)| {
                let tiling = workspace.tiling();
                let on_workspace = workspace.floating().window_is_floating_root(&window)
                    || tiling
                        .node_for_window(&window)
                        .and_then(|node| tiling.parent_of_node(node))
                        .is_some_and(|parent| tiling.is_root(parent));
                on_workspace.then(|| workspace.id())
            })
        }
    }
}

/// `cmd_swap` ends in `arrange_node` on each endpoint's new parent (sway/commands/swap.c:
/// 92-103). The endpoints trade places, so a workspace is arranged when either endpoint sat
/// directly on it; a container parent gets only `arrange_container`.
fn swap_workspaces(state: &State, source: CommandTarget, swap: &SwapTarget) -> Vec<WorkspaceId> {
    let destination = super::movement::swap_destination(state, swap).ok();
    [Some(source), destination]
        .into_iter()
        .flatten()
        .filter_map(|target| workspace_child_workspace(state, target))
        .collect()
}

/// Every workspace's tiling tree shape, to tell whether a `move` moved anything.
fn fingerprints(state: &State) -> Vec<Fingerprint> {
    state
        .swayward
        .layout
        .workspaces()
        .map(|(_, _, workspace)| {
            let nodes = workspace
                .ipc_tiling_tree()
                .nodes()
                .into_iter()
                .map(|(node, _)| node)
                .collect();
            (workspace.id(), nodes)
        })
        .collect()
}
