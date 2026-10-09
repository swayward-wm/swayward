//! Moves and swaps of tiling subtrees within and between workspaces.

use super::*;

/// Where `move <direction>` put a tiling container on another output.
pub struct DirectionalSubtreeMove {
    pub workspace: WorkspaceId,
    pub node: tiling_tree::NodeId,
    pub remapped: Vec<(tiling_tree::NodeId, tiling_tree::NodeId)>,
}

impl<W: LayoutElement> Layout<W> {
    pub fn detach_floating_group_child(&mut self, window: &W::Id) -> bool {
        let siblings = self.scratchpad_split_siblings(window);
        let detached = self
            .workspaces_mut()
            .find(|workspace| workspace.has_window(window))
            .is_some_and(|workspace| workspace.detach_floating_group_child(window));
        if detached {
            self.leave_scratchpad_split(window, &siblings);
        }
        detached
    }

    /// The workspace, floating root and parent node of `window` when it is a child of a
    /// floating group.
    pub fn floating_group_of_child(
        &self,
        window: &W::Id,
    ) -> Option<(WorkspaceId, tiling_tree::NodeId, tiling_tree::NodeId)> {
        self.workspaces().find_map(|(_, _, workspace)| {
            let floating = workspace.floating();
            let root = floating.tree_root_for_window(window)?;
            if floating.window_is_tree_root(window) {
                return None;
            }
            let parent = floating.tree(root)?.parent_of_window(window)?;
            Some((workspace.id(), root, parent))
        })
    }

    /// After a focused child left floating group `root`, focuses the focus-inactive view of its
    /// old parent when that parent still holds a view and the active workspace still focuses
    /// the moved child (`seat_get_focus_inactive(old_parent)`, sway/commands/move.c:589-597).
    /// An emptied parent has no such view, and sway falls back to the old workspace's
    /// focus-inactive node, which is the moved child itself.
    pub fn focus_floating_group_after_child_left(
        &mut self,
        workspace: WorkspaceId,
        root: tiling_tree::NodeId,
        parent: tiling_tree::NodeId,
        moved: &W::Id,
    ) {
        if self.active_workspace().map(Workspace::id) != Some(workspace)
            || self.focus().map(|focused| focused.id()) != Some(moved)
        {
            return;
        }
        let Some(workspace) = self.workspace_mut(workspace) else {
            return;
        };
        if workspace.focus_floating_view_in(root, parent) {
            workspace.activate_floating_layer();
        }
    }

    /// After a floating root moved, hands it to the active workspace of the output its centre
    /// is on, or of the nearest output when the centre is off every output, keeping its global
    /// position (`container_floating_move_to` and `container_floating_find_output`,
    /// sway/tree/container.c:1086-1145).
    pub fn rehome_floating_window(&mut self, window: &W::Id) {
        if self.is_scratchpad_hidden(window) {
            return;
        }
        let Some((source_workspace, global_rect)) = self.monitors().find_map(|monitor| {
            let origin = monitor.output().current_location().to_f64();
            monitor.workspaces.iter().find_map(|workspace| {
                let rect = workspace.floating().root_rect_for_window(window)?;
                Some((workspace.id(), Rectangle::new(origin + rect.loc, rect.size)))
            })
        }) else {
            return;
        };
        let Some(target) = self.floating_output_for_rect(global_rect) else {
            return;
        };
        // The destination is the output's active workspace, so a floater on a hidden workspace
        // moves to the visible one even on its own output.
        if target.active_workspace_ref().id() == source_workspace {
            return;
        }
        let target = target.output().clone();
        self.move_to_output(Some(window), &target, None, ActivateWindow::Smart);
        let origin = target.current_location().to_f64();
        let Some(workspace) = self.workspaces_mut().find(|ws| ws.has_window(window)) else {
            return;
        };
        let local = global_rect.loc - origin - workspace.working_area().loc;
        workspace.move_floating_window(
            Some(window),
            PositionChange::SetFixed(local.x),
            PositionChange::SetFixed(local.y),
            false,
        );
    }

    /// The output whose box holds the centre of `rect`, or the nearest one
    /// (`container_floating_find_output`, sway/tree/container.c:1086-1111).
    pub fn floating_output_for_rect(&self, rect: Rectangle<f64, Logical>) -> Option<&Monitor<W>> {
        let center = rect.loc + rect.size.downscale(2.);
        let mut closest: Option<(f64, &Monitor<W>)> = None;
        for monitor in self.monitors() {
            let output = monitor.output();
            let loc = output.current_location().to_f64();
            let size = output_size(output);
            if size.w <= 0. || size.h <= 0. {
                continue;
            }
            // wlr_box_closest_point keeps the closest point 1/256 inside the far edges.
            let x = center.x.clamp(loc.x, loc.x + size.w - 1. / 256.);
            let y = center.y.clamp(loc.y, loc.y + size.h - 1. / 256.);
            if x == center.x && y == center.y {
                return Some(monitor);
            }
            let distance = (x - center.x).powi(2) + (y - center.y).powi(2);
            if closest.as_ref().is_none_or(|(best, _)| distance < *best) {
                closest = Some((distance, monitor));
            }
        }
        closest.map(|(_, monitor)| monitor)
    }

    /// A fullscreen floater covers its output, so a `move position` that puts that output-sized
    /// box's centre on another output hands it, still fullscreen, to that output's active
    /// workspace (`container_floating_move_to`, sway/tree/container.c:1113-1145). `loc` is the
    /// requested global top-left corner.
    pub fn rehome_fullscreen_floater(&mut self, window: &W::Id, loc: Point<f64, Logical>) {
        let Some((source_workspace, size)) = self.monitors().find_map(|monitor| {
            let workspace = monitor.workspaces.iter().find(|ws| ws.has_window(window))?;
            Some((workspace.id(), output_size(monitor.output())))
        }) else {
            return;
        };
        let Some(target) = self.floating_output_for_rect(Rectangle::new(loc, size)) else {
            return;
        };
        if target.active_workspace_ref().id() == source_workspace {
            return;
        }
        let target = target.output().clone();
        self.move_to_output(Some(window), &target, None, ActivateWindow::Smart);
    }

    pub fn is_tiling_root(&self, workspace: WorkspaceId, node: tiling_tree::NodeId) -> bool {
        self.workspace(workspace)
            .is_some_and(|candidate| candidate.tiling().is_root(node))
    }

    /// Sway wraps a focused workspace's tiling children before it resolves
    /// a move destination (`workspace_wrap_children` in `cmd_move_container`,
    /// sway/commands/move.c:430-436), so the wrapper survives a destination
    /// that turns out to be the same workspace, a missing mark or a missing
    /// output. A failed move returns before any arrange, so its wrapper is
    /// left `unarranged`.
    pub fn wrap_moved_workspace_root(
        &mut self,
        workspace: WorkspaceId,
        node: tiling_tree::NodeId,
        unarranged: bool,
    ) {
        if let Some(workspace) = self
            .workspace_mut(workspace)
            .filter(|workspace| workspace.tiling().is_root(node))
        {
            if unarranged {
                workspace.tiling_mut().wrap_workspace_children_unarranged();
            } else {
                workspace.tiling_mut().wrap_workspace_children();
            }
        }
    }

    pub fn workspace_contains_tiling_node(
        &self,
        workspace: WorkspaceId,
        node: tiling_tree::NodeId,
    ) -> bool {
        self.workspace(workspace)
            .is_some_and(|candidate| candidate.tiling().contains(node))
    }

    pub fn swap_tiling_nodes(
        &mut self,
        workspace: WorkspaceId,
        first: tiling_tree::NodeId,
        second: tiling_tree::NodeId,
    ) -> Result<(), String> {
        let workspace = self
            .workspace_mut(workspace)
            .ok_or_else(|| "No matching node.".to_owned())?;
        workspace
            .swap_tiling_nodes(first, second)
            .map_err(str::to_owned)
    }

    /// Swaps a standalone floating view with a tiled node on the same
    /// workspace. A scratchpad floater hands its scratchpad membership to the
    /// view that replaces it (`container_swap`,
    /// sway/tree/container.c:1813-1882).
    pub fn swap_floating_window_with_tiling_node(
        &mut self,
        window: &W::Id,
        workspace: WorkspaceId,
        node: tiling_tree::NodeId,
    ) -> Result<Vec<(tiling_tree::NodeId, tiling_tree::NodeId)>, String> {
        let scratchpad = self.scratchpad_windows.contains(window);
        if scratchpad
            && self
                .workspace(workspace)
                .is_some_and(|candidate| candidate.tiling().is_split(node))
        {
            return Err(
                "swapping a scratchpad window with a container is not implemented yet".into(),
            );
        }
        let (floated, remapped) = self
            .workspace_mut(workspace)
            .and_then(|candidate| candidate.swap_floating_window_with_tiling_node(window, node))
            .ok_or_else(|| "Can only swap with containers and views".to_owned())?;
        if let Some(floated) = floated.filter(|_| scratchpad) {
            for id in &mut self.scratchpad_windows {
                if id == window {
                    *id = floated.clone();
                }
            }
            // The view taking a shown scratchpad view's place is shown in turn,
            // which focuses it (`root_scratchpad_show`, sway/tree/root.c:157-204,
            // from `container_swap`, sway/tree/container.c:1871-1876).
            if let Some(candidate) = self.workspace_mut(workspace) {
                candidate.activate_window(&floated);
            }
        }
        Ok(remapped)
    }

    /// Swaps standalone floating view `window` on `floater_workspace` with
    /// tiled `node` on another workspace: the view takes the tiled slot and
    /// the container floats in the view's box. A focused endpoint hands seat
    /// focus to whichever container now holds its place, so the focused
    /// workspace stays put (`swap_places`, `swap_focus`,
    /// sway/tree/container.c:1718-1798).
    pub fn swap_floating_window_with_tiling_node_across_workspaces(
        &mut self,
        window: &W::Id,
        floater_workspace: WorkspaceId,
        tiled_workspace: WorkspaceId,
        node: tiling_tree::NodeId,
    ) -> Result<Vec<(tiling_tree::NodeId, tiling_tree::NodeId)>, String> {
        if self.scratchpad_windows.contains(window) {
            return Err(
                "swapping a scratchpad window across workspaces is not implemented yet".into(),
            );
        }
        let no_node = || "No matching node.".to_owned();
        let active = self.active_workspace().map(Workspace::id);
        let tiled = self.workspace(tiled_workspace).ok_or_else(no_node)?;
        if !tiled.tiling().contains(node) || tiled.tiling().is_root(node) {
            return Err("Can only swap with containers and views".into());
        }
        let tiled_focused = active == Some(tiled_workspace)
            && !tiled.floating_is_active()
            && tiled
                .tiling()
                .focus()
                .is_some_and(|focus| tiled.tiling().contains_node(node, focus));
        let MonitorSet::Normal { monitors, .. } = &mut self.monitor_set else {
            return Err("cannot swap containers without an output".into());
        };
        let (floater_ws, tiled_ws) =
            Self::distinct_workspaces_mut(monitors, floater_workspace, tiled_workspace)
                .ok_or_else(no_node)?;
        let mut source = floater_ws
            .take_floating_window_for_swap(window)
            .ok_or_else(|| "Can only swap with containers and views".to_owned())?;
        let Some((mut subtree, slot)) = tiled_ws.detach_tiling_subtree_for_swap(node) else {
            floater_ws.float_swapped_subtree(source.leaf, source.placement, source.focused);
            return Err(no_node());
        };
        source.leaf.swap_fullscreen_position(&mut subtree);
        // Across workspaces the seat focuses the container that took the
        // focused one's place, and the focused one stays next on the focus
        // stack, so each workspace's focus-inactive view is a swapped one.
        let floater_seat_focused = source.focused && active == Some(floater_workspace);
        tiled_ws.tile_swapped_window(
            source.leaf,
            window,
            slot,
            tiled_focused || floater_seat_focused,
        );
        let (_, remapped) = floater_ws.float_swapped_subtree(
            subtree,
            source.placement,
            tiled_focused || source.focused,
        );
        Ok(remapped)
    }

    pub(crate) fn swap_tiling_nodes_between_workspaces(
        &mut self,
        first_workspace: WorkspaceId,
        first: tiling_tree::NodeId,
        second_workspace: WorkspaceId,
        second: tiling_tree::NodeId,
    ) -> Result<SwapRemap, String> {
        let fits = |workspace: WorkspaceId, slot: tiling_tree::NodeId, other_ws, other| {
            let height = self
                .workspace(other_ws)
                .map(|candidate| candidate.tiling().node_height(other));
            self.workspace(workspace)
                .zip(height)
                .is_some_and(|(candidate, height)| candidate.tiling().fits_at(slot, height))
        };
        if !fits(first_workspace, first, second_workspace, second)
            || !fits(second_workspace, second, first_workspace, first)
        {
            return Err(tiling_tree::TOO_DEEP.to_owned());
        }
        let MonitorSet::Normal { monitors, .. } = &mut self.monitor_set else {
            return Err("cannot swap containers without an output".into());
        };
        let (first_ws, second_ws) =
            Self::distinct_workspaces_mut(monitors, first_workspace, second_workspace)
                .ok_or_else(|| "No matching node.".to_owned())?;
        let (mut first_subtree, first_slot) = first_ws
            .detach_tiling_subtree_for_swap(first)
            .ok_or_else(|| "No matching node.".to_owned())?;
        let (mut second_subtree, second_slot) = second_ws
            .detach_tiling_subtree_for_swap(second)
            .ok_or_else(|| "No matching node.".to_owned())?;
        first_subtree.swap_fullscreen_position(&mut second_subtree);
        first_subtree.trade_floating_flag(&mut second_subtree);
        let second_remapped = first_ws
            .attach_tiling_subtree_for_swap(second_subtree, first_slot)
            .1;
        let first_remapped = second_ws
            .attach_tiling_subtree_for_swap(first_subtree, second_slot)
            .1;
        first_ws.tiling_mut().finish_subtree_detach(None);
        second_ws.tiling_mut().finish_subtree_detach(None);
        Ok(SwapRemap {
            first: first_remapped,
            second: second_remapped,
        })
    }

    /// Mutable borrows of two distinct workspaces, wherever they live.
    ///
    /// Returns `None` when either is missing or both name the same workspace,
    /// so callers report "No matching node." instead of panicking on a stale
    /// id or an aliasing borrow.
    fn distinct_workspaces_mut(
        monitors: &mut [Monitor<W>],
        first: WorkspaceId,
        second: WorkspaceId,
    ) -> Option<(&mut Workspace<W>, &mut Workspace<W>)> {
        if first == second {
            return None;
        }
        let first_monitor = monitors.iter().position(|monitor| monitor.has_ws(first))?;
        let second_monitor = monitors.iter().position(|monitor| monitor.has_ws(second))?;
        if first_monitor == second_monitor {
            let monitor = &mut monitors[first_monitor];
            let first_idx = monitor.idx_of_ws(first)?;
            let second_idx = monitor.idx_of_ws(second)?;
            if first_idx < second_idx {
                let (before, after) = monitor.workspaces.split_at_mut(second_idx);
                Some((&mut before[first_idx], after.first_mut()?))
            } else {
                let (before, after) = monitor.workspaces.split_at_mut(first_idx);
                Some((after.first_mut()?, &mut before[second_idx]))
            }
        } else if first_monitor < second_monitor {
            let (before, after) = monitors.split_at_mut(second_monitor);
            let first_monitor = &mut before[first_monitor];
            let second_monitor = after.first_mut()?;
            let first_idx = first_monitor.idx_of_ws(first)?;
            let second_idx = second_monitor.idx_of_ws(second)?;
            Some((
                &mut first_monitor.workspaces[first_idx],
                &mut second_monitor.workspaces[second_idx],
            ))
        } else {
            let (before, after) = monitors.split_at_mut(first_monitor);
            let first_monitor = after.first_mut()?;
            let second_monitor = &mut before[second_monitor];
            let first_idx = first_monitor.idx_of_ws(first)?;
            let second_idx = second_monitor.idx_of_ws(second)?;
            Some((
                &mut first_monitor.workspaces[first_idx],
                &mut second_monitor.workspaces[second_idx],
            ))
        }
    }

    pub fn move_tiling_subtree_to_node(
        &mut self,
        source_workspace: WorkspaceId,
        source: tiling_tree::NodeId,
        target_workspace: WorkspaceId,
        target: tiling_tree::NodeId,
    ) -> Result<Vec<(tiling_tree::NodeId, tiling_tree::NodeId)>, String> {
        if source_workspace == target_workspace {
            let workspace = self
                .workspace_mut(source_workspace)
                .ok_or_else(|| "No matching node.".to_owned())?;
            if !workspace.tiling().contains(source) || !workspace.tiling().contains(target) {
                return Err("No matching node.".to_owned());
            }
            workspace.tiling_mut().move_subtree_to_node(source, target);
            Ok(Vec::new())
        } else {
            let MonitorSet::Normal { monitors, .. } = &mut self.monitor_set else {
                return Err("cannot move a container without an output".into());
            };
            let source_monitor = monitors
                .iter()
                .position(|monitor| monitor.has_ws(source_workspace))
                .ok_or_else(|| "No matching node.".to_owned())?;
            let (source_ws, target_ws) =
                Self::distinct_workspaces_mut(monitors, source_workspace, target_workspace)
                    .ok_or_else(|| "No matching node.".to_owned())?;
            let moved_view = source_ws.tiling().window_for_node(source).map(|window| {
                let window = window.id().clone();
                let rect = source_ws.tiling().ipc_rect_for_window(&window);
                (window, rect)
            });
            let (subtree, old_parent) = source_ws
                .detach_tiling_subtree(source)
                .ok_or_else(|| "No matching node.".to_owned())?;
            let mut arrived = Vec::new();
            subtree.for_each_window(|window| arrived.push(window.id().clone()));
            let remapped = target_ws.attach_tiling_subtree_at(subtree, Some(target)).1;
            // `container_move_to_container` leaves the seat-wide focus stack alone, so each
            // arrival keeps its own place on it: a view focused more recently than the tabs
            // beside it is the visible tab (sway/commands/move.c:241-275;
            // `view_is_visible`, sway/tree/view.c:1180-1193).
            for window in arrived.iter().rev() {
                target_ws
                    .tiling_mut()
                    .rank_arrived_window_by_focus_timestamp(window);
            }
            if let Some((window, Some(rect))) = moved_view {
                // `container_move_to_container` zeroes the view's size and
                // `arrange_workspace` lays out only the destination's
                // fullscreen container (sway/commands/move.c:248-266,
                // sway/tree/arrange.c:310-316), as a workspace move does.
                target_ws
                    .tiling_mut()
                    .mark_moved_under_fullscreen(&window, rect);
            }
            source_ws.tiling_mut().finish_subtree_detach(old_parent);
            if monitors[source_monitor].workspace_switch.is_none() {
                monitors[source_monitor].clean_up_workspaces();
            }
            Ok(remapped)
        }
    }

    /// Moves tiling container `node` onto the floating view `anchor`, which
    /// may be on another workspace. Sway detaches the container and inserts it
    /// into the anchor's floating list right after the anchor
    /// (`container_move_to_container`, sway/commands/move.c:243-261).
    pub fn move_tiling_node_to_floating_window(
        &mut self,
        source_workspace: WorkspaceId,
        node: tiling_tree::NodeId,
        anchor: &W::Id,
    ) -> Result<(), String> {
        let no_node = || "No matching node.".to_owned();
        let target_workspace = self.window_workspace_id(anchor).ok_or_else(no_node)?;
        let anchor_slot = floating_tree::StackSlot::Window(anchor.clone());
        let same_workspace = source_workspace == target_workspace;
        let workspace = self.workspace_mut(source_workspace).ok_or_else(no_node)?;
        let moved = workspace
            .float_tiling_node_for_mark(node, same_workspace.then_some(&anchor_slot))
            .ok_or_else(no_node)?;
        if same_workspace {
            return Ok(());
        }
        let window = match &moved {
            floating_tree::StackSlot::Window(window) => window.clone(),
            floating_tree::StackSlot::Tree(root) => workspace
                .floating()
                .tree_window_ids(*root)
                .and_then(|windows| windows.into_iter().next())
                .ok_or_else(no_node)?,
        };
        self.move_window_to_workspace_id(&window, target_workspace)?;
        let target = self.workspace_mut(target_workspace).ok_or_else(no_node)?;
        let moved = match target.floating_tree_root_for_window(&window) {
            Some(root) => floating_tree::StackSlot::Tree(root),
            None => floating_tree::StackSlot::Window(window.clone()),
        };
        target.restack_floating_above(&moved, &anchor_slot);
        if matches!(moved, floating_tree::StackSlot::Window(_)) {
            // The moved view keeps its place at the top of the seat's focus stack, so it is the
            // target workspace's focus-inactive floater (sway/commands/move.c:553-606).
            target.activate_window_without_raising(&window);
        }
        // `container_add_sibling` refreshes the representation up to the workspace
        // (sway/tree/container.c:750-773,1410-1423), even one that so far held only floaters.
        target.tiling_mut().restore_has_had_tile(true);
        Ok(())
    }

    /// Sway's `move container to workspace` onto the container's own
    /// workspace: the destination is the workspace's focus-inactive tiling
    /// container (`seat_get_focus_inactive_tiling`, sway/commands/move.c:516),
    /// and `container_move_to_container` puts the container after that view
    /// or into that split (move.c:241-261). It does nothing when the
    /// destination is the container or inside it, or when the workspace has
    /// no focus-inactive tiling container (`container_move_to_workspace`
    /// returns early for the same workspace, move.c:198-202).
    pub fn move_tiling_node_to_focus_inactive(
        &mut self,
        workspace: WorkspaceId,
        node: tiling_tree::NodeId,
    ) {
        let Some(workspace) = self.workspace_mut(workspace) else {
            return;
        };
        let tiling = workspace.tiling();
        if !tiling.contains(node) || tiling.is_root(node) {
            return;
        }
        let Some(destination) = tiling.focus_inactive_tiling() else {
            return;
        };
        if tiling.contains_node(node, destination) {
            return;
        }
        if workspace.floating_is_active() {
            workspace
                .tiling_mut()
                .move_subtree_to_node_keeping_focus(node, destination);
        } else {
            workspace
                .tiling_mut()
                .move_subtree_to_node(node, destination);
        }
    }

    pub fn tiling_target_for_window(
        &self,
        window: &W::Id,
    ) -> Option<(WorkspaceId, tiling_tree::NodeId)> {
        self.workspaces().find_map(|(_, _, workspace)| {
            workspace
                .tiling()
                .node_for_window(window)
                .map(|node| (workspace.id(), node))
        })
    }

    pub fn swap_target_for_window(
        &self,
        window: &W::Id,
    ) -> Option<(WorkspaceId, tiling_tree::NodeId)> {
        self.workspaces().find_map(|(_, _, workspace)| {
            workspace
                .swap_node_for_window(window)
                .map(|node| (workspace.id(), node))
        })
    }

    pub fn move_tiling_subtree_to_sway_workspace(
        &mut self,
        source_workspace: WorkspaceId,
        node: tiling_tree::NodeId,
        target: crate::command::WorkspaceTarget,
        preserve_empty_workspace: bool,
        auto_back_and_forth: bool,
    ) -> Result<(WorkspaceId, Vec<(tiling_tree::NodeId, tiling_tree::NodeId)>), String> {
        if let Some(window) = self.floating_group_window(source_workspace, node) {
            self.refuse_sticky_move_on_same_output(&window, &target, auto_back_and_forth)?;
            let target_workspace =
                self.move_floating_group_to_sway_workspace(&window, target, auto_back_and_forth)?;
            return Ok((target_workspace, Vec::new()));
        }
        // A focused workspace moves as `workspace_wrap_children`'s wrapper,
        // which takes only the tiling children: floating containers stay
        // behind and keep the source workspace alive
        // (sway/commands/move.c:430-436, sway/tree/workspace.c:898-910).
        let (keeps_floating, empty_root) = self
            .workspace(source_workspace)
            .filter(|workspace| workspace.tiling().is_root(node))
            .map(|workspace| {
                (
                    !workspace.floating_transfer_window_ids().is_empty(),
                    workspace.tiling().tiles().next().is_none(),
                )
            })
            .unwrap_or_default();
        if empty_root {
            return Err("Can't move an empty workspace".to_owned());
        }
        let target =
            self.resolve_move_workspace_target(source_workspace, target, auto_back_and_forth);
        let (target_output, target_index) = self.resolve_sway_workspace_target(target)?;
        let target_workspace = match target_output.as_ref() {
            Some(output) => self
                .monitor_for_output(output)
                .and_then(|monitor| monitor.workspaces.get(target_index))
                .map(Workspace::id),
            None => self
                .workspaces()
                .nth(target_index)
                .map(|(_, _, workspace)| workspace.id()),
        }
        .ok_or_else(|| "target workspace does not exist".to_owned())?;
        if source_workspace == target_workspace {
            // The wrapper survives even when the destination is the same
            // workspace.
            self.wrap_moved_workspace_root(source_workspace, node, false);
            self.move_tiling_node_to_focus_inactive(source_workspace, node);
            return Ok((target_workspace, Vec::new()));
        }
        let MonitorSet::Normal { monitors, .. } = &mut self.monitor_set else {
            return Err("cannot move a container without an output".into());
        };
        let source_monitor = monitors
            .iter()
            .position(|monitor| monitor.has_ws(source_workspace))
            .ok_or_else(|| "No matching node.".to_owned())?;
        let target_monitor = monitors
            .iter()
            .position(|monitor| monitor.has_ws(target_workspace))
            .ok_or_else(|| "target workspace does not exist".to_owned())?;
        if source_monitor == target_monitor {
            let remapped = monitors[source_monitor]
                .move_tiling_subtree_to_workspace(
                    source_workspace,
                    node,
                    target_workspace,
                    preserve_empty_workspace || keeps_floating,
                )
                .ok_or_else(|| "No matching node.".to_owned())?;
            self.focus_workspace_left_with_floating(source_workspace, keeps_floating);
            return Ok((target_workspace, remapped));
        }

        let remapped = Self::move_tiling_subtree_between_monitors(
            monitors,
            (source_monitor, source_workspace),
            (target_monitor, target_workspace),
            node,
            preserve_empty_workspace || keeps_floating,
        )?;
        self.focus_workspace_left_with_floating(source_workspace, keeps_floating);
        Ok((target_workspace, remapped))
    }

    /// The seat focused the workspace node, not the wrapper that left, so
    /// sway's restore leaves the workspace itself focused
    /// (sway/commands/move.c:598-607).
    fn focus_workspace_left_with_floating(&mut self, workspace: WorkspaceId, keeps_floating: bool) {
        if keeps_floating {
            if let Some(workspace) = self.workspace_mut(workspace) {
                workspace.focus_workspace_itself();
            }
        }
    }
    /// `move <direction>` of a tiling container off the workspace edge onto
    /// the adjacent output's active workspace
    /// (`container_move_to_next_output`, sway/commands/move.c:277-298).
    pub fn move_tiling_subtree_to_output_from_direction(
        &mut self,
        source_workspace: WorkspaceId,
        node: tiling_tree::NodeId,
        output: &Output,
        direction: tiling_tree::Direction,
    ) -> Option<DirectionalSubtreeMove> {
        let target_workspace = self.active_workspace_id_for_output(output)?;
        let MonitorSet::Normal { monitors, .. } = &mut self.monitor_set else {
            return None;
        };
        let source_monitor = monitors
            .iter()
            .position(|monitor| monitor.has_ws(source_workspace))?;
        let (source, target) =
            Self::distinct_workspaces_mut(monitors, source_workspace, target_workspace)?;
        if !source.tiling().contains(node) || source.tiling().is_root(node) {
            return None;
        }
        let (subtree, old_parent) = source.detach_tiling_subtree(node)?;
        let (id, remapped) = target.attach_tiling_subtree_from_direction(subtree, direction);
        source.tiling_mut().finish_subtree_detach(old_parent);
        if monitors[source_monitor].workspace_switch.is_none() {
            monitors[source_monitor].clean_up_workspaces();
        }
        Some(DirectionalSubtreeMove {
            workspace: target_workspace,
            node: id,
            remapped,
        })
    }

    /// The window of `node` when it is a floating group root on
    /// `source_workspace`.
    fn floating_group_window(
        &self,
        source_workspace: WorkspaceId,
        node: tiling_tree::NodeId,
    ) -> Option<W::Id> {
        self.workspace(source_workspace).and_then(|workspace| {
            let floating = workspace.floating();
            floating.tree(node)?;
            floating.window_in_node(node).cloned()
        })
    }

    /// Moves the floating group holding `window` and returns the workspace it
    /// landed on.
    fn move_floating_group_to_sway_workspace(
        &mut self,
        window: &W::Id,
        target: crate::command::WorkspaceTarget,
        auto_back_and_forth: bool,
    ) -> Result<WorkspaceId, String> {
        self.move_window_to_sway_workspace(window, target, auto_back_and_forth)?;
        self.workspaces()
            .find(|(_, _, workspace)| workspace.has_window(window))
            .map(|(_, _, workspace)| workspace.id())
            .ok_or_else(|| "No matching node.".to_owned())
    }

    /// Moves the tiling subtree at `node` between workspaces on two different
    /// monitors, cleaning up the source monitor unless `preserve_source`.
    fn move_tiling_subtree_between_monitors(
        monitors: &mut [Monitor<W>],
        (source_monitor, source_workspace): (usize, WorkspaceId),
        (target_monitor, target_workspace): (usize, WorkspaceId),
        node: tiling_tree::NodeId,
        preserve_source: bool,
    ) -> Result<Vec<(tiling_tree::NodeId, tiling_tree::NodeId)>, String> {
        let (source, target) = if source_monitor < target_monitor {
            let (before_target, target_and_after) = monitors.split_at_mut(target_monitor);
            (&mut before_target[source_monitor], &mut target_and_after[0])
        } else {
            let (before_source, source_and_after) = monitors.split_at_mut(source_monitor);
            (&mut source_and_after[0], &mut before_source[target_monitor])
        };
        let source_idx = source
            .idx_of_ws(source_workspace)
            .ok_or_else(|| "No matching node.".to_owned())?;
        let target_idx = target
            .idx_of_ws(target_workspace)
            .ok_or_else(|| "target workspace does not exist".to_owned())?;
        let (subtree, old_parent) = source.workspaces[source_idx]
            .detach_tiling_subtree(node)
            .ok_or_else(|| "No matching node.".to_owned())?;
        let remapped = target.workspaces[target_idx]
            .attach_tiling_subtree(subtree)
            .1;
        source.workspaces[source_idx]
            .tiling_mut()
            .finish_subtree_detach(old_parent);
        if !preserve_source && source.workspace_switch.is_none() {
            source.clean_up_workspaces();
        }
        Ok(remapped)
    }
    /// Drops an interactively moved tile onto `target`, swapping the two.
    ///
    /// Sway swaps a centre drop with the container under the pointer
    /// (seatop_move_tiling.c:365-388). Returns the displaced tile when it must
    /// go back to `source_workspace` on another workspace.
    pub(super) fn drop_tile_swapping_with(
        mon: &mut Monitor<W>,
        ws_idx: usize,
        target: tiling_tree::NodeId,
        tile: Tile<W>,
        source_workspace: WorkspaceId,
        workspace_activation: WorkspaceActivation,
    ) -> Option<RemovedTile<W>> {
        let mut displaced = None;
        let ws_id = mon.workspaces[ws_idx].id();
        let target_window = mon.workspaces[ws_idx]
            .tiling()
            .window_for_node(target)
            .map(|window| window.id().clone());
        let moved_window = tile.window().id().clone();
        if source_workspace != ws_id {
            if let Some(target_window) = &target_window {
                displaced =
                    Some(mon.workspaces[ws_idx].remove_tile(target_window, Transaction::new()));
            }
        }
        mon.add_tile(
            tile,
            MonitorAddWindowTarget::Workspace {
                id: ws_id,
                column_idx: None,
            },
            ActivateWindow::Yes,
            workspace_activation,
            false,
        );
        if source_workspace == ws_id {
            if let Some(target_window) = target_window {
                let workspace = &mut mon.workspaces[ws_idx];
                if let (Some(first), Some(second)) = (
                    workspace.tiling().node_for_window(&moved_window),
                    workspace.tiling().node_for_window(&target_window),
                ) {
                    let _ = workspace.swap_tiling_nodes(first, second);
                }
            }
        }
        displaced
    }
}
