//! The sway scratchpad: hiding windows and floating trees and showing them again.

use super::*;

/// A container's standing relative to the hidden scratchpad. Sway marks only
/// the toplevel container it hides as a scratchpad container
/// (sway/tree/root.c:98-123), so the descendants of a hidden group are not
/// themselves hidden (`container_is_scratchpad_hidden`,
/// sway/tree/container.c:1696-1698).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScratchpadPlace {
    /// A hidden scratchpad container: a hidden view or a hidden group's root.
    Hidden,
    /// A split or view below the root of a hidden scratchpad group.
    InHiddenGroup,
    /// Not in the hidden scratchpad.
    NotHidden,
}

impl<W: LayoutElement> Layout<W> {
    pub fn move_to_scratchpad(&mut self, window: Option<&W::Id>) {
        let window = window
            .cloned()
            .or_else(|| self.focus().map(|window| window.id().clone()));
        let Some(window) = window else {
            return;
        };
        self.finish_starting_interactive_move(&window);
        if self
            .scratchpad
            .iter()
            .any(|removed| removed.tile.window().id() == &window)
            || self
                .scratchpad_trees
                .iter()
                .any(|removed| removed.contains_window(&window))
        {
            return;
        }
        let floating_tree = self.workspaces().find_map(|(_, _, workspace)| {
            workspace
                .floating_tree_root_for_window(&window)
                .map(|root| (workspace.id(), root))
        });
        if let Some((source_workspace, root)) = floating_tree {
            let removed = {
                let Some(workspace) = self.workspace_mut(source_workspace) else {
                    return;
                };
                workspace.clear_floating_tree_fullscreen(root);
                let Some(removed) = workspace.remove_floating_tree(root) else {
                    warn!("move_to_scratchpad: floating tree root reported for the window is gone");
                    return;
                };
                removed
            };
            for id in removed.window_ids() {
                self.push_scratchpad_window(id.clone());
            }
            self.scratchpad_trees.push_back(removed);
            self.clean_up_removed_window_workspace(source_workspace);
            return;
        }
        let automatic_maximum = self.output_layout_size();
        let mut floating_working_area = None;
        let mut empty_parent = None;
        if let Some(workspace) = self.workspaces_mut().find(|ws| ws.has_window(&window)) {
            empty_parent = workspace.tiling().fullscreen_view_pending_wrapper(&window);
            workspace.prepare_tiled_window_for_scratchpad(&window, automatic_maximum);
            if workspace.fullscreen_contains_window(&window) {
                workspace.set_fullscreen(&window, false);
            }
            floating_working_area = Some(workspace.working_area());
        }
        let Some((mut removed, source_workspace)) =
            self.detach_window_inner(&window, Transaction::new(), true)
        else {
            return;
        };
        // Sway floats a tiled view before hiding it, which stores B_CSD for a
        // CSD view (root_scratchpad_add_container, sway/tree/root.c:114-118).
        removed.tile.set_sway_csd_floating(true);
        removed.floating_working_area = floating_working_area;
        // Sway then arranges the old parent (sway/tree/root.c:128-140); a
        // wrapper the fullscreen view left was never arranged, so its box is
        // still empty.
        if let Some((workspace, wrapper)) = source_workspace
            .and_then(|id| self.workspace_mut(id))
            .zip(empty_parent)
        {
            workspace.tiling_mut().arrange_wrapper_at_empty_box(wrapper);
        }
        self.push_scratchpad_window(window);
        self.scratchpad.push_back(removed);
        if let Some(source_workspace) = source_workspace {
            self.clean_up_removed_window_workspace(source_workspace);
        }
    }

    /// Moves scratchpad tree `index` onto `workspace` and returns the window
    /// it shows. The tree stays hidden if the workspace or its first window is
    /// missing, instead of being dropped.
    fn show_scratchpad_tree(&mut self, index: usize, workspace: WorkspaceId) -> Option<W::Id> {
        let shown = self
            .scratchpad_trees
            .get(index)?
            .window_ids()
            .first()?
            .clone();
        if !self
            .workspaces()
            .any(|(_, _, candidate)| candidate.id() == workspace)
        {
            return None;
        }
        let removed = self.scratchpad_trees.remove(index)?;
        let workspace = self.workspace_mut(workspace)?;
        let root = workspace.add_floating_tree(removed, true);
        // root_scratchpad_show focuses seat_get_focus_inactive(con), the
        // group's most recently focused view, even when the group itself was
        // focused when it was hidden (sway/tree/root.c:185-186).
        workspace.focus_floating_tree_view(root);
        Some(shown)
    }

    pub fn show_scratchpad(&mut self, window: Option<&W::Id>) -> Option<W::Id> {
        let focused = self.focus().map(|window| window.id().clone());
        let shown = focused
            .filter(|id| self.scratchpad_windows.contains(id) && !self.is_scratchpad_hidden(id))
            .or_else(|| {
                self.scratchpad_windows
                    .iter()
                    .find(|id| !self.is_scratchpad_hidden(id))
                    .cloned()
            });
        let target_tree = window
            .and_then(|window| {
                self.scratchpad_trees
                    .iter()
                    .position(|removed| removed.contains_window(window))
            })
            .or_else(|| {
                // With nothing shown, sway takes the bottom of the scratchpad
                // list (`root->scratchpad->items[0]`,
                // sway/commands/scratchpad.c:64-71).
                if window.is_some() || shown.is_some() {
                    return None;
                }
                let first = self.scratchpad_windows.first()?;
                self.scratchpad_trees
                    .iter()
                    .position(|removed| removed.contains_window(first))
            });
        if let Some(index) = target_tree {
            let active_workspace = self.prepare_active_workspace_for_scratchpad_show()?;
            return self.show_scratchpad_tree(index, active_workspace);
        }
        let mut target_index = window.and_then(|window| {
            self.scratchpad
                .iter()
                .position(|removed| removed.tile.window().id() == window)
        });
        if let Some(window) = window {
            if target_index.is_none() {
                let on_active_workspace = self
                    .active_workspace()
                    .is_some_and(|workspace| workspace.has_window(window));
                if on_active_workspace {
                    self.move_to_scratchpad(Some(window));
                    return None;
                }
                self.hide_scratchpad_keeping_order(window);
                target_index = self
                    .scratchpad
                    .iter()
                    .position(|removed| removed.tile.window().id() == window);
            }
        } else if let Some(shown) = shown {
            if self.focus().is_some_and(|focused| focused.id() == &shown) {
                self.move_to_scratchpad(Some(&shown));
                return None;
            }
            self.hide_scratchpad_keeping_order(&shown);
            if let Some(index) = self
                .scratchpad_trees
                .iter()
                .position(|removed| removed.contains_window(&shown))
            {
                let active_workspace = self.active_workspace()?.id();
                return self.show_scratchpad_tree(index, active_workspace);
            }
            target_index = self
                .scratchpad
                .iter()
                .position(|removed| removed.tile.window().id() == &shown);
        }

        let index = target_index.unwrap_or_else(|| {
            self.scratchpad_windows
                .first()
                .and_then(|first| {
                    self.scratchpad
                        .iter()
                        .position(|removed| removed.tile.window().id() == first)
                })
                .unwrap_or(0)
        });
        self.scratchpad.get(index)?;
        let active_workspace = self.prepare_active_workspace_for_scratchpad_show()?;
        self.show_scratchpad_tile(index, active_workspace)
    }

    /// Hides a visible scratchpad window on the way to showing it elsewhere.
    /// Sway's `root_scratchpad_show` moves it directly and leaves the
    /// scratchpad order alone (sway/tree/root.c:157-204), unlike a hide.
    fn hide_scratchpad_keeping_order(&mut self, window: &W::Id) {
        let order = self.scratchpad_windows.clone();
        self.move_to_scratchpad(Some(window));
        let added = self
            .scratchpad_windows
            .iter()
            .filter(|id| !order.contains(id))
            .cloned()
            .collect::<Vec<_>>();
        self.scratchpad_windows = order;
        self.scratchpad_windows.extend(added);
    }

    /// Clears fullscreen on the active workspace and any global fullscreen
    /// before a scratchpad window appears, as sway's `root_scratchpad_show`
    /// does (sway/sway/tree/root.c:157-173). Returns the active workspace.
    fn prepare_active_workspace_for_scratchpad_show(&mut self) -> Option<WorkspaceId> {
        let active_workspace = self.active_workspace()?.id();
        for workspace in self.workspaces_mut() {
            let disables_fullscreen = workspace.id() == active_workspace
                || workspace.fullscreen_mode() == Some(tiling_tree::FullscreenMode::Global);
            if disables_fullscreen {
                workspace.disable_fullscreen();
            }
        }
        Some(active_workspace)
    }

    /// Shows the hidden scratchpad window at `index` floating on
    /// `active_workspace`, focused.
    fn show_scratchpad_tile(
        &mut self,
        index: usize,
        active_workspace: WorkspaceId,
    ) -> Option<W::Id> {
        // Find the destination before taking the window out of the scratchpad,
        // so a missing workspace leaves it hidden rather than dropping it.
        if !self
            .workspaces()
            .any(|(_, _, workspace)| workspace.id() == active_workspace)
        {
            return None;
        }
        let mut removed = self.scratchpad.remove(index)?;
        removed.is_floating = true;
        let shown = removed.tile.window().id().clone();
        let workspace = self.workspace_mut(active_workspace)?;
        workspace.remap_floating_position(&mut removed.tile, removed.floating_working_area);
        workspace.add_tile(
            removed.tile,
            WorkspaceAddWindowTarget::Auto,
            workspace::AddTileOptions {
                activate: ActivateWindow::Yes,
                is_floating: true,
            },
        );
        Some(shown)
    }

    /// Gives a view that a map-time rule hid in the scratchpad its committed
    /// size. Sway runs the criteria inside `view_map`, then the map commit's
    /// `handle_commit` finds the geometry new and resizes the floating view to
    /// it, keeping the content origin (sway/desktop/xdg_shell.c:319-326,
    /// `view_update_size`, sway/tree/view.c:1026-1031). The tile's stored
    /// position is its decorated origin, which the unchanged decorations keep.
    pub fn keep_committed_size_of_mapped_scratchpad_window(&mut self, window: &W::Id) {
        let Some(removed) = self
            .scratchpad
            .iter_mut()
            .find(|removed| removed.tile.window().id() == window)
        else {
            return;
        };
        let size = removed.tile.window().size();
        if size.w > 0 && size.h > 0 {
            removed.tile.floating_window_size = Some(size);
        }
    }

    pub fn scratchpad_tiles(&self) -> impl Iterator<Item = (&W, bool)> {
        self.scratchpad
            .iter()
            .map(|removed| (removed.tile.window(), removed.tile.is_sticky))
    }

    /// A hidden scratchpad window's border style and stored thickness. Sway
    /// reports the thickness as `current_border_width` even under `border
    /// none` (`c->current.border_thickness`, sway/ipc-json.c:760-761).
    pub fn scratchpad_border_thickness(
        &self,
        window: &W::Id,
    ) -> Option<(swayward_ipc::command::BorderStyle, u16)> {
        self.scratchpad
            .iter()
            .find(|removed| removed.tile.window().id() == window)
            .map(|removed| removed.tile.sway_border_thickness())
    }

    pub fn scratchpad_windows(&self) -> impl Iterator<Item = &W> {
        self.scratchpad
            .iter()
            .map(|removed| removed.tile.window())
            .chain(
                self.scratchpad_trees
                    .iter()
                    .flat_map(|removed| removed.windows()),
            )
    }

    pub fn scratchpad_trees(
        &self,
    ) -> impl Iterator<Item = (tiling_tree::IpcNode<W::Id>, bool)> + '_ {
        self.scratchpad_trees
            .iter()
            .map(|removed| (removed.ipc_tree(), removed.is_sticky()))
    }

    /// Every node of each hidden scratchpad group, with one of its windows.
    ///
    /// Sway's criteria and GET_MARKS walk hidden scratchpad containers too
    /// (`sway/sway/tree/root.c:250-257`), so a mark on a hidden group still
    /// finds it. The window is the group's representative for commands such
    /// as `scratchpad show`, which act on the whole group.
    pub fn scratchpad_tree_nodes(&self) -> impl Iterator<Item = (NodeId, &W::Id)> + '_ {
        self.scratchpad_trees.iter().flat_map(|removed| {
            let window = removed.window_ids().first();
            removed
                .ipc_tree()
                .nodes()
                .into_iter()
                .filter_map(move |(node, _)| Some((node, window?)))
        })
    }

    /// Appends `window` to the scratchpad order, or moves it to the end if it
    /// is already there. Sway appends a new scratchpad container and moves a
    /// hidden one to the end (`root_scratchpad_add_container`,
    /// `root_scratchpad_hide`, sway/tree/root.c:123,227).
    fn push_scratchpad_window(&mut self, window: W::Id) {
        self.scratchpad_windows.retain(|id| id != &window);
        self.scratchpad_windows.push(window);
    }

    /// Every scratchpad window, shown or hidden, in sway's `root->scratchpad`
    /// order.
    pub fn scratchpad_order(&self) -> &[W::Id] {
        &self.scratchpad_windows
    }

    pub fn scratchpad_is_empty(&self) -> bool {
        self.scratchpad_windows.is_empty()
    }

    pub fn is_scratchpad_window(&self, window: &W::Id) -> bool {
        self.scratchpad_windows.contains(window)
    }

    pub fn window_is_on_visible_workspace(&self, window: &W::Id) -> bool {
        self.workspaces().any(|(monitor, index, workspace)| {
            workspace.has_window(window)
                && monitor.is_some_and(|monitor| monitor.active_workspace_idx() == index)
        })
    }

    pub fn is_scratchpad_hidden(&self, window: &W::Id) -> bool {
        self.scratchpad
            .iter()
            .any(|removed| removed.tile.window().id() == window)
            || self
                .scratchpad_trees
                .iter()
                .any(|removed| removed.contains_window(window))
    }

    /// Where view `window` stands relative to the hidden scratchpad, read
    /// from the live tree.
    pub fn window_scratchpad_place(&self, window: &W::Id) -> ScratchpadPlace {
        if self
            .scratchpad
            .iter()
            .any(|removed| removed.tile.window().id() == window)
        {
            return ScratchpadPlace::Hidden;
        }
        for removed in &self.scratchpad_trees {
            if !removed.contains_window(window) {
                continue;
            }
            let tree = removed.ipc_tree();
            let root_is_window = matches!(
                tree.nodes().first(),
                Some((node, tiling_tree::IpcNodeKind::Leaf))
                    if tree.window_for_node(*node) == Some(window)
            );
            return if root_is_window {
                ScratchpadPlace::Hidden
            } else {
                ScratchpadPlace::InHiddenGroup
            };
        }
        ScratchpadPlace::NotHidden
    }

    /// Where container `node` stands relative to the hidden scratchpad, read
    /// from the live tree.
    pub fn container_scratchpad_place(&self, node: NodeId) -> ScratchpadPlace {
        for removed in &self.scratchpad_trees {
            let nodes = removed.ipc_tree().nodes();
            match nodes.iter().position(|(id, _)| *id == node) {
                Some(0) => return ScratchpadPlace::Hidden,
                Some(_) => return ScratchpadPlace::InHiddenGroup,
                None => {}
            }
        }
        ScratchpadPlace::NotHidden
    }

    /// Returning a container to tiling removes it from the scratchpad
    /// (sway/tree/container.c:990-994).
    pub(super) fn forget_scratchpad_window_if_tiled(&mut self, window: &W::Id) {
        if !self.scratchpad_windows.contains(window) {
            return;
        }
        let tiled = self
            .workspaces()
            .any(|(_, _, ws)| ws.has_window(window) && !ws.is_floating(window));
        if tiled {
            self.scratchpad_windows.retain(|id| id != window);
        }
    }
}
