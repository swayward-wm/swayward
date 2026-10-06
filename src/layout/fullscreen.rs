//! Sway fullscreen modes on windows and tiling containers.

use super::*;

impl<W: LayoutElement> Layout<W> {
    pub fn focused_fullscreen_mode(&self) -> Option<tiling_tree::FullscreenMode> {
        self.active_workspace().and_then(Workspace::fullscreen_mode)
    }

    pub fn focused_container_fullscreen_mode(&self) -> Option<tiling_tree::FullscreenMode> {
        self.active_workspace()
            .and_then(Workspace::focused_container_fullscreen_mode)
    }

    /// Sway's `root->fullscreen_global` is set. A global fullscreen view a
    /// `layout` wrap detached keeps mode 2 but no longer counts
    /// (sway/tree/workspace.c:898-910, sway/tree/container.c:1440-1446).
    pub fn global_fullscreen_active(&self) -> bool {
        self.workspaces().any(|(_, _, workspace)| {
            workspace.fullscreen_mode() == Some(tiling_tree::FullscreenMode::Global)
                && !workspace.tiling().global_fullscreen_orphaned()
        })
    }

    /// The fullscreen mode a window's own container holds, sway's
    /// `container->pending.fullscreen_mode`. A child of a fullscreen split
    /// holds none (`container_replace`, sway/tree/container.c:1471-1503).
    pub fn window_own_fullscreen_mode(&self, id: &W::Id) -> Option<tiling_tree::FullscreenMode> {
        self.workspaces().find_map(|(_, _, workspace)| {
            match workspace.tiling().node_for_window(id) {
                Some(node) => workspace.tiling().fullscreen_mode(node),
                None => workspace.floating().window_own_fullscreen_mode(id),
            }
        })
    }

    /// [`Self::window_own_fullscreen_mode`] for a split container.
    pub fn node_own_fullscreen_mode(
        &self,
        workspace_id: WorkspaceId,
        node: NodeId,
    ) -> Option<tiling_tree::FullscreenMode> {
        let workspace = self.workspace(workspace_id)?;
        if workspace.tiling().contains(node) {
            workspace.tiling().fullscreen_mode(node)
        } else {
            workspace.floating().node_own_fullscreen_mode(node)
        }
    }

    pub fn set_focused_fullscreen_mode(&mut self, mode: Option<tiling_tree::FullscreenMode>) {
        if mode.is_some() {
            for workspace in self.workspaces_mut() {
                if workspace.fullscreen_mode() == Some(tiling_tree::FullscreenMode::Global) {
                    workspace.disable_fullscreen();
                    break;
                }
            }
        }
        if let Some(workspace) = self.active_workspace_mut() {
            workspace.set_focused_fullscreen(mode);
        }
    }

    pub fn tiling_node_fullscreen_mode(
        &self,
        workspace_id: WorkspaceId,
        node: NodeId,
    ) -> Option<Option<tiling_tree::FullscreenMode>> {
        let workspace = self.workspace(workspace_id)?;
        workspace
            .tiling()
            .contains(node)
            .then(|| workspace.tiling().fullscreen_mode(node))
    }

    pub fn set_tiling_node_fullscreen_mode(
        &mut self,
        workspace_id: WorkspaceId,
        node: NodeId,
        mode: Option<tiling_tree::FullscreenMode>,
    ) {
        if mode.is_some() {
            for workspace in self.workspaces_mut() {
                if workspace.fullscreen_mode() == Some(tiling_tree::FullscreenMode::Global) {
                    workspace.disable_fullscreen();
                    break;
                }
            }
        }
        if let Some(workspace) = self.workspace_mut(workspace_id) {
            workspace.tiling_mut().set_node_fullscreen(node, mode);
        }
        if mode == Some(tiling_tree::FullscreenMode::Global) {
            let target = self
                .workspace(workspace_id)
                .and_then(|workspace| workspace.fullscreen_window().cloned());
            if let Some(window) = target {
                self.activate_window(&window);
            }
        }
    }

    pub fn fullscreen_mode(&self, id: &W::Id) -> Option<tiling_tree::FullscreenMode> {
        self.workspaces()
            .find_map(|(_, _, workspace)| workspace.fullscreen_mode_for_window(id))
    }

    pub fn set_fullscreen_mode(&mut self, id: &W::Id, mode: Option<tiling_tree::FullscreenMode>) {
        if mode.is_some() {
            for workspace in self.workspaces_mut() {
                if workspace.fullscreen_mode() == Some(tiling_tree::FullscreenMode::Global) {
                    workspace.disable_fullscreen();
                    break;
                }
            }
        }
        if mode == Some(tiling_tree::FullscreenMode::Global) {
            self.activate_window(id);
        }
        if let Some(workspace) = self
            .workspaces_mut()
            .find(|workspace| workspace.has_window(id))
        {
            workspace.activate_window(id);
            if workspace.is_floating(id) {
                workspace.set_window_fullscreen(id, mode);
                workspace.activate_window(id);
                if mode == Some(tiling_tree::FullscreenMode::Global) {
                    workspace.set_focused_fullscreen(mode);
                }
            } else {
                workspace.set_focused_fullscreen(mode);
            }
        }
    }
}
