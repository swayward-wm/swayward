use super::*;

impl<W: LayoutElement> TilingTree<W> {
    pub fn resize_adjacent(&mut self, first: NodeId, second: NodeId, delta: f64) -> bool {
        if self.never_arranged_in_parent(first) {
            return false;
        }
        let old = self.compute_geometry();
        let boxes = self.resize_boxes(&old);
        if !self.adjacent_fits_min_sane(&boxes, first, second, delta) {
            return false;
        }
        let snapped = self.snap_shares_to_pixels(&boxes, first);
        let changed = self.resize_adjacent_inner(first, second, delta);
        if !changed {
            if let Some((parent, percents)) = snapped {
                self.restore_shares(parent, percents);
            }
        }
        if changed {
            self.arrange_after_resize(&old, first);
            self.interactive_resize = None;
            self.animate_geometry_changes(old, None);
        }
        changed
    }

    /// A container mapped, moved or split under workspace fullscreen was
    /// never laid out by its parent, so its `child_total_width` and
    /// `child_total_height` are still 0 and sway's resize returns before
    /// changing anything (`container_resize_tiled`,
    /// sway/commands/resize.c:121-123 and 145-147).
    fn never_arranged_in_parent(&self, id: NodeId) -> bool {
        self.split_under_fullscreen.contains(&id) || self.split_excluded().contains(&id)
    }

    /// The boxes sway's resize code reads: the IPC boxes, except that a
    /// fullscreen container sway last arranged in its tile slot measures that
    /// slot, not the output (`container_resize_tiled` compares
    /// `pending.width`, sway/commands/resize.c:113-131).
    fn resize_boxes(
        &self,
        geometry: &geometry::Geometry<W::Id>,
    ) -> HashMap<NodeId, Rectangle<f64, Logical>> {
        let mut boxes = geometry.ipc_nodes.clone();
        if let Some((id, slot)) = self
            .fullscreen_node()
            .and_then(|id| Some((id, self.fullscreen_tile_slot_rect(id, geometry)?)))
        {
            boxes.insert(id, slot);
        }
        boxes
    }

    /// The arrange that ends a command resize (`container_resize_tiled`,
    /// sway/commands/resize.c:166-170). A nested container arranges its
    /// parent, which lays out every descendant, so a fullscreen container
    /// below it reports its tile slot. A top-level container arranges the
    /// workspace, which under workspace fullscreen arranges only the
    /// fullscreen container, at the output box (sway/tree/arrange.c:310-316):
    /// everything outside it keeps the box it had, and the percent that
    /// implies, until the next full arrange.
    fn arrange_after_resize(&mut self, old: &geometry::Geometry<W::Id>, resized: NodeId) {
        let Some(fullscreen) = self
            .fullscreen_node()
            .filter(|id| self.fullscreen_mode(*id) == Some(FullscreenMode::Workspace))
        else {
            return;
        };
        let Some(parent) = self.nodes.get(&resized).and_then(|node| node.parent) else {
            return;
        };
        if parent != self.root {
            if self.contains_node(parent, fullscreen) {
                self.fullscreen_tile_slot = true;
            }
            let arranged: Vec<_> = self
                .split_under_fullscreen
                .iter()
                .copied()
                .filter(|id| self.contains_node(parent, *id))
                .collect();
            for id in arranged {
                self.split_under_fullscreen.remove(&id);
            }
            return;
        }
        // The workspace arrange puts the fullscreen container back at the
        // output box.
        self.arrange_workspace();
        let excluded = self.split_excluded();
        let unarranged = old
            .ipc_nodes
            .iter()
            .filter(|(node, _)| {
                **node != self.root
                    && !excluded.contains(*node)
                    && !self.contains_node(fullscreen, **node)
                    && !self.unarranged_under_fullscreen.contains_key(*node)
            })
            .map(|(node, rect)| (*node, *rect))
            .collect::<Vec<_>>();
        self.unarranged_under_fullscreen.extend(unarranged);
    }

    /// Sway refuses a command resize that would take either neighbour below
    /// its sane minimum (`container_resize_tiled`, sway/commands/resize.c:113-120).
    fn adjacent_fits_min_sane(
        &self,
        ipc_nodes: &HashMap<NodeId, Rectangle<f64, Logical>>,
        first: NodeId,
        second: NodeId,
        delta: f64,
    ) -> bool {
        let Some(parent) = self.nodes.get(&first).and_then(|node| node.parent) else {
            return false;
        };
        let Some(Node {
            value: TreeNode::Split {
                layout, children, ..
            },
            ..
        }) = self.nodes.get(&parent)
        else {
            return false;
        };
        let Some(available) = children
            .iter()
            .map(|child| ipc_nodes.get(child).map(|rect| axis_extent(*layout, rect)))
            .sum::<Option<f64>>()
        else {
            return false;
        };
        let change = delta * available;
        fits_min_sane(
            ipc_nodes,
            *layout,
            [(first, change), (second, -change.ceil())],
        )
    }

    /// Sway snaps every sibling's fraction to its whole-pixel box before a
    /// command resize (`container_resize_tiled`, sway/commands/resize.c:126-131
    /// and 154-159). Returns the parent and its shares before snapping, or
    /// `None` when a sibling has no box to snap to.
    fn snap_shares_to_pixels(
        &mut self,
        ipc_nodes: &HashMap<NodeId, Rectangle<f64, Logical>>,
        child: NodeId,
    ) -> Option<(NodeId, Vec<f64>)> {
        let parent = self.nodes.get(&child)?.parent?;
        let Some(Node {
            value:
                TreeNode::Split {
                    layout,
                    children,
                    percents,
                    ..
                },
            ..
        }) = self.nodes.get_mut(&parent)
        else {
            return None;
        };
        let extents = children
            .iter()
            .map(|child| ipc_nodes.get(child).map(|rect| axis_extent(*layout, rect)))
            .collect::<Option<Vec<_>>>()?;
        let total: f64 = extents.iter().sum();
        if !total.is_finite() || extents.iter().any(|extent| *extent <= 0.) {
            return None;
        }
        let old = std::mem::replace(
            percents,
            extents.iter().map(|extent| extent / total).collect(),
        );
        Some((parent, old))
    }

    fn restore_shares(&mut self, parent: NodeId, old: Vec<f64>) {
        if let Some(Node {
            value: TreeNode::Split { percents, .. },
            ..
        }) = self.nodes.get_mut(&parent)
        {
            *percents = old;
        }
    }

    fn resize_adjacent_inner(&mut self, first: NodeId, second: NodeId, delta: f64) -> bool {
        if !delta.is_finite() {
            return false;
        }
        let Some(parent) = self.nodes.get(&first).and_then(|node| node.parent) else {
            return false;
        };
        if self.nodes.get(&second).and_then(|node| node.parent) != Some(parent) {
            return false;
        }
        let Some(Node {
            value: TreeNode::Split {
                children, percents, ..
            },
            ..
        }) = self.nodes.get_mut(&parent)
        else {
            return false;
        };
        let Some(first_index) = children.iter().position(|child| *child == first) else {
            return false;
        };
        let Some(second_index) = children.iter().position(|child| *child == second) else {
            return false;
        };
        if first_index.abs_diff(second_index) != 1 {
            return false;
        }
        let (Some(&first_old), Some(&second_old)) =
            (percents.get(first_index), percents.get(second_index))
        else {
            return false;
        };
        let first_percent = first_old + delta;
        let second_percent = second_old - delta;
        if first_percent <= 0. || second_percent <= 0. {
            return false;
        }
        if let Some(percent) = percents.get_mut(first_index) {
            *percent = first_percent;
        }
        if let Some(percent) = percents.get_mut(second_index) {
            *percent = second_percent;
        }
        self.request_window_sizes();
        true
    }

    pub fn toggle_full_width(&mut self) {
        let Some(id) = self.focus else { return };
        let Some(rect) = self.geometry(id) else {
            return;
        };
        self.resize_node_dimension(
            id,
            true,
            SizeChange::AdjustFixed((self.view_size.w - rect.size.w) as i32),
        );
    }

    pub fn set_window_border(
        &mut self,
        window: &W::Id,
        style: swayward_ipc::command::BorderStyle,
        width: Option<u16>,
    ) -> bool {
        let Some(id) = self.node_for_window(window) else {
            return false;
        };
        let Some(tile) = self.tile_mut(id) else {
            return false;
        };
        if tile.set_sway_border(style, width, false).is_err() {
            return false;
        }
        self.request_window_sizes();
        true
    }

    pub fn set_window_width(&mut self, window: Option<&W::Id>, change: SizeChange) -> bool {
        self.resolve_node(window)
            .is_some_and(|id| self.resize_node_dimension(id, true, change))
    }

    pub fn set_window_height(&mut self, window: Option<&W::Id>, change: SizeChange) -> bool {
        self.resolve_node(window)
            .is_some_and(|id| self.resize_node_dimension(id, false, change))
    }

    pub fn set_window_size_sway(
        &mut self,
        window: &W::Id,
        width: Option<SizeChange>,
        height: Option<SizeChange>,
    ) {
        let Some(id) = self.node_for_window(window) else {
            return;
        };
        self.set_node_size_sway(id, width, height);
    }

    pub fn resize_node_edge_command(
        &mut self,
        id: NodeId,
        edge: ResizeEdge,
        change: SizeChange,
    ) -> bool {
        let horizontal = edge.intersects(ResizeEdge::LEFT_RIGHT);
        let layout = if horizontal {
            Layout::SplitH
        } else {
            Layout::SplitV
        };
        let before = edge.intersects(ResizeEdge::LEFT | ResizeEdge::TOP);
        let ipc_nodes = self.resize_boxes(&self.compute_geometry());
        let Some((first, second, _, _, axis_size, _)) =
            self.resize_boundary(&ipc_nodes, id, layout, before)
        else {
            return false;
        };
        // Sway snaps each fraction to `pending.width / child_total_width` and
        // later normalises them (sway/commands/resize.c:126-139,
        // sway/tree/arrange.c:48-52), so a px amount is a share of the
        // children's summed boxes. Under fullscreen that sum includes the
        // fullscreen child's output-sized box and exceeds the parent extent.
        let axis_size = self
            .nodes
            .get(&first)
            .and_then(|node| node.parent)
            .and_then(|parent| match &self.nodes.get(&parent)?.value {
                TreeNode::Split { children, .. } => children
                    .iter()
                    .map(|child| ipc_nodes.get(child).map(|rect| axis_extent(layout, rect)))
                    .sum::<Option<f64>>(),
                TreeNode::Leaf { .. } => None,
            })
            .unwrap_or(axis_size);
        let delta = match change {
            SizeChange::AdjustFixed(value) => f64::from(value) / axis_size.max(1.),
            SizeChange::AdjustProportion(value) => {
                let parent_extent = self
                    .nodes
                    .get(&first)
                    .and_then(|node| node.parent)
                    .and_then(|parent| ipc_nodes.get(&parent))
                    .map(|rect| if horizontal { rect.size.w } else { rect.size.h })
                    .unwrap_or(axis_size);
                (parent_extent * value / 100.).trunc() / axis_size.max(1.)
            }
            SizeChange::SetFixed(_) | SizeChange::SetProportion(_) => return false,
        };
        let changed = self.resize_adjacent(first, second, delta);
        changed && first == id
    }

    pub fn set_node_size_sway(
        &mut self,
        id: NodeId,
        width: Option<SizeChange>,
        height: Option<SizeChange>,
    ) {
        if let Some(change) = width {
            self.resize_node_dimension(id, true, change);
        }
        if let Some(change) = height {
            self.resize_node_dimension(id, false, change);
        }
    }

    pub fn resize_window_edge(
        &mut self,
        window: Option<&W::Id>,
        edge: ResizeEdge,
        change: SizeChange,
    ) -> bool {
        let Some(id) = self.resolve_node(window) else {
            return false;
        };
        self.resize_node_edge_command(id, edge, change)
    }

    pub fn reset_window_height(&mut self, window: Option<&W::Id>) {
        let Some(id) = self.resolve_node(window) else {
            return;
        };
        let Some(parent) = self.nodes.get(&id).and_then(|node| node.parent) else {
            return;
        };
        if let Some(Node {
            value:
                TreeNode::Split {
                    layout: Layout::SplitV,
                    children,
                    percents,
                    ..
                },
            ..
        }) = self.nodes.get_mut(&parent)
        {
            percents.fill(1. / children.len() as f64);
            self.interactive_resize = None;
            self.request_window_sizes();
        }
    }

    pub fn toggle_window_width(&mut self, window: Option<&W::Id>, forwards: bool) {
        self.toggle_preset(window, true, forwards);
    }

    pub fn toggle_window_height(&mut self, window: Option<&W::Id>, forwards: bool) {
        self.toggle_preset(window, false, forwards);
    }

    pub fn interactive_resize_begin(&mut self, window: W::Id, edges: ResizeEdge) -> bool {
        if self.interactive_resize.is_some() {
            return false;
        }
        let Some(id) = self.node_for_window(&window) else {
            return false;
        };
        if self
            .pending_modes
            .get(&id)
            .is_some_and(|mode| mode.fullscreen.is_some() || mode.maximized)
        {
            return false;
        }
        // A corner resizes both axes, each against its own sibling boundary,
        // as sway's seatop_begin_resize_tiling does
        // (`sway/input/seatop_resize_tiling.c:106-127`). An axis with no
        // boundary in that direction is skipped, not fatal.
        let ipc_nodes = self.compute_geometry().ipc_nodes;
        let axes: Vec<_> = [
            (true, edges.intersection(ResizeEdge::LEFT_RIGHT)),
            (false, edges.intersection(ResizeEdge::TOP_BOTTOM)),
        ]
        .into_iter()
        .filter(|(_, edge)| !edge.is_empty())
        .filter_map(|(horizontal, edge)| {
            let layout = if horizontal {
                Layout::SplitH
            } else {
                Layout::SplitV
            };
            let toward_before = edge.intersects(ResizeEdge::LEFT | ResizeEdge::TOP);
            let (first, second, initial_first, initial_second, axis_size, sign) =
                self.resize_boundary(&ipc_nodes, id, layout, toward_before)?;
            Some(ResizeAxis {
                horizontal,
                first,
                second,
                initial_first,
                initial_second,
                axis_size,
                sign,
            })
        })
        .collect();
        if axes.is_empty() {
            return false;
        }
        self.interactive_resize = Some(InteractiveResize {
            window,
            target: id,
            axes,
            data: InteractiveResizeData { edges },
        });
        true
    }

    pub fn interactive_resize_update(
        &mut self,
        window: &W::Id,
        delta: Point<f64, Logical>,
    ) -> bool {
        let Some(resize) = &self.interactive_resize else {
            return false;
        };
        if &resize.window != window {
            return false;
        }
        let axes = resize.axes.clone();
        let old = self.compute_geometry();
        let mut changed = false;
        for axis in axes {
            let moved = if axis.horizontal { delta.x } else { delta.y };
            let amount = moved * axis.sign / axis.axis_size.max(1.);
            let Some((current_first, current_second)) =
                self.sibling_percents(axis.first, axis.second)
            else {
                continue;
            };
            let target_first = axis.initial_first + amount;
            let target_second = axis.initial_second - amount;
            // Each axis stops at its own limit without holding up the other.
            if target_first <= 0. || target_second <= 0. {
                continue;
            }
            let change = target_first - current_first;
            if self.resize_adjacent_inner(axis.first, axis.second, change) {
                debug_assert!((current_second - change - target_second).abs() <= 1e-6);
                changed = true;
            }
        }
        if changed {
            self.animate_geometry_changes(old, None);
        }
        changed
    }

    pub fn interactive_resize_end(&mut self, window: Option<&W::Id>) {
        if window.is_none_or(|window| {
            self.interactive_resize
                .as_ref()
                .is_some_and(|resize| &resize.window == window)
        }) {
            self.interactive_resize = None;
        }
    }

    fn resolve_node(&self, window: Option<&W::Id>) -> Option<NodeId> {
        window
            .and_then(|window| self.node_for_window(window))
            .or(self.focus)
    }

    /// The extent along the preset axis of `id`'s branch in the nearest split that runs along
    /// that axis and has siblings, and of that split itself.
    fn preset_extents(&self, id: NodeId, width: bool) -> Option<(f64, f64)> {
        let wanted = if width {
            Layout::SplitH
        } else {
            Layout::SplitV
        };
        let extent = |rect: &Rectangle<f64, Logical>| {
            if width {
                rect.size.w
            } else {
                rect.size.h
            }
        };
        let mut branch = id;
        let mut parent = self.nodes.get(&id)?.parent;
        while let Some(parent_id) = parent {
            let Node {
                parent: grandparent,
                value: TreeNode::Split {
                    layout, children, ..
                },
            } = self.nodes.get(&parent_id)?
            else {
                return None;
            };
            if *layout == wanted && children.len() > 1 {
                let geometries = self.compute_geometry();
                let current = geometries.ipc_nodes.get(&branch)?;
                let available = geometries.ipc_nodes.get(&parent_id)?;
                return Some((extent(current), extent(available)));
            }
            branch = parent_id;
            parent = *grandparent;
        }
        None
    }

    fn toggle_preset(&mut self, window: Option<&W::Id>, width: bool, forwards: bool) {
        let presets = if width {
            &self.options.layout.preset_column_widths
        } else {
            &self.options.layout.preset_window_heights
        };
        if presets.is_empty() {
            return;
        }
        let Some(id) = self.resolve_node(window) else {
            return;
        };
        let Some((current, available)) = self.preset_extents(id, width) else {
            return;
        };
        let resolved = |preset| match preset {
            PresetSize::Fixed(value) => f64::from(value),
            PresetSize::Proportion(value) => (available * value).trunc(),
        };
        let index = if forwards {
            presets
                .iter()
                .position(|preset| current + 1. < resolved(*preset))
                .unwrap_or(0)
        } else {
            presets
                .iter()
                .rposition(|preset| resolved(*preset) + 1. < current)
                .unwrap_or(presets.len() - 1)
        };
        let Some(&preset) = presets.get(index) else {
            return;
        };
        let change = match preset {
            PresetSize::Fixed(value) => SizeChange::SetFixed(value),
            PresetSize::Proportion(value) => SizeChange::SetProportion(value * 100.),
        };
        if width {
            self.set_window_width(window, change);
        } else {
            self.set_window_height(window, change);
        }
    }

    pub fn resize_node_dimension(&mut self, id: NodeId, width: bool, change: SizeChange) -> bool {
        let wanted = if width {
            Layout::SplitH
        } else {
            Layout::SplitV
        };
        // Sway converts ppt against the nearest ancestor with the axis layout,
        // whatever its child count, else the workspace (resize_set_tiled and
        // resize_adjust_tiled, sway/commands/resize.c:249-270,297-305).
        let ppt_base = {
            let mut ancestor = self.nodes.get(&id).and_then(|node| node.parent);
            while let Some(ancestor_id) = ancestor {
                match self.nodes.get(&ancestor_id) {
                    Some(Node {
                        value: TreeNode::Split { layout, .. },
                        ..
                    }) if *layout == wanted => break,
                    Some(node) => ancestor = node.parent,
                    None => return false,
                }
            }
            ancestor.unwrap_or(self.root)
        };
        let mut branch = id;
        let mut parent = self.nodes.get(&id).and_then(|node| node.parent);
        while let Some(parent_id) = parent {
            let Some(Node {
                parent: grandparent,
                value: TreeNode::Split {
                    layout, children, ..
                },
            }) = self.nodes.get(&parent_id)
            else {
                return false;
            };
            if *layout == wanted && children.len() > 1 {
                let boxes = self.resize_boxes(&self.compute_geometry());
                let extent = |rect: &Rectangle<f64, Logical>| {
                    if width {
                        rect.size.w
                    } else {
                        rect.size.h
                    }
                };
                let Some(parent_extent) = boxes.get(&ppt_base).map(extent) else {
                    return false;
                };
                let child_extent = |child| boxes.get(child).map(extent);
                let Some(current) = child_extent(&branch) else {
                    return false;
                };
                let available = children
                    .iter()
                    .filter_map(child_extent)
                    .sum::<f64>()
                    .max(1.);
                let delta = match change {
                    SizeChange::AdjustFixed(value) => f64::from(value) / available,
                    // Sway converts ppt to an int amount of px, truncating
                    // (resize_adjust_tiled, sway/commands/resize.c:258-277).
                    SizeChange::AdjustProportion(value) => {
                        (parent_extent * value / 100.).trunc() / available
                    }
                    SizeChange::SetFixed(value) => (f64::from(value) - current) / available,
                    SizeChange::SetProportion(value) => {
                        ((parent_extent * value / 100.).trunc() - current) / available
                    }
                };
                // Only `resize grow|shrink` is held to the sane minimum.
                // Sway's `resize set` skips such a change too but still
                // replies success (resize_set_tiled,
                // sway/commands/resize.c:285-339); swayward's `resize set`
                // keeps its earlier behaviour and applies it.
                let adjust = matches!(
                    change,
                    SizeChange::AdjustFixed(_) | SizeChange::AdjustProportion(_)
                );
                let changed = self.resize_across_siblings(parent_id, branch, delta, adjust);
                return changed && branch == id;
            }
            branch = parent_id;
            parent = *grandparent;
        }
        false
    }

    /// Grows or shrinks `target` by taking the change evenly from all its siblings, refusing
    /// if any share would drop to zero or, with `check_min_sane`, any box below sway's sane
    /// minimum (`container_resize_tiled`, sway/commands/resize.c:66-175).
    fn resize_across_siblings(
        &mut self,
        parent: NodeId,
        target: NodeId,
        delta: f64,
        check_min_sane: bool,
    ) -> bool {
        if !delta.is_finite() || self.never_arranged_in_parent(target) {
            return false;
        }
        let old = self.compute_geometry();
        let boxes = self.resize_boxes(&old);
        let Some(Node {
            value:
                TreeNode::Split {
                    layout,
                    children,
                    percents,
                    ..
                },
            ..
        }) = self.nodes.get_mut(&parent)
        else {
            return false;
        };
        let Some(target_index) = children.iter().position(|child| *child == target) else {
            return false;
        };
        let Some(&target_percent) = percents.get(target_index) else {
            return false;
        };
        // Siblings absorb the change; a lone child has none to take it from.
        let Some(siblings) = children.len().checked_sub(1).filter(|count| *count > 0) else {
            return false;
        };
        let compensation = delta / siblings as f64;
        if target_percent + delta <= 0.
            || percents
                .iter()
                .enumerate()
                .any(|(index, percent)| index != target_index && percent - compensation <= 0.)
        {
            return false;
        }
        let Some(available) = children
            .iter()
            .map(|child| boxes.get(child).map(|rect| axis_extent(*layout, rect)))
            .sum::<Option<f64>>()
        else {
            return false;
        };
        let changes = children.iter().enumerate().map(|(index, child)| {
            if index == target_index {
                (*child, delta * available)
            } else {
                (*child, -(compensation * available).ceil())
            }
        });
        if check_min_sane && !fits_min_sane(&boxes, *layout, changes) {
            return false;
        }
        let snapped = self.snap_shares_to_pixels(&boxes, target);
        let Some(Node {
            value: TreeNode::Split { percents, .. },
            ..
        }) = self.nodes.get_mut(&parent)
        else {
            return false;
        };
        if snapped.is_some()
            && percents.iter().enumerate().any(|(index, percent)| {
                let changed = if index == target_index {
                    percent + delta
                } else {
                    percent - compensation
                };
                changed <= 0.
            })
        {
            if let Some((parent, old)) = snapped {
                self.restore_shares(parent, old);
            }
            return false;
        }
        for (index, percent) in percents.iter_mut().enumerate() {
            *percent += if index == target_index {
                delta
            } else {
                -compensation
            };
        }
        self.arrange_after_resize(&old, target);
        self.interactive_resize = None;
        self.request_window_sizes();
        self.animate_geometry_changes(old, None);
        true
    }

    /// Whether `edge` of `window` borders a sibling rather than the
    /// workspace, following sway's `edge_is_external`
    /// (`sway/input/seatop_default.c:39-74`): some ancestor with exactly
    /// the parallel split layout has a sibling on that side. A combined edge
    /// matches no layout in sway, so corners are always external.
    pub fn is_internal_edge(&self, window: &W::Id, edge: ResizeEdge) -> bool {
        let (wanted, before) = if edge == ResizeEdge::LEFT {
            (Layout::SplitH, true)
        } else if edge == ResizeEdge::RIGHT {
            (Layout::SplitH, false)
        } else if edge == ResizeEdge::TOP {
            (Layout::SplitV, true)
        } else if edge == ResizeEdge::BOTTOM {
            (Layout::SplitV, false)
        } else {
            return false;
        };
        let Some(mut id) = self.node_for_window(window) else {
            return false;
        };
        while let Some(parent) = self.nodes.get(&id).and_then(|node| node.parent) {
            if let Some(Node {
                value: TreeNode::Split {
                    layout, children, ..
                },
                ..
            }) = self.nodes.get(&parent)
            {
                if *layout == wanted {
                    if let Some(index) = children.iter().position(|child| *child == id) {
                        if (before && index > 0) || (!before && index + 1 < children.len()) {
                            return true;
                        }
                    }
                }
            }
            id = parent;
        }
        false
    }

    /// `ipc_nodes` is the IPC rect map of a geometry computed for the current tree.
    fn resize_boundary(
        &self,
        ipc_nodes: &HashMap<NodeId, Rectangle<f64, Logical>>,
        id: NodeId,
        layout: Layout,
        toward_before: bool,
    ) -> Option<(NodeId, NodeId, f64, f64, f64, f64)> {
        let mut branch = id;
        let mut parent = self.nodes.get(&id)?.parent;
        while let Some(parent_id) = parent {
            let node = self.nodes.get(&parent_id)?;
            let TreeNode::Split {
                layout: parent_layout,
                children,
                percents,
                ..
            } = &node.value
            else {
                return None;
            };
            // Only a split of exactly the resized orientation has a boundary
            // to move: tabs and stacks share one box, so their siblings are
            // not neighbours (`sway/commands/resize.c:45-64`).
            if *parent_layout == layout {
                let index = children.iter().position(|child| *child == branch)?;
                let neighbor_index = if toward_before {
                    index.checked_sub(1)
                } else {
                    Some(index + 1).filter(|index| *index < children.len())
                };
                if let Some(&neighbor) = neighbor_index.and_then(|index| children.get(index)) {
                    let axis_size = ipc_nodes.get(&parent_id).map(|rect| {
                        let extent = if layout == Layout::SplitH {
                            rect.size.w
                        } else {
                            rect.size.h
                        };
                        extent - self.gaps * children.len().saturating_sub(1) as f64
                    })?;
                    let first = branch;
                    let second = neighbor;
                    let first_index = children.iter().position(|child| *child == first)?;
                    let second_index = children.iter().position(|child| *child == second)?;
                    let drag_sign = if toward_before { -1. } else { 1. };
                    return Some((
                        first,
                        second,
                        *percents.get(first_index)?,
                        *percents.get(second_index)?,
                        axis_size,
                        drag_sign,
                    ));
                }
            }
            branch = parent_id;
            parent = node.parent;
        }
        None
    }
}

fn axis_extent(layout: Layout, rect: &Rectangle<f64, Logical>) -> f64 {
    if layout == Layout::SplitV {
        rect.size.h
    } else {
        rect.size.w
    }
}

/// Whether every container keeps sway's sane minimum after its change in px.
/// Sway compares the boxes it last arranged, so a view mapped or moved under a
/// fullscreen container still has a zero box and refuses any resize that takes
/// from it (`container_resize_tiled`, sway/commands/resize.c:113-120, 141-148;
/// `arrange_workspace`, sway/tree/arrange.c:310-316).
fn fits_min_sane(
    ipc_nodes: &HashMap<NodeId, Rectangle<f64, Logical>>,
    layout: Layout,
    changes: impl IntoIterator<Item = (NodeId, f64)>,
) -> bool {
    let minimum = if layout == Layout::SplitV {
        geometry::MIN_SANE_H
    } else {
        geometry::MIN_SANE_W
    };
    changes.into_iter().all(|(id, change)| {
        ipc_nodes
            .get(&id)
            .is_some_and(|rect| axis_extent(layout, rect) + change >= minimum)
    })
}
