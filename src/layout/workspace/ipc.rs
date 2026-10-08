//! Workspace state as sway's IPC reports it.

use super::*;

impl<W: LayoutElement> Workspace<W> {
    pub fn window_border(
        &self,
        window: &W::Id,
    ) -> Option<(swayward_ipc::command::BorderStyle, u16)> {
        self.tiles()
            .find(|tile| tile.window().id() == window)
            .map(|tile| tile.sway_border())
    }

    /// See [`FloatingLayout::use_client_decorations_from_map`].
    pub fn use_floating_client_decorations_from_map(&mut self, window: &W::Id) {
        self.floating.use_client_decorations_from_map(window);
    }

    pub fn set_window_border(
        &mut self,
        window: &W::Id,
        style: swayward_ipc::command::BorderStyle,
        width: Option<u16>,
    ) -> Result<(), &'static str> {
        let changed = if self.floating.has_window(window) {
            self.floating.set_window_border(window, style, width)
                || self.floating.set_tree_window_border(window, style, width)
        } else {
            self.tiling.set_window_border(window, style, width)
        };
        changed
            .then_some(())
            .ok_or("This window doesn't support client side decorations")
    }

    pub fn is_floating_for_ipc(&self, id: &W::Id) -> bool {
        self.floating.has_window(id)
            || self.tiling.tiles().any(|tile| {
                tile.window().id() == id
                    && tile.restore_to_floating
                    && tile.window().pending_sizing_mode().is_fullscreen()
            })
    }

    pub fn ipc_tiling_tree(&self) -> crate::layout::tiling_tree::IpcNode<W::Id> {
        let mut tree = self.tiling.ipc_tree();
        tree.retain_leaves(&|window| !self.is_floating_for_ipc(window));
        tree
    }

    /// When focus last entered floating group `root` through a view that has since left it.
    pub fn floating_tree_entered_by_departed(&self, root: NodeId) -> Option<std::time::Duration> {
        self.floating.tree_entered_by_departed(root)
    }

    pub fn ipc_floating_trees(
        &self,
    ) -> impl Iterator<Item = (NodeId, crate::layout::tiling_tree::IpcNode<W::Id>, bool)> + '_ {
        self.floating.ipc_trees()
    }

    /// The offset GET_TREE applies to a floating root's descendants while
    /// sway would not have re-arranged them, because the workspace holds a
    /// fullscreen container (sway/sway/tree/arrange.c:310-321).
    pub fn floating_tree_ipc_shift(&self, root: NodeId) -> Option<Point<f64, Logical>> {
        self.fullscreen_window()?;
        let origin = self.output.as_ref()?.current_location().to_f64();
        self.floating.tree_ipc_shift(root, origin)
    }
}
