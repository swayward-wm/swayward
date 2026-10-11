use super::*;

impl<W: LayoutElement> TilingTree<W> {
    pub(super) fn working_area(&self) -> Rectangle<f64, Logical> {
        apply_struts(self.parent_area, self.scale, self.options.layout.struts)
    }

    /// Nodes left out of their parent's split: views mapped under fullscreen,
    /// and a fullscreen node that arrived fullscreen. Sway zeroes only the
    /// moved container's width fraction and never re-arranges its new parent
    /// while the workspace is fullscreen, so the siblings keep their boxes.
    pub(super) fn split_excluded(&self) -> std::borrow::Cow<'_, HashSet<NodeId>> {
        let arrived = self
            .fullscreen_arrived
            .then(|| self.fullscreen_node())
            .flatten();
        if arrived.is_none() && self.moved_under_fullscreen.is_empty() {
            return std::borrow::Cow::Borrowed(&self.mapped_under_fullscreen);
        }
        let mut excluded = self.mapped_under_fullscreen.clone();
        excluded.extend(arrived);
        // A global fullscreen view floated out of a split left it empty, and
        // `container_reap_empty` destroyed it (sway/tree/container.c:969-975);
        // with `root->fullscreen_global` cleared the workspace is arranged in
        // full, so the split's singleton ancestors take no share either.
        if let Some(mut id) = arrived
            .filter(|id| self.is_floating_fullscreen(*id) && self.global_fullscreen_orphaned())
        {
            while let Some(parent) = self
                .nodes
                .get(&id)
                .and_then(|node| node.parent)
                .filter(|parent| *parent != self.root && self.split_len(*parent) == Some(1))
            {
                excluded.insert(parent);
                id = parent;
            }
        }
        excluded.extend(self.moved_under_fullscreen.keys().copied());
        std::borrow::Cow::Owned(excluded)
    }

    /// Record that `window` was moved into this tree while it was fullscreen.
    /// `source_rect` is its IPC box in the source tree: sway keeps the moved
    /// container's position and content box, zeroing only its width and height.
    pub fn mark_moved_under_fullscreen(
        &mut self,
        window: &W::Id,
        source_rect: Rectangle<f64, Logical>,
    ) {
        let Some(fullscreen) = self.fullscreen_node() else {
            return;
        };
        if let Some(id) = self
            .node_for_window(window)
            .filter(|id| *id != fullscreen && !self.contains_node(fullscreen, *id))
        {
            self.mapped_under_fullscreen.remove(&id);
            self.moved_under_fullscreen.insert(id, source_rect);
            // `workspace_focus_fullscreen` (sway/commands/move.c:96-110) raises
            // the fullscreen container's focus-inactive view back above the
            // moved one, so the moved view's newer window focus no longer ranks
            // it first in the workspace focus list.
            // Sway's focus stack is seat-wide: the moved view, focused by the
            // move, sits just below the re-raised fullscreen view. A view moved
            // without focus keeps its own place, by when it was last focused
            // (`container_move_to_container` leaves the stack alone,
            // sway/commands/move.c:241-275).
            let stamp = self
                .tile(id)
                .and_then(|tile| tile.window().focus_timestamp());
            let newest = self
                .leaf_ids_in(self.root)
                .into_iter()
                .filter(|leaf| *leaf != id)
                .filter_map(|leaf| self.tile(leaf)?.window().focus_timestamp())
                .max();
            self.focus_history.retain(|candidate| *candidate != id);
            let rank = if stamp > newest {
                0
            } else {
                self.focus_history
                    .iter()
                    .position(|candidate| {
                        self.tile(*candidate)
                            .is_some_and(|tile| tile.window().focus_timestamp() < stamp)
                    })
                    .unwrap_or(self.focus_history.len())
            };
            self.focus_history.insert(rank, id);
            if let Some(leaf) = self.focused_leaf_in(fullscreen) {
                self.focus_history.retain(|candidate| *candidate != leaf);
                self.focus_history.insert(0, leaf);
                self.ipc_focus_follows_history = true;
                // The re-raised view is the workspace's focus-inactive view, so
                // switching back focuses it rather than the hidden arrival
                // (`workspace_switch`, sway/tree/workspace.c:731-743).
                if self
                    .focus
                    .is_none_or(|focus| !self.contains_node(fullscreen, focus))
                {
                    self.focus = Some(leaf);
                }
            }
        }
    }

    /// Sway commits a view mapped under fullscreen only once something marks
    /// it dirty; until then it reports calloc's `border none`. Attaching it
    /// with `workspace_add_tiling` or `container_add_child` marks it dirty
    /// (sway/tree/workspace.c:956-957, sway/tree/container.c:1436-1437), but
    /// the workspace still arranges only the fullscreen container
    /// (sway/tree/arrange.c:310-316). So the view keeps its configured border
    /// over its zero-sized box at the origin, as a moved container does.
    pub(super) fn commit_mapped_under_fullscreen(&mut self, id: NodeId) {
        if !self.mapped_under_fullscreen.remove(&id) {
            return;
        }
        let titlebar = if self
            .tile(id)
            .is_some_and(Tile::has_configured_sway_titlebar)
        {
            self.titlebar_height
        } else {
            0.
        };
        self.moved_under_fullscreen.insert(
            id,
            Rectangle::new(Point::from((0., titlebar)), Size::from((0., 0.))),
        );
    }

    /// A view a directional move brought from another output onto this
    /// workspace, beside its workspace fullscreen container. Like any move
    /// under fullscreen it keeps its position and content box over a zeroed
    /// size (`source_rect` is its old IPC box, relative to this workspace),
    /// but `container_move_to_workspace_from_direction` never calls
    /// `workspace_focus_fullscreen`, so the seat focus stays on it
    /// (sway/commands/move.c:168-196, 277-298, 715-744).
    pub fn keep_directional_arrival_under_fullscreen(
        &mut self,
        window: &W::Id,
        source_rect: Rectangle<f64, Logical>,
        focused: bool,
    ) {
        let Some(fullscreen) = self
            .fullscreen_node()
            .filter(|id| self.fullscreen_mode(*id) == Some(FullscreenMode::Workspace))
        else {
            return;
        };
        let Some(id) = self
            .node_for_window(window)
            .filter(|id| *id != fullscreen && !self.contains_node(fullscreen, *id))
        else {
            return;
        };
        self.mapped_under_fullscreen.remove(&id);
        self.moved_under_fullscreen.insert(id, source_rect);
        if focused {
            self.set_focus_id(Some(id));
        }
    }

    pub fn ipc_focus_follows_history(&self) -> bool {
        self.ipc_focus_follows_history
    }

    /// IPC box of a leaf as last arranged, before any transfer.
    pub fn ipc_rect_for_window(&self, window: &W::Id) -> Option<Rectangle<f64, Logical>> {
        let id = self.node_for_window(window)?;
        self.compute_geometry().leaf_ipc_rects.remove(&id)
    }

    /// The stale boxes that still apply: those of strict ancestors of the
    /// current fullscreen node, and those left unarranged when a view closed
    /// under fullscreen. Sway's next full arrange, when fullscreen ends or
    /// leaves the subtree, gives them their tiled boxes again.
    pub(super) fn active_stale_fullscreen_rects(&self) -> HashMap<NodeId, Rectangle<f64, Logical>> {
        let Some(fullscreen) = self.fullscreen_node() else {
            if self.fullscreen_in_floating || self.unarranged_after_sticky_carry {
                return self.unarranged_under_fullscreen.clone();
            }
            return HashMap::new();
        };
        self.unarranged_under_fullscreen
            .iter()
            .filter(|(id, _)| !self.contains_node(fullscreen, **id))
            .chain(
                self.stale_fullscreen_rects
                    .iter()
                    .filter(|(id, _)| **id != fullscreen && self.contains_node(**id, fullscreen)),
            )
            .map(|(id, rect)| (*id, *rect))
            .collect()
    }

    pub(super) fn compute_geometry(&self) -> geometry::Geometry<W::Id> {
        let excluded = self.split_excluded();
        // A floating group whose global fullscreen a detach orphaned is
        // arranged at its own box (`arrange_floating`,
        // sway/tree/arrange.c:214-219 and 249-262).
        let fullscreen = self
            .fullscreen_node()
            .filter(|_| !(self.resident_root && self.global_fullscreen_orphaned()))
            .into_iter()
            .collect();
        let visible_leaves = self.visible_leaves();
        geometry::compute(geometry::GeometryInput {
            nodes: &self.nodes,
            root: self.root,
            view_size: self.view_size,
            parent_area: self.parent_area,
            scale: self.scale,
            struts: if self.resident_root {
                Default::default()
            } else {
                self.options.layout.struts
            },
            gaps: self.gaps,
            gaps_to_edge: self.gaps_to_edge,
            titlebar_height: self.titlebar_height,
            fullscreen: &fullscreen,
            mapped_under_fullscreen: &excluded,
            stale_fullscreen_rects: &self.active_stale_fullscreen_rects(),
            hide_edge_borders: self.options.layout.hide_edge_borders,
            smart_borders: self.options.layout.smart_borders,
            visible_leaves: &visible_leaves,
            draw_uncovered_top_border: self.options.layout.draw_uncovered_top_border,
            floating_group: self.resident_root,
            // `arrange_container` lays a fullscreen container's children out in its pending
            // box (sway/tree/arrange.c:248-261); a fullscreen view's content goes back to the
            // output box regardless (`view_autoconfigure`, sway/tree/view.c:359-364).
            fullscreen_box: self.fullscreen_pending_box.filter(|_| {
                self.fullscreen_node().is_some_and(|id| {
                    matches!(
                        self.nodes.get(&id).map(|node| &node.value),
                        Some(TreeNode::Split { .. })
                    )
                })
            }),
        })
    }

    pub(super) fn animate_geometry_changes(
        &mut self,
        old: geometry::Geometry<W::Id>,
        skip: Option<NodeId>,
    ) {
        let new = self.compute_geometry();
        for (id, old_rect) in old.leaf_boxes {
            if skip == Some(id) {
                continue;
            }
            let Some(new_rect) = new.leaf_boxes.get(&id) else {
                continue;
            };
            let offset = old_rect.loc - new_rect.loc;
            if offset != Point::default() {
                if let Some(tile) = self.tile_mut(id) {
                    tile.animate_move_from(offset);
                }
            }
        }
    }

    pub(super) fn visible_leaves(&self) -> HashSet<NodeId> {
        if let Some(fullscreen) = self.fullscreen_node() {
            let mut visible = HashSet::new();
            self.collect_visible(fullscreen, &mut visible);
            return visible;
        }
        let mut visible = HashSet::new();
        self.collect_visible(self.root, &mut visible);
        visible
    }

    pub(super) fn collect_visible(&self, id: NodeId, visible: &mut HashSet<NodeId>) {
        let Some(node) = self.nodes.get(&id) else {
            return;
        };
        match &node.value {
            TreeNode::Leaf { .. } => {
                visible.insert(id);
            }
            TreeNode::Split {
                layout, children, ..
            } => {
                if matches!(layout, Layout::Tabbed | Layout::Stacked) {
                    if let Some(child) = self.shown_child_in(id) {
                        self.collect_visible(child, visible);
                    }
                } else {
                    for child in children {
                        self.collect_visible(*child, visible);
                    }
                }
            }
        }
    }

    pub(super) fn sibling_percents(&self, first: NodeId, second: NodeId) -> Option<(f64, f64)> {
        let parent = self.nodes.get(&first)?.parent?;
        if self.nodes.get(&second)?.parent != Some(parent) {
            return None;
        }
        let TreeNode::Split {
            children, percents, ..
        } = &self.nodes.get(&parent)?.value
        else {
            return None;
        };
        let first = children.iter().position(|id| *id == first)?;
        let second = children.iter().position(|id| *id == second)?;
        Some((*percents.get(first)?, *percents.get(second)?))
    }

    pub fn node_geometry(&self, id: NodeId) -> Option<Rectangle<f64, Logical>> {
        self.compute_geometry().ipc_nodes.remove(&id)
    }

    pub(super) fn cancel_resize_for(&mut self, id: NodeId) {
        if self.interactive_resize.as_ref().is_some_and(|resize| {
            resize.target == id
                || resize
                    .axes
                    .iter()
                    .any(|axis| axis.first == id || axis.second == id)
        }) {
            self.interactive_resize = None;
        }
    }

    pub(super) fn request_window_sizes(&mut self) {
        self.request_window_sizes_with(None, false);
    }

    pub(super) fn request_window_sizes_with(
        &mut self,
        transaction: Option<Transaction>,
        animate: bool,
    ) {
        // Under a workspace fullscreen container `arrange_workspace` arranges
        // only that container (sway/tree/arrange.c:310-316), so a wrapper a
        // failed move left unarranged keeps its empty box.
        let workspace_fullscreen = self
            .fullscreen_node()
            .is_some_and(|id| self.fullscreen_mode(id) == Some(FullscreenMode::Workspace));
        if !workspace_fullscreen {
            self.unarranged_wrappers.clear();
        }
        if self.fullscreen_node().is_none() {
            self.wrapper_arranged_boxes.clear();
        }
        self.forget_unarranged_after_sticky_carry();
        // Forget a stale representation once the tree changed, so a mutation that later
        // restores the same shape does not bring it back.
        if self
            .stale_root_representation
            .as_ref()
            .is_some_and(|(_, shape)| *shape != self.representation_shape())
        {
            self.stale_root_representation = None;
        }
        let geometries = self.compute_geometry();
        for (id, node) in &mut self.nodes {
            if let TreeNode::Leaf { tile } = &mut node.value {
                let transaction = transaction.clone();
                if let Some(rect) = geometries.leaf_boxes.get(id) {
                    tile.set_border_edges(
                        geometries
                            .border_edges
                            .get(id)
                            .copied()
                            .unwrap_or_else(ResizeEdge::all),
                    );
                    tile.set_decorated_box(
                        geometries
                            .border_corners
                            .get(id)
                            .copied()
                            .unwrap_or(DecoratedCorners::NONE),
                        geometries.titlebar_attached.contains(id),
                        geometries.titlebar_owned_by_parent.contains(id),
                    );
                    let mode = self.pending_modes.get(id).copied().unwrap_or_default();
                    if mode.fullscreen.is_some() {
                        tile.request_fullscreen(animate, transaction);
                    } else if mode.maximized {
                        tile.request_maximized(self.parent_area.size, animate, transaction);
                    } else {
                        tile.request_tile_size(rect.size, animate, transaction);
                    }
                }
            }
        }
    }
}
