//! Workspace and global fullscreen across the tiling tree and floating trees.

use super::*;

impl<W: LayoutElement> Workspace<W> {
    pub fn fullscreen_mode(&self) -> Option<crate::layout::tiling_tree::FullscreenMode> {
        self.tiling
            .fullscreen_node()
            .and_then(|id| self.tiling.fullscreen_mode(id))
            .or_else(|| self.floating.fullscreen_mode())
    }

    pub fn fullscreen_mode_for_window(
        &self,
        window: &W::Id,
    ) -> Option<crate::layout::tiling_tree::FullscreenMode> {
        self.tiling
            .node_for_window(window)
            .and_then(|node| {
                let fullscreen = self.tiling.fullscreen_node()?;
                self.tiling
                    .contains_node(fullscreen, node)
                    .then(|| self.tiling.fullscreen_mode(fullscreen))
                    .flatten()
            })
            .or_else(|| self.floating.fullscreen_mode_for_window(window))
    }

    /// The global fullscreen container here is one a detach orphaned: it
    /// keeps mode 2, but sway's `root->fullscreen_global` is unset
    /// (sway/tree/container.c:1440-1446), so it hides and blocks nothing.
    pub fn global_fullscreen_orphaned(&self) -> bool {
        self.tiling.global_fullscreen_orphaned() || self.floating.global_fullscreen_orphaned()
    }

    pub fn fullscreen_contains_window(&self, window: &W::Id) -> bool {
        self.tiling.fullscreen_contains_window(window)
            || self.floating.fullscreen_contains_window(window)
    }

    pub fn fullscreen_window(&self) -> Option<&W::Id> {
        self.tiling
            .fullscreen_window()
            .or_else(|| self.floating.fullscreen_window())
    }

    pub fn set_window_fullscreen(
        &mut self,
        window: &W::Id,
        mode: Option<crate::layout::tiling_tree::FullscreenMode>,
    ) -> bool {
        if mode.is_some() {
            self.disable_fullscreen();
        }
        if self.floating.tree_root_for_window(window).is_some() {
            return self.floating.set_window_fullscreen(window, mode);
        }
        self.set_fullscreen(window, mode.is_some());
        let Some(id) = self.tiling.node_for_window(window) else {
            return false;
        };
        self.tiling.set_node_fullscreen(id, mode)
    }

    /// A tiled container taking workspace fullscreen ends the floating container's
    /// (`container_set_fullscreen`, sway/tree/container.c:1308-1313), and
    /// `container_fullscreen_workspace` focuses the new one (:1199-1212). The group keeps
    /// its boxes until the workspace fullscreen ends. That is how a
    /// `for_window ... fullscreen enable` view mapped under a fullscreen floating group takes
    /// focus from it.
    pub fn end_floating_fullscreen_for_tiling(&mut self) {
        if self.tiling.fullscreen_node().is_some()
            || self.floating.fullscreen_mode()
                != Some(crate::layout::tiling_tree::FullscreenMode::Workspace)
        {
            return;
        }
        self.floating.yield_fullscreen();
        self.floating_is_active = FloatingActive::No;
    }

    /// `container_fullscreen_disable` (sway/tree/container.c:1246-1258). A
    /// fullscreen floating view, parked in the tiling tree here, goes back to
    /// the floating layer, since sway never took it out of `ws->floating`.
    pub fn disable_fullscreen(&mut self) {
        let floater = self
            .tiling
            .fullscreen_window()
            .filter(|window| self.window_is_fullscreen_floating(window))
            .filter(|window| self.tiling.node_for_window(window) == self.tiling.fullscreen_node())
            .cloned();
        if let Some(window) = floater {
            self.set_fullscreen(&window, false);
        } else if let Some(fullscreen) = self.tiling.fullscreen_node() {
            self.tiling.set_node_fullscreen(fullscreen, None);
        } else {
            self.floating.disable_fullscreen();
        }
    }

    /// The focused container's own fullscreen mode, the one sway's `fullscreen`
    /// toggles (sway/commands/fullscreen.c:33). After `splith` on a fullscreen
    /// view the wrapper holds the mode and the focused view holds none.
    pub fn focused_container_fullscreen_mode(
        &self,
    ) -> Option<crate::layout::tiling_tree::FullscreenMode> {
        if self.floating_is_active.get() {
            let window = self.active_window()?.id();
            if self.floating.tree_root_for_window(window).is_some() {
                return self.floating.focused_fullscreen_mode();
            }
            return self.fullscreen_mode_for_window(window);
        }
        let id = self.tiling.focus().filter(|id| !self.tiling.is_root(*id))?;
        self.tiling.fullscreen_mode(id)
    }

    pub fn set_focused_fullscreen(
        &mut self,
        mode: Option<crate::layout::tiling_tree::FullscreenMode>,
    ) -> bool {
        if self.floating_is_active.get() {
            let Some(window) = self.active_window().map(LayoutElement::id).cloned() else {
                return false;
            };
            if self.floating.tree_root_for_window(&window).is_some() {
                return self.floating.set_focused_fullscreen(mode);
            }
            self.set_fullscreen_mode(&window, mode);
            return true;
        }
        // With the workspace itself focused there is no container, and sway's
        // `fullscreen` succeeds without doing anything (sway/commands/fullscreen.c:22-25).
        let Some(id) = self.tiling.focus().filter(|id| !self.tiling.is_root(*id)) else {
            return false;
        };
        // A fullscreen floating view leaves fullscreen back into the floating
        // layer (`container_fullscreen_disable`, sway/tree/container.c).
        if mode.is_none() && self.active_floating_is_fullscreen() {
            if let Some(window) = self
                .tiling
                .active_tile()
                .map(|tile| tile.window().id().clone())
            {
                self.set_fullscreen(&window, false);
                return true;
            }
        }
        self.tiling.set_node_fullscreen(id, mode)
    }

    /// `floating enable|disable|toggle` on a fullscreen tiled or
    /// fullscreen-floating view. Sway reattaches the container without
    /// touching `fullscreen_mode`, and the new parent takes it back as the
    /// workspace's fullscreen (`container_set_floating`,
    /// sway/tree/container.c:941-1011; `container_handle_fullscreen_reparent`,
    /// :1380-1391). A fullscreen floating view is a tiled tile that restores
    /// to floating, so only that flag changes. `floating` is `None` to
    /// toggle. Returns false when `window` is not such a view.
    pub fn set_fullscreen_window_floating(
        &mut self,
        window: &W::Id,
        floating: Option<bool>,
    ) -> bool {
        if !self.tiling.is_pending_fullscreen(window) {
            return false;
        }
        // Floating appends the container to `workspace->floating`
        // (`workspace_add_floating`, sway/tree/workspace.c:961-972), so it
        // takes the next floating stack slot; tiling drops the slot.
        let stamp = self.floating.bump_stamp();
        let Some(tile) = self
            .tiling
            .tiles_mut()
            .find(|tile| tile.window().id() == window)
        else {
            return false;
        };
        let was_floating = tile.restore_to_floating;
        tile.restore_to_floating = floating.unwrap_or(!was_floating);
        if tile.restore_to_floating != was_floating {
            tile.floating_stamp = tile.restore_to_floating.then_some(stamp);
        }
        // container_set_floating moves a CSD view's border either way
        // (sway/tree/container.c:955-965, 995-1003).
        let floating = tile.restore_to_floating;
        tile.set_sway_csd_floating(floating);
        // Floating the focused view raises its old parent and then the view
        // (`container_set_floating`, sway/tree/container.c:969-973).
        if floating && !was_floating {
            self.tiling.raise_parent_then_focused(window);
        }
        if was_floating && !floating {
            self.tiling.place_unfloated_fullscreen(window);
        }
        self.tiling
            .orphan_global_fullscreen_on_floating(window, floating);
        true
    }

    /// A fullscreen floating view: swayward parks it in the tiling tree, sway
    /// keeps it in `ws->floating`, so `container_is_floating` is true for it
    /// (sway/tree/container.c:1041-1049).
    pub fn window_is_fullscreen_floating(&self, window: &W::Id) -> bool {
        self.tiling.tiles().any(|tile| {
            tile.window().id() == window
                && tile.restore_to_floating
                && tile.window().pending_sizing_mode().is_fullscreen()
        })
    }

    /// `resize_adjust_floating` on a fullscreen floating view, which swayward parks in the
    /// tiling tree. Sway measures the view's pending box, the output box until a resize moves
    /// it, or the output layout box for global fullscreen (sway/tree/arrange.c:310-316,
    /// 349-355), and keeps the moved box until the next arrange
    /// (sway/commands/resize.c:180-230). The view's content stays at the output box
    /// (`view_autoconfigure`, sway/tree/view.c:359-371). Returns false when nothing changes.
    pub fn adjust_fullscreen_floating_view(
        &mut self,
        window: &W::Id,
        edge: Option<ResizeEdge>,
        horizontal: bool,
        amount: i32,
        automatic_maximum: Size<f64, Logical>,
    ) -> bool {
        let constraints = crate::layout::floating_tree::floating_constraints(
            self.options.layout.floating_minimum_size,
            self.options.layout.floating_maximum_size,
            automatic_maximum,
        );
        let Some(mode) = self.fullscreen_mode_for_window(window) else {
            return false;
        };
        let current = match mode {
            crate::layout::tiling_tree::FullscreenMode::Workspace => self
                .tiling
                .fullscreen_pending_box()
                .unwrap_or_else(|| Rectangle::from_size(self.view_size)),
            crate::layout::tiling_tree::FullscreenMode::Global => {
                Rectangle::from_size(automatic_maximum)
            }
        };
        let Some(rect) = crate::layout::floating_tree::adjust_floating_box(
            current,
            edge,
            horizontal,
            amount,
            constraints,
        ) else {
            return false;
        };
        if mode == crate::layout::tiling_tree::FullscreenMode::Workspace {
            self.tiling.set_fullscreen_pending_box(rect);
        }
        true
    }

    /// Every arrange of this workspace puts a fullscreen floating group back at the output
    /// box (sway/tree/arrange.c:310-316, 349-355). The tiling tree counts the arranges; the
    /// floating layout catches up here.
    pub fn sync_floating_arrange(&mut self) {
        self.floating
            .sync_workspace_arrange(self.tiling.arrange_epoch());
    }

    /// `view_map` arranges the workspace unless the view has a parent
    /// (sway/tree/view.c:931-940); a view joining a floating group has one.
    pub(super) fn arrange_after_map(&mut self, window: &W::Id) {
        if self.floating.window_is_floating_root(window)
            || self.tiling.view_map_arranges_workspace(window)
        {
            self.tiling.note_workspace_arrange();
        }
        self.sync_floating_arrange();
    }

    /// `view_unmap` arranges the workspace (sway/tree/view.c:1001-1006), as does moving a
    /// view away (sway/commands/move.c:617-625). Sending one to the scratchpad arranges only
    /// its old parent when it had one (`root_scratchpad_add_container`,
    /// sway/tree/root.c:128-137).
    pub(super) fn arrange_after_removal(&mut self, had_parent: bool) {
        if !had_parent {
            self.tiling.note_workspace_arrange();
        }
        self.sync_floating_arrange();
    }

    pub fn active_floating_is_fullscreen(&self) -> bool {
        self.tiling.active_tile().is_some_and(|tile| {
            tile.restore_to_floating && tile.window().pending_sizing_mode().is_fullscreen()
        })
    }
}
