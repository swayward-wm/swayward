//! Sway container commands on the workspace's tiling tree and floating trees.

use super::*;

/// The view a floating/tiled swap floated, when the tiled side was a view,
/// and the node IDs the floated container was renumbered with.
pub type FloatingTiledSwap<Id> = (Option<Id>, Vec<(NodeId, NodeId)>);

impl<W: LayoutElement> Workspace<W> {
    pub fn add_tile_at_drop(
        &mut self,
        tile: Tile<W>,
        target: NodeId,
        edge: ResizeEdge,
        activate: bool,
    ) {
        self.enter_output_for_window(tile.window());
        self.tiling.add_tile_at_drop(tile, target, edge, activate);
        if activate {
            self.floating_is_active = FloatingActive::No;
        }
    }

    pub fn add_tiling_tile(&mut self, tile: Tile<W>, activate: bool) {
        self.enter_output_for_window(tile.window());
        self.tiling
            .add_tile_with_activation(tile, InsertTarget::Focused, activate);
        if activate {
            self.floating_is_active = FloatingActive::No;
        }
    }

    pub fn detach_tiling_subtree(
        &mut self,
        id: NodeId,
    ) -> Option<(DetachedSubtree<W>, Option<NodeId>)> {
        let detached = self.tiling.detach_subtree(id)?;
        if let Some(output) = &self.output {
            detached
                .0
                .for_each_window(|window| window.output_leave(output));
        }
        self.update_focus_floating_tiling_after_removing(false);
        Some(detached)
    }

    pub fn attach_tiling_subtree(
        &mut self,
        subtree: DetachedSubtree<W>,
    ) -> (NodeId, Vec<(NodeId, NodeId)>) {
        self.attach_tiling_subtree_at(subtree, None)
    }

    pub fn swap_tiling_nodes(&mut self, first: NodeId, second: NodeId) -> Result<(), &'static str> {
        if self.tiling.contains(first) && self.tiling.contains(second) {
            return self.tiling.swap_nodes(first, second);
        }
        self.floating.swap_nodes(first, second)
    }

    /// Swaps a standalone floating view with a tiled container or view on
    /// this workspace. Each takes the other's place and geometry, and focus
    /// stays with the container that had it (`swap_places`, `swap_focus`,
    /// sway/tree/container.c:1718-1798). Returns the view that became
    /// floating, when the tiled side was a view, and the remapped node IDs.
    pub fn swap_floating_window_with_tiling_node(
        &mut self,
        window: &W::Id,
        node: NodeId,
    ) -> Option<FloatingTiledSwap<W::Id>> {
        if !self.floating.window_is_floating_root(window)
            || !self.tiling.contains(node)
            || self.tiling.is_root(node)
        {
            return None;
        }
        let floater_rect = self
            .floating
            .tiles_with_offsets()
            .find(|(tile, _)| tile.window().id() == window)
            .map(|(tile, pos)| Rectangle::new(pos, tile.tile_size()))?;
        let floater_focused = self.floating_is_active.get()
            && self
                .floating
                .active_window()
                .is_some_and(|active| active.id() == window);
        let tiled_focused = !self.floating_is_active.get()
            && self
                .tiling
                .focus()
                .is_some_and(|focus| self.tiling.contains_node(node, focus));
        let (mut subtree, slot) = self.detach_tiling_subtree_for_swap(node)?;
        let RemovedTile { mut tile, .. } = self.floating.remove_tile(window, Transaction::new());
        let (floating_pos, floating_size) = (tile.floating_pos, tile.floating_window_size);
        tile.tiling_parent = None;
        tile.tiling_focus_rank = None;
        tile.stop_move_animations();
        let mut leaf = DetachedSubtree::from_tile(tile);
        leaf.swap_fullscreen_position(&mut subtree);
        self.tiling.attach_subtree_for_swap(leaf, slot);
        self.tiling.finish_subtree_detach(None);
        // swap_places leaves the seat focus stack alone, so the view keeps
        // its own focus rank rather than the departed node's.
        self.tiling.rank_arrived_window_by_focus_timestamp(window);
        if let Some(output) = &self.output {
            subtree.for_each_window(|window| window.output_enter(output));
        }
        let floated = match subtree.into_tile() {
            Ok(mut tile) => {
                tile.floating_pos = floating_pos;
                tile.floating_window_size = floating_size;
                tile.tiling_parent = None;
                tile.tiling_focus_rank = None;
                tile.stop_move_animations();
                let id = tile.window().id().clone();
                self.floating.add_tile(tile, tiled_focused);
                (Some(id), Vec::new())
            }
            Err(subtree) => (None, self.floating.add_tree(*subtree, floater_rect).1),
        };
        if floater_focused {
            self.tiling.activate_window(window);
            self.floating_is_active = FloatingActive::No;
        } else if tiled_focused {
            // The floated container keeps focus, so the next view maps
            // beside the most recently focused tiled view, not the view that
            // arrived (`seat_get_focus_inactive_tiling`,
            // `seat_get_focus_inactive_view`, sway/tree/view.c:851-866).
            self.tiling.focus_inactive_view_keeping_history();
            self.floating_is_active = FloatingActive::Yes;
        }
        Some(floated)
    }

    pub fn detach_tiling_subtree_for_swap(
        &mut self,
        id: NodeId,
    ) -> Option<(DetachedSubtree<W>, crate::layout::tiling_tree::DetachedSlot)> {
        let detached = self.tiling.detach_subtree_for_swap(id)?;
        if let Some(output) = &self.output {
            detached
                .0
                .for_each_window(|window| window.output_leave(output));
        }
        Some(detached)
    }

    pub fn attach_tiling_subtree_for_swap(
        &mut self,
        subtree: DetachedSubtree<W>,
        slot: crate::layout::tiling_tree::DetachedSlot,
    ) -> (NodeId, Vec<(NodeId, NodeId)>) {
        if let Some(output) = &self.output {
            subtree.for_each_window(|window| window.output_enter(output));
        }
        self.tiling.attach_subtree_for_swap(subtree, slot)
    }

    pub fn attach_tiling_subtree_at(
        &mut self,
        subtree: DetachedSubtree<W>,
        target: Option<NodeId>,
    ) -> (NodeId, Vec<(NodeId, NodeId)>) {
        if let Some(output) = &self.output {
            subtree.for_each_window(|window| window.output_enter(output));
        }
        let fullscreen = subtree.has_fullscreen();
        if fullscreen {
            self.disable_fullscreen();
        }
        self.floating_is_active = FloatingActive::No;
        self.tiling.attach_subtree_at(subtree, target)
    }

    pub fn clear_floating_tree_fullscreen(&mut self, root: NodeId) {
        let Some(tree) = self.floating.tree_mut(root) else {
            return;
        };
        if let Some(fullscreen) = tree.fullscreen_node() {
            tree.set_node_fullscreen(fullscreen, None);
        }
    }

    pub fn remove_floating_tree(&mut self, root: NodeId) -> Option<RemovedFloatingTree<W>> {
        let removed = self.floating.remove_tree_for_transfer(root)?;
        if let Some(output) = &self.output {
            for (_, window) in removed.tree.windows() {
                window.output_leave(output);
            }
        }
        // `container_detach` refreshes the workspace representation
        // (sway/tree/container.c:1461-1466).
        self.tiling.restore_has_had_tile(true);
        self.update_focus_floating_tiling_after_removing(true);
        Some(removed)
    }

    pub fn add_floating_tree(
        &mut self,
        removed: RemovedFloatingTree<W>,
        remap_position: bool,
    ) -> NodeId {
        if let Some(output) = &self.output {
            for (_, window) in removed.tree.windows() {
                window.output_enter(output);
            }
        }
        let (root, remapped) = self.floating.add_removed_tree(removed, remap_position);
        debug_assert!(remapped.is_empty());
        self.floating_is_active = FloatingActive::Yes;
        root
    }

    pub fn focus_floating_tree_view(&mut self, root: NodeId) {
        self.floating.focus_tree_view(root);
    }

    /// Focuses the focus-inactive view below `node` in floating group `root`. Returns false
    /// when `node` is gone or holds no view.
    pub fn focus_floating_view_in(&mut self, root: NodeId, node: NodeId) -> bool {
        self.floating.focus_tree_view_in(root, node)
    }

    /// Hands focus to the floating layer's active window, if any.
    pub fn activate_floating_layer(&mut self) {
        if self.floating.active_window().is_some() {
            self.floating_is_active = FloatingActive::Yes;
        }
    }

    pub fn remove_active_tiling_tile(&mut self) -> Option<Tile<W>> {
        if self.floating_is_active.get() {
            return None;
        }
        let id = self.tiling.active_window()?.id().clone();
        let tile = self.tiling.remove_tile(&id, Transaction::new())?;
        if let Some(output) = &self.output {
            tile.window().output_leave(output);
        }
        self.update_focus_floating_tiling_after_removing(false);
        Some(tile)
    }

    /// Moves focus from the workspace node to its focus-inactive tiling container, as
    /// `workspace_switch` does with `seat_get_focus_inactive(ws)`, which only returns the
    /// workspace itself when nothing under it was focused (sway/tree/workspace.c:731-743;
    /// sway/input/seat.c:1357-1372).
    pub fn focus_inactive_below_workspace(&mut self) {
        if !self.is_workspace_focused() {
            return;
        }
        let newest = |tiles: &mut dyn Iterator<Item = &Tile<W>>| {
            tiles
                .filter_map(|tile| tile.window().focus_timestamp())
                .max()
        };
        let floating = newest(&mut self.floating.tiles());
        if floating.is_some() && floating > newest(&mut self.tiling.tiles()) {
            self.focus_floating();
        } else if self.tiling.focus_inactive_below_root() {
            self.floating_is_active = FloatingActive::No;
        }
    }

    pub fn focus_parent(&mut self) -> bool {
        if self.floating_is_active.get() {
            if self.floating.focus_parent() {
                return true;
            }
            // A floating root's parent is the workspace (`focus_parent`,
            // sway/commands/focus.c:339-351), even when nothing is tiled. An empty tiling tree
            // keeps no focus of its own, so the raised-but-inactive floating state alone stands
            // for the focused workspace there.
            self.floating_is_active = FloatingActive::NoButRaised;
            if !self.tiling.is_empty() {
                self.tiling.focus_root();
            }
            true
        } else {
            let changed = self.tiling.focus_parent();
            if self.tiling.root_is_focused() {
                self.floating_is_active = FloatingActive::No;
            }
            changed
        }
    }

    pub fn scroll_tab_indicator(&mut self, window: &W::Id, steps: i32) -> Option<W::Id> {
        (!self.floating_is_active.get())
            .then(|| self.tiling.scroll_tab_indicator(window, steps))
            .flatten()
    }

    pub fn focus_next_prev_sibling(&mut self, next: bool, allow_wrap: bool) -> bool {
        !self.floating_is_active.get() && self.tiling.focus_next_prev_sibling(next, allow_wrap)
    }

    /// The tiling direction of `focus next|prev`; `None` on the floating layer, which never
    /// leaves the workspace (sway/commands/focus.c:457-460).
    pub fn tiling_next_prev_direction(
        &self,
        next: bool,
    ) -> Option<crate::layout::tiling_tree::Direction> {
        (!self.floating_is_active.get())
            .then(|| self.tiling.next_prev_direction(next))
            .flatten()
    }

    pub fn focus_child(&mut self) -> bool {
        if self.floating_is_active.get() {
            self.floating.focus_child()
        } else if self.is_workspace_focused()
            && self.floating_is_active == FloatingActive::NoButRaised
        {
            self.floating_is_active = FloatingActive::Yes;
            true
        } else {
            self.tiling.focus_child()
        }
    }

    pub fn focus_tiling_node(&mut self, id: crate::layout::tiling_tree::NodeId) -> bool {
        if self
            .tiling_node_windows(id)
            .is_some_and(|windows| !windows.is_empty())
        {
            self.floating_is_active = FloatingActive::No;
            self.tiling.set_focus(id);
            true
        } else {
            false
        }
    }

    pub fn focused_tiling_node(&self) -> Option<crate::layout::tiling_tree::NodeId> {
        (!self.floating_is_active.get())
            .then(|| self.tiling.focus())
            .flatten()
    }

    pub fn focused_container_node(&self) -> Option<crate::layout::tiling_tree::NodeId> {
        if self.floating_is_active.get() {
            self.floating.focused_container_node()
        } else if self.floating_is_active == FloatingActive::NoButRaised {
            self.tiling.focus().filter(|id| self.tiling.is_root(*id))
        } else {
            self.focused_tiling_node()
        }
    }

    /// Ranks a floating view that arrived without focus by when it last had focus, as sway's
    /// single seat focus stack does: focused more recently than every view already here, it
    /// becomes the workspace's focus-inactive node (sway/input/seat.c:1357-1372).
    pub fn rank_arrived_floating_window(&mut self, window: &W::Id) {
        let stamp_of = |tile: &Tile<W>| tile.window().focus_timestamp();
        let Some(stamp) = self
            .floating
            .tiles()
            .find(|tile| tile.window().id() == window)
            .and_then(stamp_of)
        else {
            return;
        };
        let newer = self
            .floating
            .tiles()
            .chain(self.tiling.tiles())
            .filter(|tile| tile.window().id() != window)
            .any(|tile| stamp_of(tile) >= Some(stamp));
        if !newer && self.floating.activate_window_without_raising(window) {
            self.floating_is_active = FloatingActive::Yes;
        }
    }

    /// `focus <direction>` from a focused floating root (sway/commands/focus.c:457-460).
    pub fn focus_floating_direction(&mut self, direction: Direction) -> bool {
        self.floating_is_active.get() && self.floating.focus_direction_without_wrap(direction)
    }

    pub fn focused_floating_tree_child(&self) -> bool {
        self.floating_is_active.get() && self.floating.focused_tree_child()
    }

    pub fn focused_floating_tree_root_is_fullscreen(&self) -> bool {
        self.floating_is_active.get() && self.floating.focused_tree_root_is_fullscreen()
    }

    pub fn is_workspace_focused(&self) -> bool {
        !self.floating_is_active.get()
            && (self.tiling.root_is_focused()
                || self.tiling.is_empty() && self.floating_is_active == FloatingActive::NoButRaised)
    }

    pub fn toggle_tiling_target_layout(
        &mut self,
        id: crate::layout::tiling_tree::NodeId,
        toggle: &swayward_ipc::command::LayoutToggle,
        container: bool,
    ) -> Option<Vec<(NodeId, NodeId)>> {
        if container {
            self.tiling.toggle_node_layout(id, toggle).then(Vec::new)
        } else {
            self.tiling.toggle_target_layout(id, toggle)
        }
    }

    pub fn restore_tiling_target_layout(
        &mut self,
        id: crate::layout::tiling_tree::NodeId,
        container: bool,
    ) -> Option<(bool, Vec<(NodeId, NodeId)>)> {
        if container {
            self.tiling
                .restore_node_layout(id)
                .then(|| (true, Vec::new()))
        } else {
            self.tiling.restore_target_layout(id)
        }
    }

    pub fn tiling_node_windows(
        &self,
        id: crate::layout::tiling_tree::NodeId,
    ) -> Option<Vec<W::Id>> {
        self.tiling.contains(id).then(|| {
            self.tiling
                .windows()
                .filter(|(leaf, _)| self.tiling.contains_node(id, *leaf))
                .map(|(_, window)| window.id().clone())
                .collect()
        })
    }

    pub fn focus_from_output_direction(
        &mut self,
        direction: crate::layout::tiling_tree::Direction,
    ) -> bool {
        if self.tiling.is_empty() {
            // With nothing tiled, sway focuses the workspace itself, never a floating view
            // (`get_node_in_output_direction`, sway/commands/focus.c:93-135).
            if !self.floating.is_empty() {
                self.floating_is_active = FloatingActive::NoButRaised;
                return true;
            }
            return false;
        }
        self.floating_is_active = FloatingActive::No;
        self.tiling.focus_from_output_direction(direction)
    }

    pub fn focus_next_or_prev(&mut self, next: bool) -> bool {
        if self.floating_is_active.get() {
            let layout = self.tiling.representation_layout();
            self.floating.focus_next_or_prev(next, layout)
        } else {
            self.tiling.focus_next_or_prev(next)
        }
    }

    pub fn focus_left_without_wrap(&mut self) -> bool {
        if self.floating_is_active.get() {
            self.floating.focus_left()
        } else {
            self.tiling.focus_direction_without_wrap(Direction::Left)
        }
    }

    pub fn focus_right_without_wrap(&mut self) -> bool {
        if self.floating_is_active.get() {
            self.floating.focus_right()
        } else {
            self.tiling.focus_direction_without_wrap(Direction::Right)
        }
    }

    pub fn focus_first_root_child(&mut self) {
        if self.floating_is_active.get() {
            self.floating.focus_leftmost();
        } else {
            self.tiling.focus_first_root_child();
        }
    }

    pub fn focus_last_root_child(&mut self) {
        if self.floating_is_active.get() {
            self.floating.focus_rightmost();
        } else {
            self.tiling.focus_last_root_child();
        }
    }

    pub fn focus_right_or_first_root_child(&mut self) {
        if !self.focus_right() {
            self.focus_first_root_child();
        }
    }

    pub fn focus_left_or_last_root_child(&mut self) {
        if !self.focus_left() {
            self.focus_last_root_child();
        }
    }

    pub fn focus_root_child(&mut self, index: usize) {
        if self.floating_is_active.get() {
            self.focus_tiling();
        }
        self.tiling.focus_root_child(index);
    }

    pub fn focus_window_in_parent(&mut self, index: u8) {
        if self.floating_is_active.get() {
            return;
        }
        self.tiling.focus_window_in_parent(index);
    }

    pub fn focus_down_without_wrap(&mut self) -> bool {
        if self.floating_is_active.get() {
            self.floating.focus_down()
        } else {
            self.tiling.focus_direction_without_wrap(Direction::Down)
        }
    }

    pub fn focus_up_without_wrap(&mut self) -> bool {
        if self.floating_is_active.get() {
            self.floating.focus_up()
        } else {
            self.tiling.focus_direction_without_wrap(Direction::Up)
        }
    }

    pub fn move_focused_floating_tree_child(&mut self, direction: Direction) -> bool {
        self.floating
            .move_focused_tree_child(direction)
            .unwrap_or(false)
    }

    pub fn move_window_in_direction(
        &mut self,
        window: &W::Id,
        direction: Direction,
        pixels: f64,
    ) -> bool {
        if self.floating.has_window(window) {
            if self.floating.move_tree_window(window, direction).is_some() {
                return true;
            }
            let (x, y) = match direction {
                Direction::Left => (-pixels, 0.),
                Direction::Right => (pixels, 0.),
                Direction::Up => (0., -pixels),
                Direction::Down => (0., pixels),
            };
            self.floating.move_window(
                Some(window),
                PositionChange::AdjustFixed(x),
                PositionChange::AdjustFixed(y),
                true,
            );
            true
        } else {
            self.tiling.move_window_direction(window, direction)
        }
    }

    pub fn move_focused_root_child_to_first(&mut self) {
        if self.floating_is_active.get() {
            return;
        }
        self.tiling.move_focused_to_first();
    }

    pub fn move_focused_root_child_to_last(&mut self) {
        if self.floating_is_active.get() {
            return;
        }
        self.tiling.move_focused_to_last();
    }

    pub fn move_focused_root_child_to_index(&mut self, index: usize) {
        if self.floating_is_active.get() {
            return;
        }
        self.tiling.move_focused_to_index(index.saturating_sub(1));
    }

    pub fn nest_or_unnest_window_left(&mut self, window: Option<&W::Id>) {
        if window.map_or(self.floating_is_active.get(), |id| {
            self.floating.has_window(id)
        }) {
            return;
        }
        self.tiling.expel_or_consume(window, false);
    }

    pub fn nest_or_unnest_window_right(&mut self, window: Option<&W::Id>) {
        if window.map_or(self.floating_is_active.get(), |id| {
            self.floating.has_window(id)
        }) {
            return;
        }
        self.tiling.expel_or_consume(window, true);
    }

    pub fn nest_focused_window(&mut self) {
        if self.floating_is_active.get() {
            return;
        }
        self.tiling.consume_focused();
    }

    pub fn unnest_focused_window(&mut self) {
        if self.floating_is_active.get() {
            return;
        }
        self.tiling.expel_focused();
    }

    pub fn swap_window_horizontal(&mut self, right: bool) {
        if self.floating_is_active.get() {
            return;
        }
        if right {
            self.tiling.move_right();
        } else {
            self.tiling.move_left();
        }
    }

    pub fn toggle_focused_tabbed_display(&mut self) {
        if self.floating_is_active.get() {
            return;
        }
        self.tiling.toggle_focused_tabbed();
    }

    pub fn set_focused_layout(
        &mut self,
        layout: crate::layout::tiling_tree::Layout,
    ) -> Vec<(NodeId, NodeId)> {
        if self.floating_is_active.get() {
            self.floating
                .focused_child_tree_mut()
                .map(|tree| tree.set_focused_layout(layout))
                .unwrap_or_default()
        } else {
            self.tiling.set_focused_layout(layout)
        }
    }

    pub fn split_focused(&mut self, layout: crate::layout::tiling_tree::Layout) {
        if self.split_fullscreen_floating(layout) {
            return;
        }
        if self.floating_is_active.get() {
            self.floating.split_active(layout);
        } else {
            self.tiling.split_focused(layout);
        }
    }

    pub fn flatten_focused_parent(&mut self) -> Option<Vec<(NodeId, NodeId)>> {
        (!self.floating_is_active.get())
            .then(|| self.tiling.focus())
            .flatten()
            .and_then(|focus| self.tiling.flatten_ancestors(focus))
    }

    pub fn toggle_focused_layout(
        &mut self,
        toggle: &swayward_ipc::command::LayoutToggle,
    ) -> Vec<(NodeId, NodeId)> {
        if self.floating_is_active.get() {
            self.floating
                .focused_child_tree_mut()
                .map(|tree| tree.toggle_focused_layout(toggle))
                .unwrap_or_default()
        } else {
            self.tiling.toggle_focused_layout(toggle)
        }
    }

    pub fn restore_focused_split_layout(&mut self) -> Option<Vec<(NodeId, NodeId)>> {
        if self.floating_is_active.get() {
            self.floating
                .focused_child_tree_mut()?
                .restore_focused_split_layout()
        } else {
            self.tiling.restore_focused_split_layout()
        }
    }

    pub fn toggle_focused_layout_split(&mut self) -> Vec<(NodeId, NodeId)> {
        if self.floating_is_active.get() {
            self.floating
                .focused_child_tree_mut()
                .map(crate::layout::tiling_tree::TilingTree::toggle_focused_layout_split)
                .unwrap_or_default()
        } else {
            self.tiling.toggle_focused_layout_split()
        }
    }

    /// A split on a fullscreen floating view, which swayward parks in the tiling tree. Sway
    /// wraps it in a new floating container and moves the fullscreen mode onto that container
    /// (`container_split` and `container_replace`, sway/tree/container.c:1471-1501), so the
    /// view leaves fullscreen back into the floating layer, is wrapped there, and the new
    /// floating root takes the mode. Returns false when the focus is not such a view.
    fn split_fullscreen_floating(&mut self, layout: crate::layout::tiling_tree::Layout) -> bool {
        if self.floating_is_active.get() || !self.active_floating_is_fullscreen() {
            return false;
        }
        let Some(window) = self.tiling.active_window().map(|w| w.id().clone()) else {
            return false;
        };
        let mode = self
            .tiling
            .node_for_window(&window)
            .and_then(|node| self.tiling.fullscreen_mode(node));
        let hidden = self.tiling.hidden_under_fullscreen();
        self.set_fullscreen_mode(&window, None);
        if !self.floating.has_window(&window) {
            return true;
        }
        self.tiling.keep_unarranged_for_floating_fullscreen(hidden);
        self.floating_is_active = FloatingActive::Yes;
        self.floating.split_active(layout);
        if let Some(root) = self.floating.tree_root_for_window(&window) {
            if let Some(tree) = self.floating.tree_mut(root) {
                tree.set_node_fullscreen(root, mode);
            }
        }
        true
    }

    /// `split toggle` reads `container_parent_layout`, which for a floating root (no parent) is
    /// the workspace layout (sway/tree/container.c:1353-1361, sway/commands/split.c:64-71).
    pub fn toggle_focused_split(&mut self) {
        use crate::layout::tiling_tree::Layout;
        if self.active_floating_is_fullscreen() && !self.floating_is_active.get() {
            let layout = if self.tiling.root_layout() == Some(Layout::SplitV) {
                Layout::SplitH
            } else {
                Layout::SplitV
            };
            self.split_fullscreen_floating(layout);
            return;
        }
        if !self.floating_is_active.get() {
            self.tiling.toggle_focused_split();
            return;
        }
        if let Some(tree) = self.floating.focused_child_tree_mut() {
            tree.toggle_focused_split();
            return;
        }
        let layout = if self.tiling.root_layout() == Some(Layout::SplitV) {
            Layout::SplitH
        } else {
            Layout::SplitV
        };
        self.floating.split_active(layout);
    }

    pub fn set_focused_display(&mut self, display: ColumnDisplay) {
        if self.floating_is_active.get() {
            return;
        }
        // niri's column display maps onto the parent split: tabbed, or a vertical stack.
        self.tiling
            .set_focused_parent_layout(if display == ColumnDisplay::Tabbed {
                crate::layout::tiling_tree::Layout::Tabbed
            } else {
                crate::layout::tiling_tree::Layout::SplitV
            });
    }

    pub fn set_focused_width(&mut self, change: SizeChange) {
        if self.floating_is_active.get() {
            self.floating
                .set_window_width(None, change, true, self.view_size.to_i32_round());
        } else {
            self.tiling.set_window_width(None, change);
        }
    }

    pub fn set_window_size_sway(
        &mut self,
        window: &W::Id,
        width: Option<SizeChange>,
        height: Option<SizeChange>,
        automatic_maximum: Size<i32, Logical>,
    ) {
        if self.is_floating(window) {
            if let Some(change) = width {
                self.floating
                    .set_window_outer_width(window, change, automatic_maximum);
            }
            if let Some(change) = height {
                self.floating
                    .set_window_outer_height(window, change, automatic_maximum);
            }
        } else {
            self.tiling.set_window_size_sway(window, width, height);
        }
    }

    pub fn resize_window_edge(
        &mut self,
        window: Option<&W::Id>,
        edge: ResizeEdge,
        change: SizeChange,
    ) -> Option<bool> {
        if window.map_or(self.floating_is_active.get(), |id| {
            self.floating.has_window(id)
        }) {
            Some(self.floating.resize_window_edge(window, edge, change))
        } else {
            Some(self.tiling.resize_window_edge(window, edge, change))
        }
    }

    pub fn expand_focused_to_available_width(&mut self) {
        if self.floating_is_active.get() {
            return;
        }
        self.tiling.toggle_full_width();
    }

    /// Makes the workspace node itself the focused node, without raising it in the tiling
    /// history, so a later switch here still descends to the focus-inactive view.
    ///
    /// Carried sticky floaters land here too: the seat still focuses the workspace
    /// (`seat_set_workspace_focus`, sway/sway/input/seat.c:1209-1221), whether the floater
    /// sits in the floating space or, fullscreen, in the tiling tree.
    pub(in crate::layout) fn focus_workspace_itself(&mut self) {
        if self.tiling.is_empty() {
            self.floating_is_active = if self.floating.is_empty() {
                FloatingActive::No
            } else {
                FloatingActive::NoButRaised
            };
        } else {
            self.floating_is_active = FloatingActive::No;
            self.tiling.focus_root_keeping_history();
        }
    }

    /// Moves a child of a floating group into the tiling tree. Sway treats only the group root
    /// as floating (`container_is_floating`, sway/tree/container.c:1041-1049), so a command
    /// aimed at the child moves it as a tiled container and leaves its siblings in the group,
    /// or reaps the group when it was the only view (`container_reap_empty`,
    /// sway/commands/move.c:609-611). Returns false when the window is not such a child.
    pub fn detach_floating_group_child(&mut self, window: &W::Id) -> bool {
        // Any view below the floating root is a child, even the only view of a floating split
        // (`floating enable; splitv`): sway moves it as tiled and reaps the emptied split.
        if self.floating.tree_root_for_window(window).is_none()
            || self.floating.window_is_tree_root(window)
        {
            return false;
        }
        // Only the focused child takes the tiling focus with it; a criteria
        // match elsewhere leaves the seat focus alone.
        let focused = self.floating_is_active.get()
            && self.floating.active_window().map(|active| active.id()) == Some(window);
        let stamp = self
            .floating
            .tiles()
            .map(Tile::window)
            .find(|candidate| candidate.id() == window)
            .and_then(LayoutElement::focus_timestamp);
        if let Some(stamp) = stamp {
            self.floating.record_departing_focus(window, stamp);
        }
        let removed = self.floating.remove_tile(window, Transaction::new());
        let mut tile = removed.tile;
        tile.restore_to_floating = false;
        self.tiling
            .add_tile_with_activation(tile, InsertTarget::Focused, focused);
        if focused {
            self.floating_is_active = FloatingActive::No;
        }
        true
    }

    /// Floats tiling container `node` for a move onto a floating mark and
    /// returns its floating root. With `anchor`, the root is stacked directly
    /// above that floating root, where `container_add_sibling` puts it in
    /// `workspace->floating` (`container_move_to_container`,
    /// sway/commands/move.c:243-261; sway/tree/container.c:1410-1423).
    ///
    /// Focus follows `cmd_move_container` (sway/commands/move.c:598-608): a
    /// focused container hands focus to the old parent's focus-inactive
    /// child, else to the workspace's, which is the moved container itself.
    pub fn float_tiling_node_for_mark(
        &mut self,
        node: NodeId,
        anchor: Option<&StackSlot<W::Id>>,
    ) -> Option<StackSlot<W::Id>> {
        if !self.tiling.contains(node) {
            return None;
        }
        let floating_was_active = self.floating_is_active;
        let floating_focus = self
            .floating
            .active_window()
            .map(|window| window.id().clone());
        let tiling_focus = self.tiling.focus();
        let focus = (!floating_was_active.get())
            .then_some(tiling_focus)
            .flatten();
        let focus_in_moved = focus.is_some_and(|focus| self.tiling.contains_node(node, focus));
        let sibling_focus = (focus == Some(node))
            .then(|| self.tiling.parent_of_node(node))
            .flatten()
            .filter(|parent| !self.tiling.is_root(*parent))
            .and_then(|parent| self.tiling.focus_inactive_in_excluding(parent, node));

        let moved = match self.tiling.window_for_node(node).map(|w| w.id().clone()) {
            Some(window) => {
                self.toggle_window_floating(Some(&window));
                if !self.floating.window_is_floating_root(&window) {
                    return None;
                }
                StackSlot::Window(window)
            }
            None => StackSlot::Tree(self.set_container_floating(node, true)?),
        };
        if let Some(anchor) = anchor {
            self.floating.restack_above(&moved, anchor);
        }

        if let Some(sibling) = sibling_focus {
            self.tiling.set_focus(sibling);
            self.floating_is_active = FloatingActive::No;
        } else if focus_in_moved {
            if let StackSlot::Window(window) = &moved {
                self.floating.activate_window_without_raising(window);
            }
            self.floating_is_active = FloatingActive::Yes;
        } else {
            if let Some(focus) = tiling_focus.filter(|focus| self.tiling.contains(*focus)) {
                self.tiling.set_focus(focus);
            }
            if let Some(window) = floating_focus {
                self.floating.activate_window_without_raising(&window);
            }
            self.floating_is_active = if self.tiling.is_empty() {
                FloatingActive::Yes
            } else {
                floating_was_active
            };
        }
        Some(moved)
    }

    pub fn restack_floating_above(&mut self, moved: &StackSlot<W::Id>, anchor: &StackSlot<W::Id>) {
        self.floating.restack_above(moved, anchor);
    }

    pub fn floating_tree_root_for_window(&self, window: &W::Id) -> Option<NodeId> {
        self.floating.tree_root_for_window(window)
    }

    pub fn floating_transfer_window_ids(&self) -> Vec<W::Id> {
        self.floating.transfer_window_ids()
    }

    pub fn window_is_floating_root(&self, window: &W::Id) -> bool {
        self.floating.window_is_floating_root(window) || self.floating.window_is_tree_root(window)
    }

    pub fn focused_floating_tree_root(&self) -> Option<NodeId> {
        self.active_window()
            .and_then(|window| self.floating.tree_root_for_window(window.id()))
    }

    pub fn contains_swap_node(&self, id: crate::layout::tiling_tree::NodeId) -> bool {
        self.tiling.contains(id) || self.floating.tree_root_for_node(id).is_some()
    }

    pub fn swap_node_for_window(
        &self,
        window: &W::Id,
    ) -> Option<crate::layout::tiling_tree::NodeId> {
        self.tiling.node_for_window(window).or_else(|| {
            let root = self.floating.tree_root_for_window(window)?;
            self.floating.tree(root)?.node_for_window(window)
        })
    }

    pub fn is_tiling_split(&self, id: crate::layout::tiling_tree::NodeId) -> bool {
        self.tiling.is_split(id)
            || self
                .floating
                .tree_root_for_node(id)
                .and_then(|root| self.floating.tree(root))
                .is_some_and(|tree| tree.is_split(id))
    }

    /// `floating` with the workspace focused first wraps its tiling children in a container
    /// and focuses it (sway/commands/floating.c:28-33). Returns the wrapper.
    pub fn wrap_and_focus_workspace_children(&mut self) -> Option<NodeId> {
        if !self.is_workspace_focused() {
            return None;
        }
        let wrapper = self.tiling.wrap_workspace_children_for_floating()?;
        self.tiling.set_focus(wrapper);
        self.floating_is_active = FloatingActive::No;
        Some(wrapper)
    }

    pub fn set_container_floating(&mut self, node: NodeId, floating: bool) -> Option<NodeId> {
        if let Some(root) = self.floating.tree_root_for_node(node) {
            if floating {
                return Some(root);
            }
            // Returning a focused container to tiling leaves the seat focus on it
            // (`container_set_floating`, sway/tree/container.c:976-1011).
            let root_focused = self.floating_is_active.get()
                && self.floating.focused_container_node() == Some(root);
            let subtree = self.floating.remove_tree(root)?;
            if let Some(output) = &self.output {
                subtree.for_each_window(|window| window.output_enter(output));
            }
            let hidden = if subtree.has_fullscreen() {
                self.disable_fullscreen();
                self.tiling.take_floating_fullscreen()
            } else {
                None
            };
            self.floating_is_active = FloatingActive::No;
            let (root, _) = self.tiling.attach_unfloated_subtree(subtree);
            if let Some(hidden) = hidden {
                self.tiling.restore_hidden_under_fullscreen(hidden);
            }
            if root_focused {
                self.tiling.set_focus(root);
            }
            if self.floating.is_empty() {
                self.floating_is_active = FloatingActive::No;
            }
            return Some(root);
        }
        if !floating {
            return self.tiling.contains(node).then_some(node);
        }
        let whole_workspace = self.tiling.is_root(node);
        let (subtree, old_parent) = self.detach_tiling_subtree(node)?;
        self.tiling.finish_subtree_detach(old_parent);
        if whole_workspace {
            // Floating a focused workspace wraps its children and resets the
            // workspace to splith (sway/commands/floating.c:29-31).
            self.tiling
                .set_empty_layout(crate::layout::tiling_tree::Layout::SplitH);
        }
        let size = Size::from((
            self.working_area.size.w * 0.5,
            self.working_area.size.h * 0.75,
        ));
        let rect = Rectangle::new(
            self.working_area.loc
                + (self.working_area.size.to_point() - size.to_point()).downscale(2.),
            size,
        );
        let (root, remapped) = self.floating.add_tree(subtree, rect);
        debug_assert!(remapped.is_empty());
        // Floating a container detaches it, which clears
        // `root->fullscreen_global` for good (sway/tree/container.c:941-975
        // and 1440-1446).
        self.floating.orphan_tree_global_fullscreen(root);
        self.floating_is_active = FloatingActive::Yes;
        Some(root)
    }
}
