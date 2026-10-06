use super::*;

impl<W: LayoutElement> TilingTree<W> {
    /// Sets `id`'s pending fullscreen mode, creating its pending entry only when there is a
    /// mode to record.
    pub(super) fn set_pending_fullscreen(
        &mut self,
        id: NodeId,
        fullscreen: Option<FullscreenMode>,
    ) {
        if let Some(mode) = self.pending_modes.get_mut(&id) {
            mode.fullscreen = fullscreen;
        } else if fullscreen.is_some() {
            self.pending_modes.insert(
                id,
                PendingMode {
                    fullscreen,
                    ..PendingMode::default()
                },
            );
        }
    }

    pub fn set_fullscreen(&mut self, window: &W::Id, fullscreen: bool) -> bool {
        let Some(id) = self.node_for_window(window) else {
            return false;
        };
        self.set_node_fullscreen(id, fullscreen.then_some(FullscreenMode::Workspace))
    }

    pub fn set_node_fullscreen(&mut self, id: NodeId, fullscreen: Option<FullscreenMode>) -> bool {
        if !self.replace_fullscreen_state(id, fullscreen) {
            return false;
        }
        self.request_window_sizes_with(Some(Transaction::new()), true);
        true
    }

    pub(super) fn replace_fullscreen_state(
        &mut self,
        id: NodeId,
        fullscreen: Option<FullscreenMode>,
    ) -> bool {
        if !self.nodes.contains_key(&id) {
            return false;
        }
        if self.fullscreen_mode(id) == fullscreen {
            return false;
        }
        let current = self.fullscreen_node();
        // `container_set_fullscreen` (sway/tree/container.c:1307-1333):
        // disabling touches only `id`; workspace mode ends both the global
        // and the workspace fullscreen container; global mode ends only the
        // global one and `id`'s own workspace mode, so a workspace fullscreen
        // ancestor keeps its mode beside the new global view.
        let cleared: Vec<NodeId> = self
            .pending_modes
            .iter()
            .filter_map(|(node, mode)| {
                let mode = mode.fullscreen?;
                let keep = match fullscreen {
                    None => *node != id,
                    Some(FullscreenMode::Global) => {
                        *node != id && mode == FullscreenMode::Workspace
                    }
                    Some(FullscreenMode::Workspace) => false,
                };
                (!keep).then_some(*node)
            })
            .collect();
        // Fullscreen moving from a container to its descendant leaves the
        // container, and the splits between them, at their fullscreen boxes
        // (container_set_fullscreen, sway/tree/container.c:1312-1315).
        let stale = match (current, fullscreen) {
            (Some(current), Some(_)) if current != id && self.contains_node(current, id) => {
                let geometries = self.compute_geometry();
                let mut stale = self.stale_fullscreen_rects.clone();
                let mut node = self.nodes.get(&id).and_then(|node| node.parent);
                while let Some(ancestor) = node {
                    if let Some(rect) = geometries.ipc_nodes.get(&ancestor) {
                        stale.insert(ancestor, *rect);
                    }
                    if ancestor == current {
                        break;
                    }
                    node = self.nodes.get(&ancestor).and_then(|node| node.parent);
                }
                stale
            }
            _ => HashMap::new(),
        };
        self.fullscreen_tile_slot = false;
        self.orphaned_global_fullscreen = None;
        self.fullscreen_arrived = false;
        self.wrapper_arranged_boxes.clear();
        self.fullscreen_rearranged = false;
        self.stale_fullscreen_rects = stale;
        self.unarranged_under_fullscreen.clear();
        self.split_under_fullscreen.clear();
        for node in cleared {
            if let Some(mode) = self.pending_modes.get_mut(&node) {
                mode.fullscreen = None;
            }
        }
        if current.is_some() {
            self.mapped_under_fullscreen.clear();
            self.moved_under_fullscreen.clear();
            self.fullscreen_layout_wrappers.clear();
            self.pre_layout_ipc_rects.clear();
        }
        if let Some(fullscreen) = fullscreen {
            self.set_pending_fullscreen(id, Some(fullscreen));
            if self.focus != Some(id) {
                self.set_focus_id(self.focused_leaf_in(id));
            }
        }
        self.cancel_resize_for(id);
        true
    }

    /// Record that the current fullscreen node was moved into this tree while
    /// fullscreen, so its branch keeps no share of the parent split.
    /// Re-arrange the split holding the fullscreen node without a workspace
    /// arrange, so the fullscreen container reports its tiled slot.
    pub fn arrange_fullscreen_parent(&mut self) {
        let Some(fullscreen) = self.fullscreen_node() else {
            return;
        };
        let focused_in_fullscreen = self
            .focus
            .is_some_and(|focus| self.contains_node(fullscreen, focus));
        let parent = self.nodes.get(&fullscreen).and_then(|node| node.parent);
        if focused_in_fullscreen
            && parent.is_some_and(|parent| self.fullscreen_layout_wrappers.contains(&parent))
        {
            // The parent is a `layout` wrapper sway never arranged, so
            // `arrange_container(wrapper)` lays the fullscreen container out
            // inside its empty box (sway/tree/arrange.c:184-196 and 248-261).
            self.arrange_fullscreen_wrappers();
            return;
        }
        if focused_in_fullscreen
            && parent.is_some_and(|parent| {
                parent != self.root
                    && matches!(
                        self.nodes.get(&parent).map(|node| &node.value),
                        Some(TreeNode::Split {
                            layout: Layout::SplitH | Layout::SplitV,
                            ..
                        })
                    )
            })
        {
            self.fullscreen_tile_slot = true;
        }
    }

    /// Sway's `arrange_workspace` on this tree's workspace
    /// (sway/tree/arrange.c:310-322). A global fullscreen container is not
    /// `workspace->fullscreen`, so it is laid out in its tile slot
    /// (`container_fullscreen_global`, sway/tree/container.c:1220-1243). A
    /// workspace fullscreen container is the only thing arranged, at the
    /// output box, so a tiled slot or an empty box from an earlier
    /// `arrange_container` is gone.
    /// Without a workspace fullscreen container it arranges the tiling children, which gives a
    /// wrapper a failed move left unarranged its box (sway/tree/arrange.c:317-321).
    pub fn arrange_workspace(&mut self) {
        let Some(id) = self.fullscreen_node() else {
            self.unarranged_wrappers.clear();
            self.wrapper_arranged_boxes.clear();
            return;
        };
        if self.fullscreen_mode(id) == Some(FullscreenMode::Global) {
            self.unarranged_wrappers.clear();
            self.fullscreen_tile_slot = true;
        } else {
            self.fullscreen_tile_slot = false;
            self.fullscreen_rearranged = true;
            self.forget_wrapper_boxes_in(id);
        }
    }

    /// `arrange_container(fs)` gives the fullscreen container's descendants
    /// new boxes (sway/tree/arrange.c:310-316), so the empty-box layout an
    /// earlier `arrange_container(wrapper)` left them no longer applies.
    fn forget_wrapper_boxes_in(&mut self, fullscreen: NodeId) {
        let nodes = &self.nodes;
        self.wrapper_arranged_boxes.retain(|id, _| {
            let mut node = nodes.get(id).and_then(|node| node.parent);
            while let Some(ancestor) = node {
                if ancestor == fullscreen {
                    return false;
                }
                node = nodes.get(&ancestor).and_then(|node| node.parent);
            }
            true
        });
    }

    /// Sway's `arrange_root` reaching this tree's workspace: every fullscreen
    /// container gets the root or output box again (sway/tree/arrange.c:310-316
    /// and 340-361).
    pub fn arrange_root(&mut self) {
        let Some(id) = self.fullscreen_node() else {
            return;
        };
        self.fullscreen_tile_slot = false;
        self.fullscreen_rearranged = true;
        self.forget_wrapper_boxes_in(id);
    }

    /// Whether this tree holds a global fullscreen container
    /// (`root->fullscreen_global`).
    pub fn has_global_fullscreen(&self) -> bool {
        self.fullscreen_node()
            .is_some_and(|id| self.fullscreen_mode(id) == Some(FullscreenMode::Global))
    }

    /// The fullscreen node is a global one sway no longer tracks as
    /// `root->fullscreen_global` (see `orphaned_global_fullscreen`): it keeps
    /// mode 2 but hides nothing (`view_is_visible`, sway/tree/view.c:1195-1201)
    /// and does not stop a new view taking focus (`should_focus`,
    /// sway/tree/view.c:707-710).
    pub fn global_fullscreen_orphaned(&self) -> bool {
        self.orphaned_global_fullscreen.is_some()
            && self.orphaned_global_fullscreen == self.fullscreen_node()
            && self.has_global_fullscreen()
    }

    /// Sway's `arrange_container` on each pending fullscreen layout wrapper:
    /// the wrapper was never arranged, so its subtree is laid out inside its
    /// empty box, and those pending boxes stay until the workspace is
    /// arranged without fullscreen. See `wrapper_arranged_boxes`.
    pub(super) fn arrange_fullscreen_wrappers(&mut self) {
        if self.fullscreen_node().is_none() {
            return;
        }
        self.fullscreen_rearranged = false;
        let wrappers: Vec<NodeId> = self
            .fullscreen_layout_wrappers
            .iter()
            .copied()
            .filter(|id| self.nodes.contains_key(id))
            .collect();
        self.wrapper_arranged_boxes = self.empty_box_layout(wrappers);
    }

    /// The fullscreen view's parent, when it is a pending fullscreen layout
    /// wrapper: a container sway never arranged, so its box is still empty.
    pub fn fullscreen_view_pending_wrapper(&self, window: &W::Id) -> Option<NodeId> {
        let id = self.node_for_window(window)?;
        if self.fullscreen_node() != Some(id) {
            return None;
        }
        self.nodes
            .get(&id)
            .and_then(|node| node.parent)
            .filter(|parent| self.fullscreen_layout_wrappers.contains(parent))
    }

    /// Sway's `arrange_container(wrapper)` on a wrapper whose box is still
    /// empty, after the fullscreen view left it for the scratchpad
    /// (`root_scratchpad_add_container`, sway/tree/root.c:128-140). The
    /// wrapper reports its empty box and its subtree the boxes laid out in
    /// it, until the next relayout of this tree (see `unarranged_wrappers`).
    pub fn arrange_wrapper_at_empty_box(&mut self, wrapper: NodeId) {
        if self.fullscreen_node().is_some()
            || !matches!(
                self.nodes.get(&wrapper).map(|node| &node.value),
                Some(TreeNode::Split { children, .. }) if !children.is_empty()
            )
        {
            return;
        }
        let mut boxes = self.empty_box_layout(vec![wrapper]);
        boxes.remove(&wrapper);
        self.wrapper_arranged_boxes = boxes;
        self.unarranged_wrappers.insert(wrapper);
    }

    /// The pending boxes `arrange_children` gives the subtrees of `roots`,
    /// each laid out from an empty box.
    fn empty_box_layout(&self, roots: Vec<NodeId>) -> HashMap<NodeId, Rectangle<f64, Logical>> {
        let mut boxes = HashMap::new();
        let mut stack: Vec<(NodeId, Rectangle<f64, Logical>)> = roots
            .into_iter()
            .map(|id| (id, Rectangle::default()))
            .collect();
        while let Some((id, rect)) = stack.pop() {
            boxes.insert(id, rect);
            let Some(TreeNode::Split {
                layout,
                children,
                percents,
                ..
            }) = self.nodes.get(&id).map(|node| &node.value)
            else {
                continue;
            };
            for (index, child) in children.iter().enumerate() {
                let is_view = matches!(
                    self.nodes.get(child).map(|node| &node.value),
                    Some(TreeNode::Leaf { .. })
                );
                stack.push((
                    *child,
                    sway_child_box(
                        *layout,
                        rect,
                        percents,
                        index,
                        is_view,
                        self.titlebar_height,
                    ),
                ));
            }
        }
        boxes
    }

    pub fn mark_fullscreen_arrived(&mut self) {
        if self.fullscreen_node().is_some() {
            self.fullscreen_arrived = true;
        }
    }

    /// [`Self::mark_fullscreen_arrived`], then configure the tiled views at
    /// the shares they keep.
    pub fn mark_fullscreen_arrived_and_relayout(&mut self) {
        self.mark_fullscreen_arrived();
        self.request_window_sizes();
    }

    /// The fullscreen container this tree shows: a global fullscreen view
    /// (`root->fullscreen_global`) over a workspace fullscreen container it
    /// may sit inside (`arrange_root`, sway/tree/arrange.c:340-361).
    pub fn fullscreen_node(&self) -> Option<NodeId> {
        let find = |wanted: FullscreenMode| {
            self.pending_modes
                .iter()
                .find_map(|(id, mode)| (mode.fullscreen == Some(wanted)).then_some(*id))
        };
        find(FullscreenMode::Global).or_else(|| find(FullscreenMode::Workspace))
    }

    pub fn fullscreen_mode(&self, id: NodeId) -> Option<FullscreenMode> {
        self.pending_modes.get(&id).and_then(|mode| mode.fullscreen)
    }

    pub fn fullscreen_contains_window(&self, window: &W::Id) -> bool {
        self.fullscreen_node()
            .zip(self.node_for_window(window))
            .is_some_and(|(fullscreen, node)| self.contains_node(fullscreen, node))
    }

    pub fn fullscreen_window(&self) -> Option<&W::Id> {
        let fullscreen = self.fullscreen_node()?;
        let leaf = self.focused_leaf_in(fullscreen)?;
        self.tile(leaf).map(|tile| tile.window().id())
    }

    pub fn set_maximized(&mut self, window: &W::Id, maximized: bool) -> bool {
        let Some(id) = self.node_for_window(window) else {
            return false;
        };
        let mode = self.pending_modes.entry(id).or_default();
        if mode.maximized == maximized {
            return false;
        }
        mode.maximized = maximized;
        self.cancel_resize_for(id);
        self.request_window_sizes_with(Some(Transaction::new()), true);
        true
    }

    pub fn is_active_pending_fullscreen(&self) -> bool {
        self.focus
            .and_then(|focus| self.fullscreen_node().map(|fullscreen| (focus, fullscreen)))
            .is_some_and(|(focus, fullscreen)| self.contains_node(fullscreen, focus))
    }

    pub fn is_pending_fullscreen(&self, window: &W::Id) -> bool {
        self.node_for_window(window)
            .and_then(|id| self.pending_modes.get(&id))
            .is_some_and(|mode| mode.fullscreen.is_some())
    }

    pub fn is_pending_maximized(&self, window: &W::Id) -> bool {
        self.node_for_window(window)
            .and_then(|id| self.pending_modes.get(&id))
            .is_some_and(|mode| mode.maximized)
    }
}

/// The pending box `arrange_children` gives one child of a container at
/// `parent` (sway/tree/arrange.c:15-212): linear splits divide the box by
/// their fractions with the last child taking the rest, and drop a child
/// smaller than 10 px to an empty box; tabbed and stacked children take the
/// whole box, a non-view child below the titlebar rows. Gaps are left out:
/// sway clamps them to the room the box has, which here is none.
pub(super) fn sway_child_box(
    layout: Layout,
    parent: Rectangle<f64, Logical>,
    percents: &[f64],
    index: usize,
    is_view: bool,
    titlebar: f64,
) -> Rectangle<f64, Logical> {
    let linear = |extent: f64| {
        let start = percents
            .iter()
            .take(index)
            .map(|percent| (percent * extent).round())
            .sum::<f64>();
        let size = if index + 1 == percents.len() {
            extent - start
        } else {
            percents
                .get(index)
                .map_or(0., |percent| (percent * extent).round())
        };
        (start, size)
    };
    let sane = |rect: Rectangle<f64, Logical>| {
        if rect.size.w < 10. || rect.size.h < 10. {
            Rectangle::new(rect.loc, Size::default())
        } else {
            rect
        }
    };
    match layout {
        Layout::SplitH => {
            let (start, width) = linear(parent.size.w);
            sane(super::introspection::signed_rect(
                parent.loc.x + start,
                parent.loc.y,
                width,
                parent.size.h,
            ))
        }
        Layout::SplitV => {
            let (start, height) = linear(parent.size.h);
            sane(super::introspection::signed_rect(
                parent.loc.x,
                parent.loc.y + start,
                parent.size.w,
                height,
            ))
        }
        Layout::Tabbed | Layout::Stacked => {
            let rows = if layout == Layout::Stacked {
                percents.len()
            } else {
                1
            };
            let offset = if is_view { 0. } else { titlebar * rows as f64 };
            super::introspection::signed_rect(
                parent.loc.x,
                parent.loc.y + offset,
                parent.size.w,
                parent.size.h - offset,
            )
        }
    }
}
