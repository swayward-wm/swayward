use super::*;

impl<W: LayoutElement> TilingTree<W> {
    pub fn visible_window_count(&self) -> usize {
        self.visible_leaves().len()
    }

    pub fn geometry(&self, id: NodeId) -> Option<Rectangle<f64, Logical>> {
        self.compute_geometry().leaf_boxes.remove(&id)
    }

    #[cfg(test)]
    pub fn titlebar_titles(&self) -> Vec<String> {
        self.compute_geometry()
            .titlebars
            .into_values()
            .map(|bar| bar.title)
            .collect()
    }

    pub fn titlebar_rects(&self) -> Vec<(W::Id, Rectangle<f64, Logical>)> {
        self.compute_geometry()
            .titlebars
            .into_values()
            .map(|bar| (bar.target, bar.rect))
            .collect()
    }

    /// Where a tiled drag drops: the closest edge of the hovered view within
    /// 0.3 * min(w, h) of it, else a swap (sway/input/seatop_move_tiling.c:271-308). Sway's two
    /// earlier passes are not implemented: a drop on a titlebar groups the views as tabs
    /// (L203-218) and a drop within 30 px of an ancestor's perpendicular edge inserts beside
    /// that ancestor (L220-268). Task review2-tiling-drop-target-missing-sway-passes tracks
    /// them; it is blocked on an oracle harness that can drive real pointer motion.
    pub fn tiled_drop_target(&self, pos: Point<f64, Logical>) -> Option<(NodeId, ResizeEdge)> {
        let geometries = self.compute_geometry();
        let visible = self.visible_leaves();
        let (id, rect) = geometries
            .ipc_nodes
            .iter()
            .filter(|(id, rect)| visible.contains(id) && rect.contains(pos))
            .min_by(|(_, a), (_, b)| {
                (a.size.w * a.size.h)
                    .partial_cmp(&(b.size.w * b.size.h))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })?;
        let distances = [
            (pos.x - rect.loc.x, ResizeEdge::LEFT),
            (pos.y - rect.loc.y, ResizeEdge::TOP),
            (rect.loc.x + rect.size.w - pos.x, ResizeEdge::RIGHT),
            (rect.loc.y + rect.size.h - pos.y, ResizeEdge::BOTTOM),
        ];
        let (distance, edge) = distances
            .into_iter()
            .min_by(|(a, _), (b, _)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))?;
        let thickness = rect.size.w.min(rect.size.h) * 0.3;
        Some((
            *id,
            if distance > thickness {
                ResizeEdge::empty()
            } else {
                edge
            },
        ))
    }

    pub fn ipc_focus_is_stale(&self, id: NodeId) -> bool {
        self.ipc_stale_nodes.contains(&id)
    }

    pub fn window_for_node(&self, id: NodeId) -> Option<&W> {
        self.tile(id).map(Tile::window)
    }

    pub(super) fn split_meta(&self, id: NodeId) -> Option<&SplitMeta> {
        match &self.nodes.get(&id)?.value {
            TreeNode::Split { meta, .. } => Some(meta),
            TreeNode::Leaf { .. } => None,
        }
    }

    pub(super) fn split_meta_mut(&mut self, id: NodeId) -> Option<&mut SplitMeta> {
        match &mut self.nodes.get_mut(&id)?.value {
            TreeNode::Split { meta, .. } => Some(meta),
            TreeNode::Leaf { .. } => None,
        }
    }

    pub fn set_title_format(&mut self, id: NodeId, format: String) -> bool {
        let Some(meta) = self.split_meta_mut(id) else {
            return false;
        };
        meta.title_format = (format != "%title").then_some(format);
        // `title_format` on a container refreshes the representation up to the workspace
        // (sway/commands/title_format.c:27).
        self.stale_root_representation = None;
        true
    }

    pub fn is_split_sticky(&self, id: NodeId) -> bool {
        self.split_meta(id).is_some_and(|meta| meta.sticky)
    }

    pub fn set_split_sticky(&mut self, id: NodeId, sticky: bool) -> bool {
        let Some(meta) = self.split_meta_mut(id) else {
            return false;
        };
        meta.sticky = sticky;
        true
    }

    pub fn windows(&self) -> impl Iterator<Item = (NodeId, &W)> {
        self.iter_depth_first().filter_map(|(id, node)| match node {
            TreeNode::Leaf { tile } => Some((id, tile.window())),
            TreeNode::Split { .. } => None,
        })
    }

    /// The tiled slot a fullscreen node reports after sway re-arranged its
    /// parent without re-arranging the workspace.
    pub(super) fn fullscreen_tile_slot_rect(
        &self,
        id: NodeId,
        geometries: &geometry::Geometry<W::Id>,
    ) -> Option<Rectangle<f64, Logical>> {
        if !self.fullscreen_tile_slot || self.fullscreen_node() != Some(id) {
            return None;
        }
        // A tabbed or stacked child's box is its parent's whole box; the
        // tab bar is drawn inside it (`apply_tabbed_layout`,
        // sway/tree/arrange.c:183-219).
        let parent = self.nodes.get(&id).and_then(|node| node.parent);
        let slot = parent
            .filter(|parent| {
                matches!(
                    self.nodes.get(parent).map(|node| &node.value),
                    Some(TreeNode::Split {
                        layout: Layout::Tabbed | Layout::Stacked,
                        ..
                    })
                )
            })
            .unwrap_or(id);
        geometries.tiled_ipc_nodes.get(&slot).copied()
    }

    pub fn ipc_tree(&self) -> IpcNode<W::Id> {
        let geometries = self.compute_geometry();
        let snapshot = IpcSnapshot::new(self, &geometries);
        let root = self.resident_root().unwrap_or(self.root);
        snapshot.node(root, None).unwrap_or_else(|| IpcNode::Split {
            id: root,
            layout: Layout::SplitH,
            title: None,
            percent: None,
            rect: Rectangle::default(),
            focus: Vec::new(),
            focused: false,
            fullscreen_mode: 0,
            sticky: false,
            children: Vec::new(),
        })
    }
}

/// One GET_TREE serialization pass. The fullscreen node and the nodes inside a pending
/// fullscreen layout wrapper are computed once here instead of per node.
struct IpcSnapshot<'a, W: LayoutElement> {
    tree: &'a TilingTree<W>,
    geometries: &'a geometry::Geometry<W::Id>,
    fullscreen: Option<NodeId>,
    /// Nodes at or below a pending fullscreen layout wrapper (empty without fullscreen).
    in_pending_wrapper: HashSet<NodeId>,
    /// [`TilingTree::active_stale_fullscreen_rects`], computed once per snapshot.
    stale_fullscreen_rects: HashMap<NodeId, Rectangle<f64, Logical>>,
}

impl<'a, W: LayoutElement> IpcSnapshot<'a, W> {
    fn new(tree: &'a TilingTree<W>, geometries: &'a geometry::Geometry<W::Id>) -> Self {
        let fullscreen = tree.fullscreen_node();
        let mut in_pending_wrapper = HashSet::new();
        if fullscreen.is_some() {
            let mut stack = tree
                .fullscreen_layout_wrappers
                .iter()
                .copied()
                .collect::<Vec<_>>();
            while let Some(id) = stack.pop() {
                // Once the workspace is arranged, `arrange_container(fs)`
                // gave the fullscreen container's descendants their boxes
                // (sway/tree/arrange.c:310-316).
                let rearranged = tree.fullscreen_rearranged
                    && fullscreen.is_some_and(|fs| fs != id && tree.contains_node(fs, id));
                if rearranged || !tree.nodes.contains_key(&id) || !in_pending_wrapper.insert(id) {
                    continue;
                }
                if let Some(TreeNode::Split { children, .. }) =
                    tree.nodes.get(&id).map(|node| &node.value)
                {
                    stack.extend(children.iter().copied());
                }
            }
        }
        Self {
            tree,
            geometries,
            fullscreen,
            in_pending_wrapper,
            stale_fullscreen_rects: tree.active_stale_fullscreen_rects(),
        }
    }

    /// A pending fullscreen layout wrapper itself: reported with percent 0 and an empty rect.
    fn is_pending_wrapper(&self, id: NodeId) -> bool {
        self.fullscreen.is_some() && self.tree.fullscreen_layout_wrappers.contains(&id)
    }

    /// A wrapper sway never arranged (see `unarranged_wrappers`, and a split
    /// in `moved_under_fullscreen`): reported with calloc's empty box, so its
    /// children omit percent.
    fn is_unarranged_wrapper(&self, id: NodeId) -> bool {
        self.tree.unarranged_wrappers.contains(&id)
            || (self.tree.moved_under_fullscreen.contains_key(&id) && !self.is_view(id))
    }

    fn under_tabbed_pending_wrapper(&self, id: NodeId) -> bool {
        self.tree
            .nodes
            .get(&id)
            .and_then(|node| node.parent)
            .filter(|parent| self.is_pending_wrapper(*parent))
            .and_then(|parent| self.tree.nodes.get(&parent))
            .is_some_and(|parent| {
                matches!(
                    parent.value,
                    TreeNode::Split {
                        layout: Layout::Tabbed | Layout::Stacked,
                        ..
                    }
                )
            })
    }

    fn node(&self, id: NodeId, percent: Option<f64>) -> Option<IpcNode<W::Id>> {
        let inside_pending_wrapper = self.in_pending_wrapper.contains(&id);
        // Sway's percent is the pending box's area over the parent's, and is
        // omitted when the parent box is empty (sway/ipc-json.c:744-755).
        let percent = match self.unarranged(id) {
            Some(UnarrangedIpc {
                percent_parent: Some(parent),
                ..
            }) => {
                if parent.size.w == 0. || parent.size.h == 0. {
                    None
                } else {
                    self.arranged_wrapper_box(id)
                        .map(|own| (own.size.w / parent.size.w) * (own.size.h / parent.size.h))
                }
            }
            _ => percent,
        };
        Some(match &self.tree.nodes.get(&id)?.value {
            TreeNode::Split {
                layout,
                children,
                percents,
                ..
            } => self.split(id, *layout, children, percents, percent),
            TreeNode::Leaf { tile } => self.leaf(id, tile, percent, inside_pending_wrapper),
        })
    }

    fn split(
        &self,
        id: NodeId,
        layout: Layout,
        children: &[NodeId],
        percents: &[f64],
        percent: Option<f64>,
    ) -> IpcNode<W::Id> {
        let tree = self.tree;
        let pending_wrapper = self.is_pending_wrapper(id);
        let child_percents = self.child_percents(id, layout, children, percents);
        let titlebar_rows = self.titlebar_rows(layout, pending_wrapper, children.len());
        IpcNode::Split {
            id,
            layout,
            title: tree
                .split_meta(id)
                .and_then(|meta| meta.title_format.clone()),
            percent: if self.is_unarranged_wrapper(id) {
                // Its empty box over a nonempty parent's (sway/ipc-json.c:744-755).
                percent.map(|_| 0.)
            } else {
                pending_wrapper.then_some(0.).or(percent)
            },
            rect: if pending_wrapper || tree.unarranged_wrappers.contains(&id) {
                Rectangle::default()
            } else if let Some(unarranged) = self.unarranged(id) {
                unarranged.rect
            } else if self.is_unarranged_wrapper(id) {
                Rectangle::default()
            } else if let Some(mut pre_layout) = self
                .in_pending_wrapper
                .contains(&id)
                .then(|| tree.pre_layout_ipc_rects.get(&id).copied())
                .flatten()
            {
                // A split moved into a pending fullscreen layout wrapper keeps
                // its pre-layout box, less a tab bar when the wrapper is tabbed
                // or stacked. The fullscreen container itself keeps its box.
                if self.fullscreen != Some(id) && self.under_tabbed_pending_wrapper(id) {
                    pre_layout.loc.y += tree.titlebar_height;
                    pre_layout.size.h = (pre_layout.size.h - tree.titlebar_height).max(0.);
                }
                pre_layout
            } else if let Some(slot) = tree.fullscreen_tile_slot_rect(id, self.geometries) {
                slot
            } else {
                self.geometries
                    .ipc_nodes
                    .get(&id)
                    .copied()
                    .unwrap_or_default()
            },
            focus: self.focus_order(children),
            focused: tree.focus == Some(id),
            fullscreen_mode: tree.fullscreen_mode(id).map_or(0, |mode| mode as i32),
            sticky: tree.is_split_sticky(id),
            children: children
                .iter()
                .zip(child_percents)
                .filter_map(|(child, percent)| {
                    let mut node = self.node(*child, percent)?;
                    if self.unarranged(*child).is_some() {
                        // Already reported as GET_TREE shows it.
                        return Some(node);
                    }
                    let inset = tree.titlebar_height * titlebar_rows as f64;
                    if self.is_unarranged_wrapper(id) {
                        // Sway never arranged the wrapper, so each child keeps
                        // the box the workspace strip gave it; the strip below
                        // the wrapper's own tab bar was never applied.
                        lift_unarranged_strip(&mut node, inset, true);
                    }
                    match &mut node {
                        // GET_TREE subtracts the tab rows from the wrapper's
                        // empty box, so its height goes negative
                        // (sway/ipc-json.c:816-825).
                        IpcNode::Split { rect, .. } if self.is_unarranged_wrapper(*child) => {
                            *rect = signed_rect(
                                rect.loc.x,
                                rect.loc.y + inset,
                                rect.size.w,
                                rect.size.h - inset,
                            );
                        }
                        _ => inset_split_by_parent_titlebar(&mut node, inset),
                    }
                    Some(node)
                })
                .collect(),
        }
    }

    /// Children in focus order, most recent first, followed by any never focused. Stale IPC
    /// focus entries (fresh wrappers, see `ipc_stale_nodes`) do not count.
    fn focus_order(&self, children: &[NodeId]) -> Vec<NodeId> {
        let tree = self.tree;
        tree.children_in_focus_order(children, |entry| tree.ipc_stale_nodes.contains(&entry))
    }

    /// Titlebar rows a tabbed or stacked parent reserves above each child's rect.
    fn titlebar_rows(&self, layout: Layout, pending_wrapper: bool, len: usize) -> usize {
        match layout {
            Layout::Tabbed | Layout::Stacked if pending_wrapper => 0,
            Layout::Tabbed => 1,
            Layout::Stacked => len,
            Layout::SplitH | Layout::SplitV => 0,
        }
    }

    /// The `percent` GET_TREE reports for each child, computed once per parent.
    fn child_percents(
        &self,
        id: NodeId,
        layout: Layout,
        children: &[NodeId],
        percents: &[f64],
    ) -> Vec<Option<f64>> {
        match layout {
            // A pending wrapper reports a 0x0 box, and sway omits percent
            // when the parent box is empty (sway/ipc-json.c:744-755).
            _ if self.is_pending_wrapper(id) || self.is_unarranged_wrapper(id) => {
                vec![None; children.len()]
            }
            // Every child fills the strip's content box, so it reports 1,
            // except a fullscreen child, whose box is the output's.
            Layout::Tabbed | Layout::Stacked => children
                .iter()
                .map(|child| {
                    Some(if self.fullscreen == Some(*child) {
                        self.fullscreen_child_percent(id, *child, 1.)
                    } else {
                        1.
                    })
                })
                .collect(),
            // The fullscreen container lays its children out over the output
            // box (sway/tree/arrange.c:310-316).
            Layout::SplitH | Layout::SplitV
                if self.fullscreen == Some(id) && self.tree.fullscreen_rearranged =>
            {
                self.split_percents(id, layout, children, percents)
            }
            Layout::SplitH | Layout::SplitV if self.fullscreen.is_some() => {
                self.pending_split_percents(id, layout, children, percents)
            }
            Layout::SplitH | Layout::SplitV => self.split_percents(id, layout, children, percents),
        }
    }

    /// Split percents while a fullscreen view exists. Sway measures against the pre-fullscreen
    /// tile slots, so the rects come from `pre_layout_ipc_rects` or the tiled pass; hidden
    /// children report 0 and the others are renormalised over the visible ones, and the
    /// fullscreen child reports its slot area over the parent's.
    fn pending_split_percents(
        &self,
        id: NodeId,
        layout: Layout,
        children: &[NodeId],
        percents: &[f64],
    ) -> Vec<Option<f64>> {
        let tree = self.tree;
        let geometries = self.geometries;
        let rect = |child: &NodeId| {
            tree.pre_layout_ipc_rects
                .get(child)
                .or_else(|| geometries.tiled_ipc_nodes.get(child))
                .copied()
        };
        let parent_rect = rect(&id).unwrap_or_default();
        let parent_extent = geometry::axis_extent(layout, parent_rect).round();
        let allocated = child_shares(parent_extent, percents);
        let excluded = tree.split_excluded();
        let visible_total = children
            .iter()
            .zip(percents)
            .filter(|(child, _)| !excluded.contains(child))
            .map(|(_, percent)| percent)
            .sum::<f64>();
        // Sway leaves the other children at the whole-pixel boxes they last
        // had over the visible children (`arrange_workspace`,
        // sway/tree/arrange.c:310-316).
        let visible_shares: Vec<f64> = children
            .iter()
            .zip(percents)
            .map(|(child, percent)| {
                if excluded.contains(child) {
                    0.
                } else {
                    percent / visible_total
                }
            })
            .collect();
        let visible_allocated = geometry::whole_pixel_extents(parent_extent, &visible_shares);
        children
            .iter()
            .zip(percents)
            .zip(allocated)
            .zip(visible_allocated)
            .map(
                |(((child, stored_percent), allocated), visible_allocated)| {
                    let is_fullscreen = self.fullscreen == Some(*child);
                    Some(
                        if let Some(stale) = self
                            .stale_fullscreen_rects
                            .get(child)
                            .filter(|_| area(parent_rect) > 0.)
                        {
                            // Sway's percent is the box's area over the parent's
                            // (sway/ipc-json.c:744-755).
                            area(*stale) / area(parent_rect)
                        } else if excluded.contains(child) && !is_fullscreen {
                            0.
                        } else if !excluded.is_empty() && !is_fullscreen {
                            if parent_extent > 0. {
                                visible_allocated / parent_extent
                            } else {
                                *stored_percent / visible_total
                            }
                        } else if is_fullscreen {
                            self.fullscreen_child_percent(id, *child, *stored_percent)
                        } else if parent_extent > 0. {
                            allocated / parent_extent
                        } else {
                            *stored_percent
                        },
                    )
                },
            )
            .collect()
    }

    /// A fullscreen child's percent: its box's area over its parent's
    /// pending box (sway/ipc-json.c:744-755), both as before the fullscreen
    /// pass unless the child reports its tile slot.
    fn fullscreen_child_percent(&self, parent: NodeId, child: NodeId, fallback: f64) -> f64 {
        let tree = self.tree;
        let geometries = self.geometries;
        let parent_rect = tree
            .pre_layout_ipc_rects
            .get(&parent)
            .copied()
            .unwrap_or_else(|| {
                let mut rect = geometries
                    .tiled_ipc_nodes
                    .get(&parent)
                    .copied()
                    .unwrap_or_default();
                // Only a rounding correction: a parent that keeps a stale box
                // (see `stale_fullscreen_rects`) is not its share of the split.
                let extent = self.allocated_extent(parent);
                match self.parent_layout(parent) {
                    Some(Layout::SplitH) => {
                        if let Some(extent) = extent.filter(|e| (e - rect.size.w).abs() < 1.) {
                            rect.size.w = extent;
                        }
                    }
                    Some(Layout::SplitV) => {
                        if let Some(extent) = extent.filter(|e| (e - rect.size.h).abs() < 1.) {
                            rect.size.h = extent;
                        }
                    }
                    _ => {}
                }
                rect
            });
        let child_rect = tree
            .fullscreen_tile_slot_rect(child, geometries)
            .map(|mut slot| {
                // The tile slot is the child's whole-pixel share too.
                if let Some(extent) = self.allocated_extent(child) {
                    match self.parent_layout(child) {
                        Some(Layout::SplitH) if (extent - slot.size.w).abs() < 1. => {
                            slot.size.w = extent;
                        }
                        Some(Layout::SplitV) if (extent - slot.size.h).abs() < 1. => {
                            slot.size.h = extent;
                        }
                        _ => {}
                    }
                }
                slot
            })
            .or_else(|| geometries.ipc_nodes.get(&child).copied())
            .unwrap_or_default();
        let parent_area = parent_rect.size.w.round() * parent_rect.size.h.round();
        let child_area = child_rect.size.w.round() * child_rect.size.h.round();
        if parent_area > 0. {
            child_area / parent_area
        } else {
            fallback
        }
    }

    fn parent_layout(&self, id: NodeId) -> Option<Layout> {
        let parent = self.tree.nodes.get(&id)?.parent?;
        match &self.tree.nodes.get(&parent)?.value {
            TreeNode::Split { layout, .. } => Some(*layout),
            TreeNode::Leaf { .. } => None,
        }
    }

    /// The whole-pixel extent sway gives `id` along its linear parent's axis:
    /// `round(fraction * child_total)`, the last child taking the remainder
    /// (sway/tree/arrange.c:78-88 and 160-174). The float layout would round
    /// a third of 1280 to 427 where sway's last column gets 426.
    fn allocated_extent(&self, id: NodeId) -> Option<f64> {
        let tree = self.tree;
        let parent = tree.nodes.get(&id)?.parent?;
        let TreeNode::Split {
            layout: layout @ (Layout::SplitH | Layout::SplitV),
            children,
            percents,
            ..
        } = &tree.nodes.get(&parent)?.value
        else {
            return None;
        };
        let index = children.iter().position(|child| *child == id)?;
        let available = children
            .iter()
            .filter_map(|child| self.geometries.tiled_ipc_nodes.get(child).copied())
            .map(|rect| geometry::axis_extent(*layout, rect))
            .sum::<f64>();
        child_shares(available.round(), percents)
            .get(index)
            .copied()
    }

    /// Split percents without fullscreen. Sway reports percent as
    /// (w / parent_w) * (h / parent_h) (sway/ipc-json.c:751-754) with child extents
    /// round(fraction * child_total) and the last child taking the remainder
    /// (sway/tree/arrange.c:78-88 and 160-174), so the children share the space left after
    /// inner gaps and the result is taken over the rounded parent extent.
    fn split_percents(
        &self,
        id: NodeId,
        layout: Layout,
        children: &[NodeId],
        percents: &[f64],
    ) -> Vec<Option<f64>> {
        let geometries = self.geometries;
        let parent_extent = geometries
            .ipc_nodes
            .get(&id)
            .copied()
            .map(|rect| geometry::axis_extent(layout, rect).round())
            .unwrap_or_default();
        let available = children
            .iter()
            .filter_map(|child| geometries.ipc_nodes.get(child).copied())
            .map(|rect| geometry::axis_extent(layout, rect))
            .sum::<f64>();
        // Sway's container widths are integers, so its child_total_width is too
        // (apply_horiz_layout, sway/tree/arrange.c:70-88). A floating group's fractional rect
        // would otherwise round the first child's share the other way.
        let allocated = child_shares(available.round(), percents);
        percents
            .iter()
            .zip(allocated)
            .map(|(stored_percent, allocated)| {
                Some(if parent_extent > 0. {
                    allocated / parent_extent
                } else {
                    *stored_percent
                })
            })
            .collect()
    }

    fn leaf(
        &self,
        id: NodeId,
        tile: &crate::layout::tile::Tile<W>,
        percent: Option<f64>,
        inside_pending_wrapper: bool,
    ) -> IpcNode<W::Id> {
        let tree = self.tree;
        let geometries = self.geometries;
        let fallback_titlebar = self.fullscreen.is_some()
            && tree.fullscreen_mode(id).is_none()
            && !tree.mapped_under_fullscreen.contains(&id)
            && !tree.moved_under_fullscreen.contains_key(&id)
            && tile.has_configured_sway_titlebar()
            && (inside_pending_wrapper
                || !geometries.titlebars.contains_key(&id)
                || tree.pre_layout_ipc_rects.contains_key(&id));
        let mut rect = tree
            .fullscreen_tile_slot_rect(id, geometries)
            .or_else(|| tree.pre_layout_ipc_rects.get(&id).copied())
            .or_else(|| {
                (!inside_pending_wrapper)
                    .then(|| geometries.leaf_ipc_rects.get(&id).copied())
                    .flatten()
            })
            .unwrap_or_default();
        if fallback_titlebar {
            rect.loc.y += tree.titlebar_height;
            if inside_pending_wrapper {
                rect.size.h -= tree.titlebar_height;
            } else {
                rect.size.h = (rect.size.h - tree.titlebar_height).max(0.);
            }
        }
        IpcNode::Leaf {
            id,
            window: tile.window().id().clone(),
            percent,
            focused: tree.focus == Some(id),
            fullscreen_mode: tree.fullscreen_mode(id).map_or(0, |mode| mode as i32),
            rect,
            deco_rect: (!inside_pending_wrapper)
                .then(|| geometries.titlebars.get(&id).map(|bar| bar.ipc_rect))
                .flatten()
                .or_else(|| {
                    (tree.moved_under_fullscreen.contains_key(&id)
                        && tile.has_configured_sway_titlebar())
                    .then(|| Rectangle::new(Point::default(), (0., tree.titlebar_height).into()))
                })
                .or_else(|| {
                    fallback_titlebar.then(|| {
                        Rectangle::new(Point::default(), (rect.size.w, tree.titlebar_height).into())
                    })
                }),
            border: tile.sway_border_thickness(),
            border_edges: geometries
                .border_edges
                .get(&id)
                .copied()
                .unwrap_or_else(ResizeEdge::all),
            sticky: tile.is_sticky,
            mapped_under_fullscreen: tree.mapped_under_fullscreen.contains(&id),
            moved_under_fullscreen: tree.moved_under_fullscreen.get(&id).copied(),
            unarranged: self.unarranged(id),
        }
    }

    /// The boxes sway reports for a node it left unarranged beside a
    /// fullscreen container, or `None` where the regular boxes apply.
    ///
    /// Sway never arranges the workspace's tiling children while it has a
    /// fullscreen container (sway/tree/arrange.c:310-316), so such a node keeps
    /// whatever pending box it last had:
    ///
    /// - calloc's empty box for a view mapped under fullscreen;
    /// - its pre-`layout` box for a child of a fresh `layout` wrapper;
    /// - for the subtree of a wrapper a view was mapped into, the boxes
    ///   `arrange_container(wrapper)` derived from the wrapper's own empty box
    ///   (sway/tree/view.c:931-940).
    ///
    /// GET_TREE then reports that box less the titlebar rows a tabbed or
    /// stacked parent claims, whatever the child's border (`get_deco_rect`
    /// and `ipc_json_describe_node`, sway/ipc-json.c:543-580 and 816-825).
    fn unarranged(&self, id: NodeId) -> Option<UnarrangedIpc> {
        let tree = self.tree;
        let parent = tree.nodes.get(&id)?.parent?;
        if self.fullscreen == Some(id)
            && tree.fullscreen_rearranged
            && self.in_pending_wrapper.contains(&id)
        {
            // `arrange_workspace` puts the fullscreen container back at the
            // output box (sway/tree/arrange.c:310-316).
            return Some(UnarrangedIpc {
                rect: self.geometries.ipc_nodes.get(&id).copied()?,
                deco_rect: Rectangle::default(),
                window_rect: None,
                percent_parent: None,
                absolute: false,
            });
        }
        if let Some(sway_box) = self.arranged_wrapper_box(id) {
            if self.fullscreen == Some(id) {
                return Some(UnarrangedIpc {
                    rect: sway_box,
                    deco_rect: Rectangle::default(),
                    window_rect: None,
                    percent_parent: None,
                    absolute: true,
                });
            }
            let parent_box = self.arranged_wrapper_box(parent).unwrap_or_default();
            let mut ipc = self.sway_ipc_boxes(id, parent, sway_box, parent_box);
            if self.is_view(id) {
                ipc.window_rect = Some(self.configured_content(id, parent, sway_box));
            }
            ipc.percent_parent = Some(parent_box);
            ipc.absolute = true;
            return Some(ipc);
        }
        let fullscreen = self.fullscreen?;
        if id == fullscreen || !self.is_strip(parent) {
            return None;
        }
        let parent_box = if self.is_pending_wrapper(parent) {
            Rectangle::default()
        } else {
            self.geometries
                .ipc_nodes
                .get(&parent)
                .copied()
                .unwrap_or_default()
        };
        let never_arranged = tree.mapped_under_fullscreen.contains(&id)
            || tree
                .moved_under_fullscreen
                .get(&id)
                .is_some_and(|rect| rect.size.w == 0. && rect.size.h == 0. && rect.loc.x == 0.);
        if never_arranged {
            let mut ipc = self.sway_ipc_boxes(id, parent, Rectangle::default(), parent_box);
            ipc.absolute = true;
            return Some(ipc);
        }
        if self.is_pending_wrapper(parent) {
            let sway_box = tree.pre_layout_ipc_rects.get(&id).copied()?;
            return Some(self.sway_ipc_boxes(id, parent, sway_box, parent_box));
        }
        None
    }

    fn is_view(&self, id: NodeId) -> bool {
        matches!(
            self.tree.nodes.get(&id).map(|node| &node.value),
            Some(TreeNode::Leaf { .. })
        )
    }

    fn is_strip(&self, id: NodeId) -> bool {
        matches!(
            self.tree.nodes.get(&id).map(|node| &node.value),
            Some(TreeNode::Split {
                layout: Layout::Tabbed | Layout::Stacked,
                ..
            })
        )
    }

    /// The pending box sway's `arrange_container(wrapper)` gave `id`, when
    /// `id` is at or below a pending `layout` wrapper a view was mapped into,
    /// or below one the fullscreen view left for the scratchpad (see
    /// `TilingTree::wrapper_arranged_boxes`).
    fn arranged_wrapper_box(&self, id: NodeId) -> Option<Rectangle<f64, Logical>> {
        self.tree.wrapper_arranged_boxes.get(&id).copied()
    }

    /// GET_TREE's `rect` and `deco_rect` for a node with pending box
    /// `sway_box` under a parent with pending box `parent_box`
    /// (`get_deco_rect` and `ipc_json_describe_node`, sway/ipc-json.c:543-580
    /// and 816-825). Sway's `hide_lone_tab` is always off here.
    fn sway_ipc_boxes(
        &self,
        id: NodeId,
        parent: NodeId,
        sway_box: Rectangle<f64, Logical>,
        parent_box: Rectangle<f64, Logical>,
    ) -> UnarrangedIpc {
        let tree = self.tree;
        let (layout, index, count) = match tree.nodes.get(&parent).map(|node| &node.value) {
            Some(TreeNode::Split {
                layout, children, ..
            }) => (
                *layout,
                children
                    .iter()
                    .position(|child| *child == id)
                    .unwrap_or_default(),
                children.len(),
            ),
            _ => (Layout::SplitH, 0, 1),
        };
        let strip = matches!(layout, Layout::Tabbed | Layout::Stacked);
        // A container's border is calloc's `B_NONE`; a view's is its own.
        let normal = tree.tile(id).is_some_and(|tile| {
            tile.sway_border_thickness().0 == swayward_ipc::command::BorderStyle::Normal
        });
        let titlebar = tree.titlebar_height;
        let mut deco = Rectangle::default();
        if strip || normal {
            deco = signed_rect(
                sway_box.loc.x - parent_box.loc.x,
                sway_box.loc.y - parent_box.loc.y,
                sway_box.size.w,
                titlebar,
            );
            match layout {
                Layout::Tabbed => {
                    deco.size.w = (parent_box.size.w / count.max(1) as f64).trunc();
                    deco.loc.x += deco.size.w * index as f64;
                }
                Layout::Stacked => {
                    if !self.is_view(id) {
                        deco.loc.y -= titlebar * count as f64;
                    }
                    deco.loc.y += titlebar * index as f64;
                }
                Layout::SplitH | Layout::SplitV => {}
            }
        }
        let rows = if layout == Layout::Stacked { count } else { 1 };
        let offset = deco.size.h * rows as f64;
        UnarrangedIpc {
            rect: signed_rect(
                sway_box.loc.x,
                sway_box.loc.y + offset,
                sway_box.size.w,
                sway_box.size.h - offset,
            ),
            deco_rect: deco,
            window_rect: None,
            percent_parent: None,
            absolute: false,
        }
    }

    /// The content box `view_autoconfigure` gives a view at `sway_box`,
    /// relative to it as `window_rect` reports it: inset by the side borders
    /// and below any titlebar, and never smaller than 1x1
    /// (sway/tree/view.c:376-463, sway/ipc-json.c:595-602).
    fn configured_content(
        &self,
        id: NodeId,
        parent: NodeId,
        sway_box: Rectangle<f64, Logical>,
    ) -> Rectangle<f64, Logical> {
        use swayward_ipc::command::BorderStyle;
        let tree = self.tree;
        let (style, width) = tree
            .tile(id)
            .map(|tile| tile.sway_border_thickness())
            .unwrap_or((BorderStyle::None, 0));
        let side = match style {
            BorderStyle::Normal | BorderStyle::Pixel => f64::from(width),
            _ => 0.,
        };
        let rows = match tree.nodes.get(&parent).map(|node| &node.value) {
            Some(TreeNode::Split {
                layout: Layout::Stacked,
                children,
                ..
            }) => children.len() as f64,
            Some(TreeNode::Split {
                layout: Layout::Tabbed,
                ..
            }) => 1.,
            _ if style == BorderStyle::Normal => 1.,
            _ => 0.,
        };
        Rectangle::new(
            Point::from((side, 0.)),
            Size::from((
                (sway_box.size.w - 2. * side).max(1.),
                (sway_box.size.h - rows * tree.titlebar_height - side).max(1.),
            )),
        )
    }
}

/// Moves `node`'s subtree up by `height`, growing the top node by the same
/// amount: the strip offset a never-arranged wrapper did not apply.
fn lift_unarranged_strip<I>(node: &mut IpcNode<I>, height: f64, grow: bool) {
    let (IpcNode::Split { rect, .. } | IpcNode::Leaf { rect, .. }) = node;
    rect.loc.y -= height;
    if grow {
        rect.size.h += height;
    }
    if let IpcNode::Split { children, .. } = node {
        for child in children {
            lift_unarranged_strip(child, height, false);
        }
    }
}

/// A fullscreen container has no titlebar row (`get_deco_rect`,
/// sway/ipc-json.c:543-553), so it keeps its box.
fn inset_split_by_parent_titlebar<I>(node: &mut IpcNode<I>, height: f64) {
    if let IpcNode::Split {
        rect,
        fullscreen_mode: 0,
        ..
    } = node
    {
        rect.loc.y += height;
        rect.size.h = (rect.size.h - height).max(0.);
    }
}

/// Each child's whole-pixel share of `extent`: `round(extent * percent)`, with the last child
/// taking the remainder (sway/tree/arrange.c:171-174), clamped at zero for a sub-pixel last
/// child. The prefix sum is accumulated once instead of per child.
fn child_shares(extent: f64, percents: &[f64]) -> Vec<f64> {
    let mut earlier = 0.;
    percents
        .iter()
        .enumerate()
        .map(|(index, percent)| {
            if index + 1 == percents.len() {
                (extent.round() - earlier).max(0.)
            } else {
                let share = (extent * percent).round();
                earlier += share;
                share
            }
        })
        .collect()
}

fn area(rect: Rectangle<f64, Logical>) -> f64 {
    rect.size.w.round() * rect.size.h.round()
}

/// A rectangle whose size may be negative, as sway's pending boxes are once
/// GET_TREE subtracts titlebar rows from an empty box. `Size::new` rejects a
/// negative size in debug builds, so the fields are assigned directly.
pub(super) fn signed_rect(x: f64, y: f64, w: f64, h: f64) -> Rectangle<f64, Logical> {
    let mut rect = Rectangle::<f64, Logical> {
        loc: Point::from((x, y)),
        ..Default::default()
    };
    rect.size.w = w;
    rect.size.h = h;
    rect
}
