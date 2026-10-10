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
        // Fullscreen moving to an unrelated container ends with an arrange
        // that lays out only the new one (`cmd_fullscreen` and `view_map`,
        // sway/commands/fullscreen.c:54-55, sway/tree/view.c:931-935;
        // sway/tree/arrange.c:310-316), so everything else, the old
        // fullscreen container included, keeps the box it had.
        let replaced_unrelated = matches!(
            (current, fullscreen),
            (Some(current), Some(FullscreenMode::Workspace))
                if self.fullscreen_mode(current) == Some(FullscreenMode::Workspace)
                    && !self.contains_node(current, id)
                    && !self.contains_node(id, current)
        );
        let unarranged = if replaced_unrelated {
            let geometries = self.compute_geometry();
            geometries
                .ipc_nodes
                .into_iter()
                .filter(|(node, _)| {
                    *node != self.root
                        && !self.contains_node(id, *node)
                        && !self.split_excluded().contains(node)
                        && !self.wrapper_arranged_boxes.contains_key(node)
                })
                .collect()
        } else {
            HashMap::new()
        };
        self.fullscreen_tile_slot = false;
        self.fullscreen_pending_box = None;
        self.orphaned_global_fullscreen = None;
        self.fullscreen_arrived = false;
        // A wrapper a failed move left unarranged keeps the empty-box layout
        // too: `arrange_workspace` reaches only the new workspace fullscreen
        // container (sway/tree/arrange.c:310-316).
        let keeps_unarranged_wrapper = fullscreen == Some(FullscreenMode::Workspace)
            && self.unarranged_wrappers.iter().any(|wrapper| {
                self.nodes
                    .get(&id)
                    .and_then(|node| node.parent)
                    .is_some_and(|parent| parent == *wrapper)
            });
        if replaced_unrelated || keeps_unarranged_wrapper {
            // A `layout` wrapper's subtree keeps the empty-box layout
            // `arrange_container(wrapper)` gave it; only the new fullscreen
            // container is arranged again.
            self.wrapper_arranged_boxes.remove(&id);
        } else {
            self.wrapper_arranged_boxes.clear();
        }
        // The arrange that follows gives the new fullscreen container the
        // output box, even inside a never-arranged `layout` wrapper.
        self.fullscreen_rearranged = replaced_unrelated;
        self.stale_fullscreen_rects = stale;
        if replaced_unrelated {
            // The views hidden under the old fullscreen container stay as
            // they were; the new one is arranged.
            self.mapped_under_fullscreen.remove(&id);
            self.moved_under_fullscreen.remove(&id);
        }
        self.unarranged_under_fullscreen = unarranged;
        self.split_under_fullscreen.clear();
        for node in cleared {
            if let Some(mode) = self.pending_modes.get_mut(&node) {
                mode.fullscreen = None;
            }
        }
        if current.is_some() && !replaced_unrelated {
            self.mapped_under_fullscreen.clear();
            self.moved_under_fullscreen.clear();
            self.fullscreen_layout_wrappers.clear();
            self.pre_layout_ipc_rects.clear();
        }
        if let Some(fullscreen) = fullscreen {
            // The arrange that follows always reaches the new fullscreen container
            // (sway/tree/arrange.c:310-316 and 347-353), even when a floating container keeps
            // the workspace fullscreen beside a new global one
            // (`container_set_fullscreen`, sway/tree/container.c:1316-1323).
            self.mapped_under_fullscreen.remove(&id);
            self.moved_under_fullscreen.remove(&id);
            self.unarranged_under_fullscreen.remove(&id);
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
        self.forget_unarranged_after_sticky_carry();
        self.note_workspace_arrange();
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

    /// Any arrange of this workspace lays out the nodes a sticky carry left unarranged.
    pub(super) fn forget_unarranged_after_sticky_carry(&mut self) {
        if std::mem::take(&mut self.unarranged_after_sticky_carry) {
            self.mapped_under_fullscreen.clear();
            self.moved_under_fullscreen.clear();
            self.unarranged_under_fullscreen.clear();
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
        self.forget_unarranged_after_sticky_carry();
        self.note_workspace_arrange();
        let Some(id) = self.fullscreen_node() else {
            return;
        };
        self.fullscreen_tile_slot = false;
        self.fullscreen_rearranged = true;
        self.forget_wrapper_boxes_in(id);
    }

    /// See `fullscreen_pending_box`.
    pub fn fullscreen_pending_box(&self) -> Option<Rectangle<f64, Logical>> {
        self.fullscreen_pending_box
    }

    /// Moves the fullscreen node's pending box, as `resize_adjust_floating` does, and
    /// arranges the fullscreen subtree in it (sway/commands/resize.c:219-229).
    pub fn set_fullscreen_pending_box(&mut self, rect: Rectangle<f64, Logical>) {
        if self.fullscreen_node().is_none() {
            return;
        }
        self.fullscreen_pending_box = Some(rect);
        self.request_window_sizes();
    }

    /// Counts the arranges of this tree's workspace; see `arrange_epoch`.
    pub fn arrange_epoch(&self) -> u64 {
        self.arrange_epoch
    }

    /// Records an `arrange_workspace` or `arrange_root` reaching this tree's workspace, which
    /// puts any fullscreen container back at the output box (sway/tree/arrange.c:310-316,
    /// 349-355). Floating groups follow through `arrange_epoch`.
    pub fn note_workspace_arrange(&mut self) {
        self.arrange_epoch = self.arrange_epoch.wrapping_add(1);
        self.forget_fullscreen_pending_box();
    }

    /// Whether mapping `window` arranges the workspace. `view_map` arranges the parent when
    /// the view has one, else the workspace, and the workspace for a fullscreen view
    /// (sway/tree/view.c:931-940). A view mapped onto the workspace, or wrapped there by
    /// `workspace_layout` (`workspace_add_tiling`, sway/tree/workspace.c:948-951), has none.
    pub fn view_map_arranges_workspace(&self, window: &W::Id) -> bool {
        let Some(id) = self.node_for_window(window) else {
            return false;
        };
        if self.fullscreen_node() == Some(id) {
            return true;
        }
        let Some(parent) = self.parent_of_node(id) else {
            return false;
        };
        parent == self.root
            || (self.parent_of_node(parent) == Some(self.root) && self.split_len(parent) == Some(1))
    }

    /// An arrange puts the fullscreen node back at the output box
    /// (sway/tree/arrange.c:310-316, 349-355).
    pub fn forget_fullscreen_pending_box(&mut self) {
        if self.fullscreen_pending_box.take().is_some() {
            self.request_window_sizes();
        }
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

    /// `floating disable` on the fullscreen floating view `window`, which this
    /// tree parks at the workspace level. Sway detaches it and adds it beside
    /// the workspace's focus-inactive tiling container, or last inside it when
    /// that is a split, else last on the workspace, without touching the seat
    /// focus (`container_set_floating`, sway/tree/container.c:976-994).
    pub fn place_unfloated_fullscreen(&mut self, window: &W::Id) {
        let Some(id) = self
            .node_for_window(window)
            .filter(|id| self.nodes.get(id).and_then(|node| node.parent) == Some(self.root))
        else {
            return;
        };
        let reference = self.focus_history.iter().copied().find(|candidate| {
            *candidate != self.root
                && self.nodes.contains_key(candidate)
                && self.contains_node(self.root, *candidate)
                && !self.contains_node(id, *candidate)
                && !self.is_floating_fullscreen(*candidate)
        });
        self.move_subtree_to_node_keeping_focus(id, reference.unwrap_or(self.root));
    }

    /// `floating enable|disable` on the global fullscreen node `window`.
    /// `container_set_floating` detaches it, which clears
    /// `root->fullscreen_global`, and neither `workspace_add_floating` nor
    /// `container_add_sibling` restores it, while the view keeps
    /// `FULLSCREEN_GLOBAL` (sway/tree/container.c:941-1011, 1380-1391 and
    /// 1440-1446). `cmd_floating` then arranges the workspace like any other
    /// (sway/commands/floating.c:53-56, sway/tree/arrange.c:317-321): a
    /// floating view takes no share of the split, a tiled one reports its
    /// slot.
    pub fn orphan_global_fullscreen_on_floating(&mut self, window: &W::Id, floating: bool) {
        let Some(id) = self.node_for_window(window) else {
            return;
        };
        if self.fullscreen_node() != Some(id) || !self.orphan_global_fullscreen() {
            return;
        }
        self.fullscreen_arrived = floating;
        self.fullscreen_tile_slot = !floating;
        self.request_window_sizes();
    }

    /// `window` arrived as a global fullscreen view sway no longer tracks as
    /// `root->fullscreen_global`: `workspace_add_floating` keeps its mode 2
    /// and restores nothing (sway/tree/workspace.c:961-972), so it stays
    /// orphaned here.
    pub fn arrive_orphaned_global_fullscreen(&mut self, window: &W::Id) {
        let Some(id) = self.node_for_window(window) else {
            return;
        };
        if self.fullscreen_mode(id).is_none() {
            return;
        }
        self.set_pending_fullscreen(id, Some(FullscreenMode::Global));
        self.orphaned_global_fullscreen = Some(id);
        // It sits in `ws->floating`, so it takes no share of the split.
        self.fullscreen_arrived = true;
        self.request_window_sizes();
    }

    /// Record this tree's global fullscreen node as orphaned (see
    /// `orphaned_global_fullscreen`). Returns false when there is none.
    pub fn orphan_global_fullscreen(&mut self) -> bool {
        let Some(id) = self
            .fullscreen_node()
            .filter(|_| self.has_global_fullscreen())
        else {
            return false;
        };
        self.orphaned_global_fullscreen = Some(id);
        true
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

    /// Whether focus stays on the focused `window` when it leaves this tree
    /// for another workspace. An emptied old parent sends sway's refocus to the
    /// most recent entry on the old workspace (sway/commands/move.c:589-597).
    /// Focusing the view put its ancestors on the seat's stack outermost first
    /// (sway/input/seat.c:1178-1190), so that entry is the outermost ancestor.
    /// When that ancestor is outside the fullscreen container, the fullscreen
    /// container obstructs it and `seat_set_focus` refuses
    /// (sway/input/seat.c:1148-1151). The moved view keeps focus, and reaping
    /// the emptied containers puts its new workspace on the stack
    /// (sway/input/seat.c:304-313).
    pub fn departing_view_keeps_focus(&self, window: &W::Id) -> bool {
        let (Some(leaf), Some(fullscreen)) = (self.node_for_window(window), self.fullscreen_node())
        else {
            return false;
        };
        if fullscreen == leaf
            || self.global_fullscreen_orphaned()
            || !self.contains_node(fullscreen, leaf)
        {
            return false;
        }
        let Some(parent) = self
            .parent_of_node(leaf)
            .filter(|parent| *parent != self.root)
        else {
            return false;
        };
        if self.split_len(parent) != Some(1) {
            return false;
        }
        let mut outermost = parent;
        while let Some(next) = self
            .parent_of_node(outermost)
            .filter(|next| *next != self.root)
        {
            outermost = next;
        }
        !self.contains_node(fullscreen, outermost)
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
