use super::*;

impl<W: LayoutElement> TilingTree<W> {
    pub fn split(&mut self, id: NodeId, layout: Layout) {
        self.interactive_resize = None;
        self.fullscreen_tile_slot = false;
        if id == self.root && self.split_len(id).is_some_and(|len| len > 0) {
            if !self.can_wrap_root_children() {
                return;
            }
            let Some(TreeNode::Split {
                layout: old_layout, ..
            }) = self.nodes.get(&id).map(|node| &node.value)
            else {
                return;
            };
            let old_layout = *old_layout;
            let wrapper = self.wrap_root_children(old_layout);
            if let Some(TreeNode::Split {
                layout: root_layout,
                ..
            }) = self.nodes.get_mut(&id).map(|node| &mut node.value)
            {
                *root_layout = layout;
            }
            self.stale_root_representation = Some((old_layout, self.representation_shape()));
            self.set_focus_id(Some(wrapper));
            self.request_window_sizes();
            return;
        }
        if matches!(
            self.nodes.get(&id).map(|node| &node.value),
            Some(TreeNode::Leaf { .. })
        ) {
            self.split_leaf(id, layout);
        } else {
            self.split_container(id, layout);
            self.request_window_sizes();
        }
    }

    fn split_leaf(&mut self, id: NodeId, layout: Layout) {
        let singleton_split_parent =
            self.nodes
                .get(&id)
                .and_then(|node| node.parent)
                .filter(|parent| {
                    matches!(
                        self.nodes.get(parent).map(|node| &node.value),
                        Some(TreeNode::Split {
                            layout: Layout::SplitH | Layout::SplitV,
                            children,
                            ..
                        }) if children.len() == 1
                    )
                });
        if let Some(parent) = singleton_split_parent {
            if let Some(Node {
                value: TreeNode::Split {
                    layout: current, ..
                },
                ..
            }) = self.nodes.get_mut(&parent)
            {
                *current = layout;
            }
        } else {
            self.wrap_node(id, layout);
        }
    }

    fn split_container(&mut self, id: NodeId, layout: Layout) {
        let Some(parent) = self
            .nodes
            .get(&id)
            .map(|node| node.parent.unwrap_or(self.root))
        else {
            return;
        };
        let siblings = self.split_len(parent).unwrap_or_default();
        if id == self.root {
            if let Some(Node {
                value: TreeNode::Split {
                    layout: current, ..
                },
                ..
            }) = self.nodes.get_mut(&id)
            {
                *current = layout;
            }
        } else if siblings <= 1 && parent != self.root {
            self.wrap_node(id, layout);
        } else if siblings <= 1 {
            if let Some(Node {
                value: TreeNode::Split {
                    layout: current, ..
                },
                ..
            }) = self.nodes.get_mut(&parent)
            {
                *current = layout;
            }
        } else {
            self.wrap_node(id, layout);
        }
    }

    pub fn set_layout(&mut self, id: NodeId, layout: Layout) {
        self.interactive_resize = None;
        self.fullscreen_tile_slot = false;
        if let Some(Node {
            value:
                TreeNode::Split {
                    layout: current,
                    meta,
                    ..
                },
            ..
        }) = self.nodes.get_mut(&id)
        {
            if matches!(*current, Layout::SplitH | Layout::SplitV) && *current != layout {
                meta.previous_layout = Some(*current);
            }
            *current = layout;
            self.compact_tree();
            self.request_window_sizes();
        } else {
            self.split(id, layout);
        }
    }

    /// Sets `id`'s layout without recording a split layout for `layout default`.
    /// Only the `layout` command and an empty-workspace split update sway's
    /// `prev_split_layout` (sway/commands/layout.c:171-189,
    /// sway/tree/workspace.c:1058-1063); a move that reorients the workspace
    /// leaves it alone (sway/commands/move.c:331-340).
    pub(super) fn set_layout_keeping_previous(&mut self, id: NodeId, layout: Layout) {
        let previous = self.previous_layout(id);
        self.set_layout(id, layout);
        if let Some(meta) = self.split_meta_mut(id) {
            meta.previous_layout = previous;
        }
    }

    /// Toggles the focused leaf's parent between tabbed and SplitH, splitting the leaf when it
    /// has no parent split. This backs niri's toggle-column-tabbed-display action.
    pub fn toggle_focused_tabbed(&mut self) {
        let Some(parent) = self.focus.and_then(|id| self.nodes.get(&id)?.parent) else {
            return;
        };
        let layout = match &self.nodes.get(&parent).map(|node| &node.value) {
            Some(TreeNode::Split {
                layout: Layout::Tabbed,
                ..
            }) => Layout::SplitH,
            _ => Layout::Tabbed,
        };
        self.set_layout(parent, layout);
    }

    pub fn set_focused_layout(&mut self, layout: Layout) -> Vec<(NodeId, NodeId)> {
        let root_layout = match self.nodes.get(&self.root).map(|node| &node.value) {
            Some(TreeNode::Split { layout, .. }) => *layout,
            Some(TreeNode::Leaf { .. }) | None => unreachable!(),
        };
        // Preserve the empty representation until the command changes its layout, matching the
        // old-layout comparison in sway's layout command (sway/commands/layout.c:152-189).
        if layout != root_layout {
            self.has_had_tile = true;
            self.empty_representation_layout = None;
        }
        let (target, remapped) = self.focused_layout_target();
        let Some(target) = target else {
            self.set_layout(self.root, layout);
            return remapped;
        };
        self.apply_focused_target_layout(target, layout);
        remapped
    }

    /// Applies a `layout` command's new layout to the target that
    /// [`Self::focused_layout_target`] chose. When the target is the workspace
    /// and a container is focused, sway keeps the workspace layout and wraps
    /// its children in a new container instead (sway/commands/layout.c:178-183).
    fn apply_focused_target_layout(&mut self, target: NodeId, layout: Layout) {
        let root_layout = self.split_layout(self.root);
        let container = self.focus.is_some_and(|focus| {
            focus != self.root || matches!(root_layout, Some(Layout::Tabbed | Layout::Stacked))
        });
        self.apply_layout_target(target, layout, container);
    }

    /// `container` is whether sway's handler context holds a container.
    fn apply_layout_target(&mut self, target: NodeId, layout: Layout, container: bool) {
        let root_layout = self.split_layout(self.root);
        if target == self.root && container && Some(layout) != root_layout {
            if !self.can_wrap_root_children() {
                return;
            }
            // `workspace_wrap_children` detaches every child, and detaching a
            // global fullscreen container clears `root->fullscreen_global`,
            // which reattaching does not restore (sway/tree/workspace.c:898-910,
            // sway/tree/container.c:1380-1391 and 1440-1446). The layout
            // command then arranges the workspace like any other: the wrapper
            // and the fullscreen view report their tile slots.
            let global = self
                .fullscreen_node()
                .is_some_and(|id| self.fullscreen_mode(id) == Some(FullscreenMode::Global));
            let pre_layout_ipc_rects = self
                .fullscreen_node()
                .filter(|_| !global)
                .map(|_| self.compute_geometry().ipc_nodes);
            self.fullscreen_tile_slot = global;
            let wrapper = self.wrap_root_children(layout);
            if let Some(rects) = pre_layout_ipc_rects {
                self.fullscreen_layout_wrappers.insert(wrapper);
                self.pre_layout_ipc_rects.extend(rects);
            }
            self.request_window_sizes();
        } else {
            self.set_layout_for_command(target, layout);
        }
    }

    pub(super) fn split_layout(&self, id: NodeId) -> Option<Layout> {
        match self.nodes.get(&id).map(|node| &node.value) {
            Some(TreeNode::Split { layout, .. }) => Some(*layout),
            Some(TreeNode::Leaf { .. }) | None => None,
        }
    }

    /// The workspace's own split layout (`ws->layout`), not its IPC representation.
    pub fn root_layout(&self) -> Option<Layout> {
        self.split_layout(self.root)
    }

    pub fn split_focused(&mut self, layout: Layout) {
        if let Some(focus) = self.focus {
            self.split(focus, layout);
        } else {
            // sway's workspace_split records the old layout as
            // `prev_split_layout` unconditionally, even when it is unchanged
            // or tabbed/stacked (sway/tree/workspace.c:1058-1063).
            let representation = self.representation_layout();
            let previous = self.split_layout(self.root);
            self.set_layout(self.root, layout);
            if let Some(meta) = self.split_meta_mut(self.root) {
                meta.previous_layout = previous;
            }
            self.empty_representation_layout = Some(representation);
        }
    }

    pub fn flatten_parent(&mut self, id: NodeId) -> Option<(NodeId, NodeId)> {
        self.interactive_resize = None;
        let parent = self.nodes.get(&id)?.parent?;
        if parent == self.root || self.split_len(parent) != Some(1) {
            return None;
        }
        let grandparent = self.nodes.get(&parent)?.parent?;
        let TreeNode::Split { children, .. } = &mut self.nodes.get_mut(&grandparent)?.value else {
            return None;
        };
        let slot = children.iter_mut().find(|child| **child == parent)?;
        *slot = id;
        self.nodes.get_mut(&id)?.parent = Some(grandparent);
        if let Some(fullscreen) = self
            .pending_modes
            .get(&parent)
            .and_then(|mode| mode.fullscreen)
        {
            self.set_pending_fullscreen(id, Some(fullscreen));
        }
        if self.focus == Some(parent) {
            self.set_focus_id(Some(id));
        }
        self.remove_node(parent);
        self.request_window_sizes();
        Some((parent, id))
    }

    pub fn toggle_split(&mut self, id: NodeId) {
        let layout = self
            .nodes
            .get(&id)
            .and_then(|node| node.parent)
            .and_then(|parent| self.nodes.get(&parent))
            .and_then(|parent| match parent.value {
                TreeNode::Split { layout, .. } => Some(layout),
                TreeNode::Leaf { .. } => None,
            });
        self.split(
            id,
            if layout == Some(Layout::SplitV) {
                Layout::SplitH
            } else {
                Layout::SplitV
            },
        );
    }

    pub fn toggle_focused_layout(&mut self, toggle: &LayoutToggle) -> Vec<(NodeId, NodeId)> {
        // An empty workspace has no focused container; sway toggles the workspace
        // layout itself (sway/commands/layout.c:159-163,184-188).
        if self.focus.is_none() {
            if let Some(layout) = self.toggled_layout(self.root, toggle) {
                return self.set_focused_layout(layout);
            }
            return Vec::new();
        }
        let (target, remapped) = self.focused_layout_target();
        if let Some((target, layout)) =
            target.and_then(|target| Some((target, self.toggled_layout(target, toggle)?)))
        {
            self.apply_focused_target_layout(target, layout);
        }
        remapped
    }

    /// `[criteria] layout toggle ...` on the window leaf `id`; see
    /// [`Self::set_target_layout`].
    pub fn toggle_target_layout(
        &mut self,
        id: NodeId,
        toggle: &LayoutToggle,
    ) -> Option<Vec<(NodeId, NodeId)>> {
        let parent = self.nodes.get(&id)?.parent.unwrap_or(self.root);
        let (target, remapped) = self.layout_target_from_parent(parent);
        if let Some((target, layout)) =
            target.and_then(|target| Some((target, self.toggled_layout(target, toggle)?)))
        {
            self.apply_layout_target(target, layout, true);
        }
        Some(remapped)
    }

    pub fn toggle_node_layout(&mut self, target: NodeId, toggle: &LayoutToggle) -> bool {
        let Some(layout) = self.toggled_layout(target, toggle) else {
            return false;
        };
        self.set_layout_for_command(target, layout);
        true
    }

    /// The layout `layout toggle` picks for `target`, or `None` when it is not a split.
    fn toggled_layout(&self, target: NodeId, toggle: &LayoutToggle) -> Option<Layout> {
        let current = self.split_layout(target)?;
        let tree_layout = |layout| match layout {
            swayward_ipc::command::Layout::SplitH => Some(Layout::SplitH),
            swayward_ipc::command::Layout::SplitV => Some(Layout::SplitV),
            swayward_ipc::command::Layout::Tabbed => Some(Layout::Tabbed),
            swayward_ipc::command::Layout::Stacked => Some(Layout::Stacked),
            swayward_ipc::command::Layout::ToggleSplit => None,
        };
        let next = match toggle {
            LayoutToggle::Default | LayoutToggle::Split => {
                return Some(self.split_toggled_layout(target))
            }
            LayoutToggle::All => match current {
                Layout::SplitH => Layout::SplitV,
                Layout::SplitV => Layout::Stacked,
                Layout::Stacked => Layout::Tabbed,
                Layout::Tabbed => Layout::SplitH,
            },
            LayoutToggle::Cycle(cycle) => {
                let next = cycle
                    .iter()
                    .position(|candidate| match candidate {
                        LayoutToggleEntry::Split => {
                            matches!(current, Layout::SplitH | Layout::SplitV)
                        }
                        LayoutToggleEntry::Layout(layout) => tree_layout(*layout) == Some(current),
                    })
                    .and_then(|index| cycle.get((index + 1) % cycle.len()))
                    .or_else(|| {
                        cycle
                            .iter()
                            .find(|candidate| matches!(candidate, LayoutToggleEntry::Layout(_)))
                    });
                match next {
                    Some(LayoutToggleEntry::Split) => self.split_toggled_layout(target),
                    Some(LayoutToggleEntry::Layout(layout)) => tree_layout(*layout)?,
                    None => return None,
                }
            }
        };
        Some(next)
    }

    pub(super) fn previous_layout(&self, id: NodeId) -> Option<Layout> {
        self.split_meta(id).and_then(|meta| meta.previous_layout)
    }

    pub fn restore_focused_split_layout(&mut self) -> Option<Vec<(NodeId, NodeId)>> {
        // With nothing focused the command targets the workspace and restores
        // its `prev_split_layout` (sway/commands/layout.c:106-108,160-165).
        if self.focus.is_none() {
            let layout = self.previous_layout(self.root)?;
            return Some(self.set_focused_layout(layout));
        }
        let (target, remapped) = self.focused_layout_target();
        let target = target?;
        let layout = self.previous_layout(target)?;
        self.apply_focused_target_layout(target, layout);
        Some(remapped)
    }

    /// `[criteria] layout default` on the window leaf `id`; see
    /// [`Self::set_target_layout`]. The flag is false when the target has
    /// no previous split layout, which sway rejects as invalid syntax
    /// (sway/commands/layout.c:106-108,165-167).
    pub fn restore_target_layout(&mut self, id: NodeId) -> Option<(bool, Vec<(NodeId, NodeId)>)> {
        let parent = self.nodes.get(&id)?.parent.unwrap_or(self.root);
        let (target, remapped) = self.layout_target_from_parent(parent);
        let Some((target, layout)) =
            target.and_then(|target| Some((target, self.previous_layout(target)?)))
        else {
            return Some((false, remapped));
        };
        self.apply_layout_target(target, layout, true);
        Some((true, remapped))
    }

    pub fn restore_node_layout(&mut self, target: NodeId) -> bool {
        let Some(layout) = self.previous_layout(target) else {
            return self.nodes.contains_key(&target);
        };
        self.set_layout_for_command(target, layout);
        true
    }

    pub fn toggle_focused_layout_split(&mut self) -> Vec<(NodeId, NodeId)> {
        if self.focus.is_none() {
            let layout = self.split_toggled_layout(self.root);
            return self.set_focused_layout(layout);
        }
        let (target, remapped) = self.focused_layout_target();
        if let Some(target) = target {
            let layout = self.split_toggled_layout(target);
            self.apply_focused_target_layout(target, layout);
        }
        remapped
    }

    fn split_toggled_layout(&self, target: NodeId) -> Layout {
        match self.nodes.get(&target).map(|node| &node.value) {
            Some(TreeNode::Split {
                layout: Layout::SplitH,
                ..
            }) => Layout::SplitV,
            Some(TreeNode::Split {
                layout: Layout::SplitV,
                ..
            }) => Layout::SplitH,
            // sway/commands/layout.c:37-44: the previous split, else the
            // configured orientation, else the output's longer axis.
            _ => self.previous_layout(target).unwrap_or_else(|| {
                state::default_layout(self.options.layout.default_orientation, self.view_size)
            }),
        }
    }

    /// `split toggle`: sway reads the focused container's parent layout and splits
    /// V unless that is V; a focused workspace always splits V
    /// (`sway/commands/split.c` `cmd_split`).
    pub fn toggle_focused_split(&mut self) {
        match self.focus {
            Some(focus) => self.toggle_split(focus),
            None => self.split_focused(Layout::SplitV),
        }
    }

    /// Sets the layout of the focused leaf's parent split.
    pub fn set_focused_parent_layout(&mut self, layout: Layout) {
        let Some(parent) = self.focus.and_then(|id| self.nodes.get(&id)?.parent) else {
            return;
        };
        self.set_layout(parent, layout);
    }

    /// Wraps `id` in a new container, or returns `id` unchanged when the
    /// wrapper would exceed [`MAX_TREE_DEPTH`](super::depth::MAX_TREE_DEPTH).
    pub(super) fn wrap_node(&mut self, id: NodeId, layout: Layout) -> NodeId {
        if !self.can_wrap(id) {
            return id;
        }
        let Some(parent) = self
            .nodes
            .get(&id)
            .map(|node| node.parent.unwrap_or(self.root))
        else {
            return id;
        };
        let Some(index) = self.child_index(parent, id) else {
            return id;
        };
        let Some(&old_percent) = (match self.nodes.get(&parent).map(|node| &node.value) {
            Some(TreeNode::Split { percents, .. }) => percents.get(index),
            _ => None,
        }) else {
            return id;
        };
        let wrapper = self.alloc(Node {
            parent: Some(parent),
            value: TreeNode::Split {
                layout,
                children: vec![id],
                percents: vec![1.],
                meta: SplitMeta::default(),
            },
        });
        if let Some(TreeNode::Split {
            children, percents, ..
        }) = self.nodes.get_mut(&parent).map(|node| &mut node.value)
        {
            if let (Some(child), Some(percent)) = (children.get_mut(index), percents.get_mut(index))
            {
                *child = wrapper;
                // Sway carries the fraction across the replacement in a
                // `float` and renormalises on the next arrange
                // (`container_replace`, sway/tree/container.c:1485-1490;
                // `apply_horiz_layout`, sway/tree/arrange.c:48-52), so a
                // share that lands on half a pixel can round the other way.
                *percent = f64::from(old_percent as f32);
            }
            let total: f64 = percents.iter().sum();
            if total > 0. {
                for percent in percents.iter_mut() {
                    *percent /= total;
                }
            }
        }
        if let Some(node) = self.nodes.get_mut(&id) {
            node.parent = Some(wrapper);
        }
        self.raise_split_wrapper(id, wrapper);
        if let Some(fullscreen) = self
            .pending_modes
            .get_mut(&id)
            .and_then(|mode| mode.fullscreen.take())
        {
            self.set_pending_fullscreen(wrapper, Some(fullscreen));
        }
        wrapper
    }

    /// Moves the root's children into a new container, or returns the root
    /// unchanged when that would exceed
    /// [`MAX_TREE_DEPTH`](super::depth::MAX_TREE_DEPTH).
    /// Wrap the workspace's tiling children in one new container that keeps
    /// the workspace layout (`workspace_wrap_children`,
    /// sway/tree/workspace.c:898-910).
    pub fn wrap_workspace_children(&mut self) -> Option<NodeId> {
        let layout = self
            .split_layout(self.root)
            .filter(|_| self.split_len(self.root).is_some_and(|len| len > 0))?;
        let wrapper = self.wrap_root_children(layout);
        self.request_window_sizes();
        (wrapper != self.root).then_some(wrapper)
    }

    /// `floating` on a focused workspace wraps its children and then makes the workspace
    /// horizontal without refreshing its representation, which keeps the old layout
    /// (sway/commands/floating.c:28-33). Returns the wrapper.
    pub fn wrap_workspace_children_for_floating(&mut self) -> Option<NodeId> {
        let old_layout = self.split_layout(self.root)?;
        let wrapper = self.wrap_workspace_children()?;
        if let Some(TreeNode::Split { layout, .. }) =
            self.nodes.get_mut(&self.root).map(|node| &mut node.value)
        {
            *layout = Layout::SplitH;
        }
        self.stale_root_representation = Some((old_layout, self.representation_shape()));
        Some(wrapper)
    }

    /// [`Self::wrap_workspace_children`] for a move that then fails: sway
    /// returns without arranging, so GET_TREE reports the wrapper's empty box
    /// until the next relayout (see `unarranged_wrappers`).
    pub fn wrap_workspace_children_unarranged(&mut self) {
        let unarranged = std::mem::take(&mut self.unarranged_wrappers);
        let before = self.root_children().and_then(<[_]>::first).copied();
        self.wrap_workspace_children();
        self.unarranged_wrappers = unarranged;
        if let Some(wrapper) = self
            .root_children()
            .and_then(<[_]>::first)
            .copied()
            .filter(|wrapper| Some(*wrapper) != before)
        {
            self.unarranged_wrappers.insert(wrapper);
        }
    }

    fn wrap_root_children(&mut self, layout: Layout) -> NodeId {
        if !self.can_wrap_root_children() {
            return self.root;
        }
        let Some(TreeNode::Split {
            children, percents, ..
        }) = self.nodes.get_mut(&self.root).map(|node| &mut node.value)
        else {
            return self.root;
        };
        let children = std::mem::take(children);
        let percents = std::mem::take(percents);
        let wrapper = self.alloc(Node {
            parent: Some(self.root),
            value: TreeNode::Split {
                layout,
                children: children.clone(),
                percents,
                meta: SplitMeta::default(),
            },
        });
        self.ipc_stale_nodes.insert(wrapper);
        for child in children {
            if let Some(node) = self.nodes.get_mut(&child) {
                node.parent = Some(wrapper);
            }
            self.commit_mapped_under_fullscreen(child);
        }
        if let Some(TreeNode::Split {
            children, percents, ..
        }) = self.nodes.get_mut(&self.root).map(|node| &mut node.value)
        {
            *children = vec![wrapper];
            *percents = vec![1.];
        }
        wrapper
    }

    /// `[criteria] layout <layout>` on the window leaf `id`: sway sets the
    /// handler context's container to the window, so the command takes the
    /// same path as on a focused window, flattening a singleton parent and
    /// wrapping the workspace children rather than changing the workspace
    /// layout (sway/commands/layout.c:134-149,178-183).
    pub fn set_target_layout(
        &mut self,
        id: NodeId,
        layout: Layout,
    ) -> Option<Vec<(NodeId, NodeId)>> {
        let parent = self.nodes.get(&id)?.parent.unwrap_or(self.root);
        let (target, remapped) = self.layout_target_from_parent(parent);
        if let Some(target) = target {
            self.apply_layout_target(target, layout, true);
        }
        Some(remapped)
    }

    // Sway's `layout` command replaces at most one singleton parent
    // (sway/commands/layout.c:134-149), unlike general tree compaction.
    fn set_layout_for_command(&mut self, id: NodeId, layout: Layout) {
        self.interactive_resize = None;
        self.fullscreen_tile_slot = false;
        if let Some(Node {
            value:
                TreeNode::Split {
                    layout: current,
                    meta,
                    ..
                },
            ..
        }) = self.nodes.get_mut(&id)
        {
            let changed = *current != layout;
            if matches!(*current, Layout::SplitH | Layout::SplitV) && changed {
                meta.previous_layout = Some(*current);
            }
            *current = layout;
            if changed {
                // A changed layout ends with `arrange_root` under a global
                // fullscreen container and `arrange_workspace` otherwise
                // (sway/commands/layout.c:191-195).
                if self.has_global_fullscreen() {
                    self.arrange_root();
                } else {
                    self.arrange_workspace();
                }
            }
            self.request_window_sizes();
        }
    }

    // Sway operates on the focused container's parent. If both that parent and its parent are
    // singletons, it replaces the parent with its child once and targets the grandparent
    // (sway/commands/layout.c:134-149).
    fn focused_layout_target(&mut self) -> (Option<NodeId>, Vec<(NodeId, NodeId)>) {
        let Some(focus) = self.focus else {
            return (None, Vec::new());
        };
        let target = if focus == self.root {
            self.resident_root().unwrap_or(self.root)
        } else {
            let Some(node) = self.nodes.get(&focus) else {
                return (None, Vec::new());
            };
            node.parent.unwrap_or(self.root)
        };
        self.layout_target_from_parent(target)
    }

    /// The node a `layout` command on a container whose parent is `target`
    /// changes, after flattening one singleton parent like sway
    /// (sway/commands/layout.c:134-149).
    fn layout_target_from_parent(
        &mut self,
        target: NodeId,
    ) -> (Option<NodeId>, Vec<(NodeId, NodeId)>) {
        if target == self.root
            || self.split_len(target) != Some(1)
            || self.resident_root() == Some(target)
        {
            return (Some(target), Vec::new());
        }
        let Some(grandparent) = self.nodes.get(&target).and_then(|node| node.parent) else {
            return (Some(target), Vec::new());
        };
        if grandparent == self.root || self.split_len(grandparent) != Some(1) {
            return (Some(target), Vec::new());
        }
        let Some(&child) = (match self.nodes.get(&target).map(|node| &node.value) {
            Some(TreeNode::Split { children, .. }) => children.first(),
            _ => None,
        }) else {
            return (Some(target), Vec::new());
        };
        let Some(remapped) = self.flatten_parent(child) else {
            return (Some(target), Vec::new());
        };
        (Some(grandparent), vec![remapped])
    }
}
