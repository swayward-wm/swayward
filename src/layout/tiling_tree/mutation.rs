use super::*;

impl<W: LayoutElement> TilingTree<W> {
    pub fn add_tile(&mut self, tile: Tile<W>, target: InsertTarget) -> NodeId {
        self.add_tile_with_activation(tile, target, true)
    }

    pub fn add_tile_at_drop(
        &mut self,
        tile: Tile<W>,
        target: NodeId,
        edge: ResizeEdge,
        activate: bool,
    ) -> NodeId {
        let layout = if edge.intersects(ResizeEdge::LEFT | ResizeEdge::RIGHT) {
            Layout::SplitH
        } else {
            Layout::SplitV
        };
        // tiled_drop_target computes the target before the drop completes; a
        // target that has since gone falls back to an ordinary insertion.
        let Some(parent) = self
            .nodes
            .get(&target)
            .map(|node| node.parent.unwrap_or(self.root))
        else {
            return self.add_tile_with_activation(tile, InsertTarget::Focused, activate);
        };
        if !matches!(
            self.nodes.get(&parent).map(|node| &node.value),
            Some(TreeNode::Split { layout: current, .. }) if *current == layout
        ) {
            self.split(target, layout);
        }
        let id = self.add_tile_with_activation(tile, InsertTarget::Node(target), activate);
        if edge.intersects(ResizeEdge::LEFT | ResizeEdge::TOP) {
            let parent = self
                .nodes
                .get(&id)
                .and_then(|node| node.parent)
                .expect("invariant: an inserted tile has a parent");
            let first = self
                .child_index(parent, target)
                .expect("invariant: the drop target remains a child of the insertion parent");
            let second = self
                .child_index(parent, id)
                .expect("invariant: the inserted tile is a child of its parent");
            let TreeNode::Split {
                children, percents, ..
            } = &mut self
                .nodes
                .get_mut(&parent)
                .expect("invariant: every child parent is present in the arena")
                .value
            else {
                unreachable!()
            };
            children.swap(first, second);
            percents.swap(first, second);
        }
        id
    }

    pub fn add_tile_right_of(
        &mut self,
        right_of: &W::Id,
        tile: Tile<W>,
        activate: bool,
    ) -> Option<NodeId> {
        let target = self.node_for_window(right_of)?;
        Some(self.add_tile_with_activation(tile, InsertTarget::Node(target), activate))
    }

    pub fn add_tile_to_subtree(
        &mut self,
        subtree: NodeId,
        tile: Tile<W>,
        activate: bool,
    ) -> Option<NodeId> {
        self.nodes
            .contains_key(&subtree)
            .then(|| self.add_tile_with_activation(tile, InsertTarget::Node(subtree), activate))
    }

    pub fn add_tile_with_activation(
        &mut self,
        mut tile: Tile<W>,
        target: InsertTarget,
        activate: bool,
    ) -> NodeId {
        self.interactive_resize = None;
        self.has_had_tile = true;
        tile.update_config(self.view_size, self.scale, self.options.clone());
        tile.set_sway_csd_floating(false);
        let pending_mode = tile.window().pending_sizing_mode();
        let fullscreen = self.fullscreen_node();
        let previous_focus = self.focus;
        let old_geometries = self.compute_geometry();
        let maps_into_wrapper = !self.fullscreen_layout_wrappers.is_empty();
        if maps_into_wrapper {
            self.pre_layout_ipc_rects.clear();
        }
        let id = self.alloc(Node {
            parent: None,
            value: TreeNode::Leaf {
                tile: Box::new(tile),
            },
        });
        let (parent, after) = self.insertion_slot(target);
        self.insert_child(parent, id, after);
        // Sway never focuses a view mapped while its workspace has a
        // fullscreen container (`should_focus`, `sway/tree/view.c:706-709`),
        // but only a view added directly to the workspace stays unarranged:
        // one added to a container is arranged with its siblings
        // (`arrange_container(parent)`, `sway/tree/view.c:931-940`).
        let focus_blocked = fullscreen.is_some();
        // A global fullscreen container does not set `workspace->fullscreen`
        // (`container_fullscreen_global`, sway/tree/container.c), so
        // `arrange_workspace` lays the new view out with its siblings.
        let mapped_under_fullscreen = parent == self.root
            && fullscreen.is_some_and(|fullscreen| {
                self.fullscreen_mode(fullscreen) == Some(FullscreenMode::Workspace)
            });
        self.forget_unarranged_after_map(parent, mapped_under_fullscreen);
        if fullscreen.is_some_and(|fullscreen| {
            self.divides_box_under_fullscreen(parent, fullscreen)
                || (parent == self.root && !mapped_under_fullscreen)
        }) {
            self.fullscreen_tile_slot = true;
        }
        if parent == self.root {
            if let Some(layout) = match self.options.layout.workspace_layout {
                swayward_config::WorkspaceLayout::Default => None,
                swayward_config::WorkspaceLayout::Stacking => Some(Layout::Stacked),
                swayward_config::WorkspaceLayout::Tabbed => Some(Layout::Tabbed),
            } {
                self.wrap_node(id, layout);
            }
        }
        self.place_new_leaf_in_focus_order(id, activate, focus_blocked, previous_focus);
        if mapped_under_fullscreen && !pending_mode.is_fullscreen() {
            self.mapped_under_fullscreen.insert(id);
            // With no tiling container to map beside, sway attaches the view
            // with `workspace_add_tiling` (sway/tree/view.c:849-901), which
            // commits it. Only a floating fullscreen view leaves that so.
            let floating_fullscreen = fullscreen
                .and_then(|fullscreen| self.tile(fullscreen))
                .is_some_and(|tile| tile.restore_to_floating);
            let no_tiling_sibling = self.root_children().is_some_and(|children| {
                children
                    .iter()
                    .all(|child| *child == id || Some(*child) == fullscreen)
            });
            if floating_fullscreen && no_tiling_sibling {
                self.commit_mapped_under_fullscreen(id);
            }
        }
        if pending_mode.is_maximized() {
            self.pending_modes.insert(
                id,
                PendingMode {
                    maximized: true,
                    ..PendingMode::default()
                },
            );
        }
        self.compact_tree();
        if maps_into_wrapper {
            // The view maps beside the fullscreen container, inside the
            // wrapper, and `arrange_container(parent)` lays the wrapper out at
            // its empty box (sway/tree/view.c:931-940).
            self.arrange_fullscreen_wrappers();
        }
        if pending_mode.is_fullscreen() {
            self.replace_fullscreen_state(id, Some(FullscreenMode::Workspace));
        }
        self.animate_geometry_changes(old_geometries, Some(id));
        self.request_window_sizes();
        id
    }

    /// Drops the boxes left unarranged by a close under fullscreen that the
    /// arrange after mapping a view into `parent` recomputes. A view mapped
    /// into a container arranges that container (`arrange_container(parent)`,
    /// sway/tree/view.c:936-939), which gives its descendants new boxes. A
    /// view mapped onto the workspace arranges the workspace (view.c:938-939),
    /// which under workspace fullscreen arranges only the fullscreen container
    /// (sway/tree/arrange.c:310-316) and otherwise everything.
    fn forget_unarranged_after_map(&mut self, parent: NodeId, mapped_under_fullscreen: bool) {
        if parent != self.root {
            let unarranged = std::mem::take(&mut self.unarranged_under_fullscreen);
            self.unarranged_under_fullscreen = unarranged
                .into_iter()
                .filter(|(id, _)| *id == parent || !self.contains_node(parent, *id))
                .collect();
        } else if !mapped_under_fullscreen {
            self.unarranged_under_fullscreen.clear();
        }
    }

    /// Whether a leaf added to `parent` takes a share of a split box inside `fullscreen`. Only
    /// split layouts divide the parent box; tabbed and stacked children keep the full box
    /// (`apply_tabbed_layout`, sway/tree/arrange.c:163-187).
    fn divides_box_under_fullscreen(&self, parent: NodeId, fullscreen: NodeId) -> bool {
        parent != self.root
            && parent != fullscreen
            && self.contains_node(parent, fullscreen)
            && matches!(
                self.nodes.get(&parent).map(|node| &node.value),
                Some(TreeNode::Split {
                    layout: Layout::SplitH | Layout::SplitV,
                    ..
                })
            )
    }

    /// Focuses a new leaf, or ranks it last behind the kept focus: sway appends every new node
    /// to the tail of the seat's focus stack (`seat_node_from_node`, sway/input/seat.c:349) and
    /// only a focused view moves to the head.
    fn place_new_leaf_in_focus_order(
        &mut self,
        id: NodeId,
        activate: bool,
        focus_blocked: bool,
        previous_focus: Option<NodeId>,
    ) {
        match previous_focus {
            Some(previous_focus) if !activate || focus_blocked => {
                self.focus_history.retain(|candidate| *candidate != id);
                self.focus_history.push(id);
                self.focus = Some(previous_focus);
            }
            _ => self.set_focus_id(Some(id)),
        }
    }

    fn insertion_slot(&self, target: InsertTarget) -> (NodeId, Option<NodeId>) {
        let target = match target {
            // With the workspace focused, sway maps beside its focus-inactive
            // container (`seat_get_focus_inactive(ws)`, sway/tree/view.c:849-882).
            InsertTarget::Focused if self.focus == Some(self.root) => self
                .focus_history
                .iter()
                .copied()
                .find(|id| *id != self.root && self.contains_node(self.root, *id)),
            // A floating fullscreen view is floating in sway, so the new view
            // maps beside the focus-inactive tiling view instead, or onto the
            // workspace when it has none (`seat_get_focus_inactive_tiling`,
            // sway/tree/view.c:851-866).
            InsertTarget::Focused
                if self
                    .focus
                    .is_some_and(|focus| self.is_floating_fullscreen(focus)) =>
            {
                self.focus_inactive_tiling_leaf()
            }
            InsertTarget::Focused => self.focus,
            InsertTarget::Node(id) => Some(id),
            InsertTarget::MoveDestination => {
                if let Some(focus) = self.focus.filter(|focus| self.is_split(*focus)) {
                    return (focus, None);
                }
                self.focus
            }
        };
        let parent = target
            .and_then(|id| self.nodes.get(&id)?.parent)
            .unwrap_or(self.root);
        let after = target
            .filter(|target| self.nodes.get(target).and_then(|node| node.parent) == Some(parent));
        (parent, after)
    }

    /// A fullscreen tile that unfullscreens back to floating: sway keeps it
    /// in the workspace's floating list (`container_set_floating`,
    /// sway/tree/container.c:951-975).
    pub(super) fn is_floating_fullscreen(&self, id: NodeId) -> bool {
        self.fullscreen_mode(id).is_some()
            && self.tile(id).is_some_and(|tile| tile.restore_to_floating)
    }

    /// The most recently focused tiling view, skipping a floating fullscreen
    /// one (`seat_get_focus_inactive_tiling`, sway/input/seat.c:1374-1389).
    fn focus_inactive_tiling_leaf(&self) -> Option<NodeId> {
        self.focus_history.iter().copied().find(|id| {
            self.tile(*id).is_some()
                && !self.is_floating_fullscreen(*id)
                && self.contains_node(self.root, *id)
        })
    }

    pub fn remove_tile_node(&mut self, id: NodeId) -> Option<Tile<W>> {
        let old_geometries = self.compute_geometry();
        if !matches!(
            self.nodes.get(&id).map(|node| &node.value),
            Some(TreeNode::Leaf { .. })
        ) {
            return None;
        }
        let removed_fullscreen = self.fullscreen_node() == Some(id);
        if let Some(fullscreen) = self.fullscreen_node().filter(|_| !removed_fullscreen) {
            // Only the fullscreen container is arranged after the close; the
            // rest keep the boxes they had (sway/tree/view.c:1001-1006,
            // sway/tree/arrange.c:310-316). Views mapped under fullscreen
            // already report their empty box.
            let excluded = self.split_excluded();
            let unarranged = old_geometries
                .ipc_nodes
                .iter()
                .filter(|(node, _)| {
                    **node != self.root
                        && !excluded.contains(*node)
                        && !self.contains_node(fullscreen, **node)
                })
                .map(|(node, rect)| (*node, *rect))
                .collect::<Vec<_>>();
            self.unarranged_under_fullscreen.extend(unarranged);
        }
        let removed_global = removed_fullscreen
            && self.fullscreen_mode(id) == Some(FullscreenMode::Global)
            && self.focus == Some(id);
        let node = self.remove_node(id)?;
        if removed_fullscreen {
            self.mapped_under_fullscreen.clear();
            self.moved_under_fullscreen.clear();
            self.wrapper_arranged_boxes.clear();
        }
        let TreeNode::Leaf { mut tile } = node.value else {
            unreachable!();
        };
        tile.clear_tiled_content_size();
        self.interactive_resize = None;
        if let Some(parent) = node.parent {
            self.remove_child(parent, id);
            self.reap_empty_from(parent);
        }
        if self.windows().next().is_none() {
            let Some(&TreeNode::Split { layout, .. }) =
                self.nodes.get(&self.root).map(|node| &node.value)
            else {
                unreachable!()
            };
            self.empty_representation_layout = Some(layout);
            self.pending_modes.clear();
            self.set_focus_id(None);
        } else if removed_global {
            // Sway emits the destroy signal while the view is still global fullscreen
            // (sway/tree/container.c:488-501), so the seat refuses every sibling it obstructs
            // and focus stays on the workspace (sway/input/seat.c:1148-1151).
            self.set_focus_id(Some(self.root));
        } else if self.focus == Some(id) {
            self.set_focus_id(
                self.fullscreen_node()
                    .and_then(|fullscreen| self.focused_leaf_in(fullscreen))
                    .or_else(|| self.focused_leaf_in(self.root)),
            );
        }
        self.animate_geometry_changes(old_geometries, None);
        Some(*tile)
    }

    /// Removes a window that is being transferred elsewhere rather than closed. Sway refocuses
    /// the most recent focus entry under the old parent, which is a container when that
    /// container was focused on its own (sway/commands/move.c:598-608;
    /// sway/tree/root.c:128-140). Closing uses the view-only rule in `remove_tile`.
    pub fn remove_tile_for_transfer(
        &mut self,
        window: &W::Id,
        transaction: Transaction,
    ) -> Option<Tile<W>> {
        let id = self.node_for_window(window)?;
        let target = (self.focus == Some(id) && self.fullscreen_node().is_none())
            .then(|| {
                self.transfer_focus_target(Some(id), self.nodes.get(&id).and_then(|n| n.parent))
            })
            .flatten();
        let tile = self.remove_tile(window, transaction)?;
        if self.focus.is_some() {
            self.resolve_transfer_focus(target);
        }
        Some(tile)
    }

    pub fn remove_tile(&mut self, window: &W::Id, transaction: Transaction) -> Option<Tile<W>> {
        let id = self.node_for_window(window)?;
        let tile = self.remove_tile_node(id)?;
        self.request_window_sizes_with(Some(transaction), true);
        Some(tile)
    }

    /// As `remove_tile`, but resizes the remaining windows without a transaction or an
    /// animation. Emptied parents are reaped either way.
    pub fn remove_tile_without_transaction(&mut self, window: &W::Id) -> Option<Tile<W>> {
        let id = self.node_for_window(window)?;
        let tile = self.remove_tile_node(id)?;
        self.request_window_sizes();
        Some(tile)
    }

    pub fn add_tile_to_existing_parent(
        &mut self,
        mut tile: Tile<W>,
        parent: NodeId,
        activate: bool,
    ) -> NodeId {
        self.interactive_resize = None;
        tile.update_config(self.view_size, self.scale, self.options.clone());
        tile.set_sway_csd_floating(false);
        let old_geometries = self.compute_geometry();
        let id = self.alloc(Node {
            parent: Some(parent),
            value: TreeNode::Leaf {
                tile: Box::new(tile),
            },
        });
        self.unarranged_under_fullscreen.clear();
        self.insert_child(parent, id, None);
        if activate {
            self.set_focus_id(Some(id));
        }
        self.animate_geometry_changes(old_geometries, Some(id));
        self.request_window_sizes();
        id
    }
}
