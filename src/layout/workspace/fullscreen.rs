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

    pub fn disable_fullscreen(&mut self) {
        if let Some(fullscreen) = self.tiling.fullscreen_node() {
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
        self.tiling
            .orphan_global_fullscreen_on_floating(window, floating);
        true
    }

    pub fn active_floating_is_fullscreen(&self) -> bool {
        self.tiling.active_tile().is_some_and(|tile| {
            tile.restore_to_floating && tile.window().pending_sizing_mode().is_fullscreen()
        })
    }
}
