use super::*;

impl<W: LayoutElement> TilingTree<W> {
    pub(super) fn alloc(&mut self, node: Node<W>) -> NodeId {
        let id = NodeId(NODE_ID_COUNTER.next());
        self.nodes.insert(id, node);
        id
    }

    pub(super) fn insert_with_id(&mut self, old_id: NodeId, node: Node<W>) -> NodeId {
        if let std::collections::hash_map::Entry::Vacant(entry) = self.nodes.entry(old_id) {
            entry.insert(node);
            old_id
        } else {
            self.alloc(node)
        }
    }

    pub(super) fn remove_node(&mut self, id: NodeId) -> Option<Node<W>> {
        let node = self.nodes.remove(&id)?;
        let was_fullscreen = self.fullscreen_node().is_some();
        for (_, table) in side_tables!(self, &mut) {
            table.forget(id);
        }
        // Destroying the fullscreen container ends fullscreen
        // (`container_begin_destroy`, sway/tree/container.c:480-482), so the
        // next arrange lays out the views that were hidden under it.
        if was_fullscreen && self.fullscreen_node().is_none() {
            self.mapped_under_fullscreen.clear();
            self.moved_under_fullscreen.clear();
            self.unarranged_under_fullscreen.clear();
            self.split_under_fullscreen.clear();
            self.fullscreen_layout_wrappers.clear();
            self.pre_layout_ipc_rects.clear();
        }
        // tab_active also names nodes in its values: a container's shown child.
        self.tab_active.retain(|_, active| *active != id);
        // So does last_entered_by: the leaf whose focus raised a container.
        self.last_entered_by.retain(|_, leaf| *leaf != id);
        Some(node)
    }

    pub(super) fn insert_child(&mut self, parent: NodeId, child: NodeId, after: Option<NodeId>) {
        let index = match self.nodes.get(&parent).map(|node| &node.value) {
            None => return,
            Some(TreeNode::Split { children, .. }) => after
                .and_then(|id| children.iter().position(|child| *child == id))
                .map_or(children.len(), |index| index + 1),
            Some(TreeNode::Leaf { .. }) => return,
        };
        self.insert_child_at(parent, child, index);
    }

    pub(super) fn insert_child_at(&mut self, parent: NodeId, child: NodeId, index: usize) {
        if !self.nodes.contains_key(&child) {
            debug_assert!(false, "inserted child must be present in the arena");
            return;
        }
        let Some(Node {
            value: TreeNode::Split {
                children, percents, ..
            },
            ..
        }) = self.nodes.get_mut(&parent)
        else {
            return;
        };
        // A new child has no fraction; the next arrange gives it the average
        // of the others and divides every fraction by the sum, in child order
        // (`apply_horiz_layout`/`apply_vert_layout`, sway/tree/arrange.c:22-52).
        // The same float operations matter: a `resize` reply compares the
        // fractions bitwise (sway/commands/resize.c:273-277).
        let existing: f64 = percents.iter().sum();
        let percent = if percents.is_empty() || existing <= 0. {
            1.
        } else {
            existing / percents.len() as f64
        };
        children.insert(index.min(children.len()), child);
        percents.insert(index.min(percents.len()), percent);
        let total: f64 = percents.iter().sum();
        if total > 0. {
            for share in percents.iter_mut() {
                *share /= total;
            }
        }
        self.nodes
            .get_mut(&child)
            .expect("invariant: the validated inserted child remains in the arena")
            .parent = Some(parent);
    }

    /// Gives each of `fresh` (children of `parent` whose fraction sway
    /// zeroed) the average share of the other children, then normalizes, as
    /// sway's next arrange does (`apply_horiz_layout`/`apply_vert_layout`,
    /// sway/tree/arrange.c). With no other children they split evenly.
    pub(super) fn share_as_fresh(&mut self, parent: NodeId, fresh: &[NodeId]) {
        let Some(Node {
            value: TreeNode::Split {
                children, percents, ..
            },
            ..
        }) = self.nodes.get_mut(&parent)
        else {
            return;
        };
        let is_fresh = |child: &NodeId| fresh.contains(child);
        let (count, total) = children
            .iter()
            .zip(percents.iter())
            .filter(|(child, _)| !is_fresh(child))
            .fold((0usize, 0.), |(count, total), (_, percent)| {
                (count + 1, total + percent)
            });
        let share = if count == 0 { 1. } else { total / count as f64 };
        for (child, percent) in children.iter().zip(percents.iter_mut()) {
            if is_fresh(child) {
                *percent = share;
            }
        }
        let sum: f64 = percents.iter().sum();
        if sum > 0. {
            for percent in percents.iter_mut() {
                *percent /= sum;
            }
        }
    }

    pub(super) fn set_child_percent(&mut self, parent: NodeId, child: NodeId, percent: f64) {
        let Some(Node {
            value: TreeNode::Split {
                children, percents, ..
            },
            ..
        }) = self.nodes.get_mut(&parent)
        else {
            return;
        };
        if let Some(slot) = children
            .iter()
            .position(|candidate| *candidate == child)
            .and_then(|index| percents.get_mut(index))
        {
            *slot = percent;
        }
    }

    pub(super) fn insert_existing_child(
        &mut self,
        parent: NodeId,
        child: NodeId,
        index: usize,
        split_share_of: NodeId,
    ) {
        if !self.nodes.contains_key(&child) {
            debug_assert!(false, "inserted child must be present in the arena");
            return;
        }
        let Some(Node {
            value: TreeNode::Split {
                children, percents, ..
            },
            ..
        }) = self.nodes.get_mut(&parent)
        else {
            return;
        };
        let Some(target_index) = children.iter().position(|id| *id == split_share_of) else {
            return;
        };
        let Some(target_percent) = percents.get_mut(target_index) else {
            return;
        };
        *target_percent /= 2.;
        let percent = *target_percent;
        let index = index.min(children.len());
        children.insert(index, child);
        percents.insert(index, percent);
        self.nodes
            .get_mut(&child)
            .expect("invariant: the validated inserted child remains in the arena")
            .parent = Some(parent);
    }

    /// Sets a split's layout, carrying its children's shares across an axis
    /// change (see [`Self::sync_fraction_axis`]).
    pub(super) fn change_split_layout(&mut self, id: NodeId, layout: Layout) {
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
            if meta.fraction_axis.is_none() && matches!(*current, Layout::SplitH | Layout::SplitV) {
                meta.fraction_axis = Some(*current);
            }
            *current = layout;
        }
        self.sync_fraction_axis(id);
    }

    /// Sway keeps a width and a height fraction on every container and lays
    /// a split out with the one along its axis; a child with no fraction on
    /// that axis gets the average of its siblings' (`apply_horiz_layout`/
    /// `apply_vert_layout`, sway/tree/arrange.c:15-52 and 100-137). When the
    /// split's linear axis changed, this parks the shares of the old axis and
    /// brings back those of the new one.
    pub(super) fn sync_fraction_axis(&mut self, id: NodeId) {
        let Some(Node {
            value:
                TreeNode::Split {
                    layout,
                    children,
                    percents,
                    meta,
                },
            ..
        }) = self.nodes.get_mut(&id)
        else {
            return;
        };
        if !matches!(*layout, Layout::SplitH | Layout::SplitV) {
            return;
        }
        let axis = *layout;
        if meta.fraction_axis.is_some_and(|old| old != axis) {
            let latent = std::mem::take(&mut meta.latent_shares);
            let mut shares: Vec<f64> = children
                .iter()
                .map(|child| {
                    latent
                        .iter()
                        .find(|(id, _)| id == child)
                        .map_or(0., |(_, share)| *share)
                })
                .collect();
            let (known, total) = shares
                .iter()
                .filter(|share| **share > 0.)
                .fold((0usize, 0.), |(count, sum), share| (count + 1, sum + share));
            let fresh = if known == 0 { 1. } else { total / known as f64 };
            for share in shares.iter_mut().filter(|share| **share <= 0.) {
                *share = fresh;
            }
            let sum: f64 = shares.iter().sum();
            if sum > 0. && sum.is_finite() {
                for share in shares.iter_mut() {
                    *share /= sum;
                }
                meta.latent_shares = children
                    .iter()
                    .copied()
                    .zip(percents.iter().copied())
                    .collect();
                *percents = shares;
            }
        }
        meta.fraction_axis = Some(axis);
    }

    /// Renames `old` to `new` in `parent`'s parked shares: sway's
    /// `container_replace` and `container_split` give the replacement both
    /// fractions (sway/tree/container.c:1485-1490 and 1545-1546).
    pub(super) fn rename_latent_share(&mut self, parent: NodeId, old: NodeId, new: NodeId) {
        if let Some(meta) = self.split_meta_mut(parent) {
            for (id, _) in meta.latent_shares.iter_mut() {
                if *id == old {
                    *id = new;
                }
            }
        }
    }

    pub(super) fn remove_child(&mut self, parent: NodeId, child: NodeId) {
        let Some(Node {
            value:
                TreeNode::Split {
                    children,
                    percents,
                    meta,
                    ..
                },
            ..
        }) = self.nodes.get_mut(&parent)
        else {
            return;
        };
        meta.latent_shares.retain(|(id, _)| *id != child);
        if let Some(index) = children.iter().position(|id| *id == child) {
            children.remove(index);
            percents.remove(index);
            // Rescale against what the siblings actually hold, not against
            // 1 - removed. Accumulated rounding, or a removed share of 1,
            // left the remainder not summing to 1 and tripped the tree
            // invariant. An all-zero remainder falls back to equal shares.
            let total: f64 = percents.iter().sum();
            if total > 0. {
                for percent in percents.iter_mut() {
                    *percent /= total;
                }
            } else if !percents.is_empty() {
                let equal = 1. / percents.len() as f64;
                for percent in percents.iter_mut() {
                    *percent = equal;
                }
            }
        }
    }

    pub(super) fn child_index(&self, parent: NodeId, child: NodeId) -> Option<usize> {
        match &self.nodes.get(&parent)?.value {
            TreeNode::Split { children, .. } => children.iter().position(|id| *id == child),
            TreeNode::Leaf { .. } => None,
        }
    }

    pub(super) fn child_at(&self, parent: NodeId, index: usize) -> Option<NodeId> {
        match &self.nodes.get(&parent)?.value {
            TreeNode::Split { children, .. } => children.get(index).copied(),
            TreeNode::Leaf { .. } => None,
        }
    }

    pub(super) fn split_len(&self, id: NodeId) -> Option<usize> {
        match &self.nodes.get(&id)?.value {
            TreeNode::Split { children, .. } => Some(children.len()),
            TreeNode::Leaf { .. } => None,
        }
    }

    pub(super) fn detach_subtree_only(&mut self, id: NodeId) -> Option<NodeId> {
        let parent = self.nodes.get(&id).and_then(|node| node.parent)?;
        self.remove_child(parent, id);
        self.nodes.get_mut(&id)?.parent = None;
        Some(parent)
    }

    pub(super) fn root_children(&self) -> Option<&[NodeId]> {
        match &self.nodes.get(&self.root)?.value {
            TreeNode::Split { children, .. } => Some(children),
            TreeNode::Leaf { .. } => None,
        }
    }

    pub(super) fn root_branch(&self, mut id: NodeId) -> Option<NodeId> {
        loop {
            let parent = self.nodes.get(&id)?.parent?;
            if parent == self.root {
                return Some(id);
            }
            id = parent;
        }
    }

    pub(super) fn tile(&self, id: NodeId) -> Option<&Tile<W>> {
        match &self.nodes.get(&id)?.value {
            TreeNode::Leaf { tile } => Some(tile),
            TreeNode::Split { .. } => None,
        }
    }

    pub(super) fn tile_mut(&mut self, id: NodeId) -> Option<&mut Tile<W>> {
        match &mut self.nodes.get_mut(&id)?.value {
            TreeNode::Leaf { tile } => Some(tile),
            TreeNode::Split { .. } => None,
        }
    }

    pub(crate) fn node_for_window(&self, window: &W::Id) -> Option<NodeId> {
        self.windows()
            .find_map(|(id, candidate)| (candidate.id() == window).then_some(id))
    }

    pub(super) fn leaf_ids_in(&self, id: NodeId) -> Vec<NodeId> {
        let mut ids = Vec::new();
        self.collect_leaf_ids(id, &mut ids);
        ids
    }

    pub(super) fn collect_leaf_ids(&self, id: NodeId, ids: &mut Vec<NodeId>) {
        let Some(node) = self.nodes.get(&id) else {
            return;
        };
        match &node.value {
            TreeNode::Leaf { .. } => ids.push(id),
            TreeNode::Split { children, .. } => {
                for child in children {
                    self.collect_leaf_ids(*child, ids);
                }
            }
        }
    }

    pub(super) fn first_leaf(&self) -> Option<NodeId> {
        self.first_leaf_in(self.root)
    }

    pub(super) fn first_leaf_in(&self, id: NodeId) -> Option<NodeId> {
        match &self.nodes.get(&id)?.value {
            TreeNode::Leaf { .. } => Some(id),
            TreeNode::Split { children, .. } => {
                children.iter().find_map(|child| self.first_leaf_in(*child))
            }
        }
    }

    pub fn iter_depth_first(&self) -> impl Iterator<Item = (NodeId, &TreeNode<W>)> {
        let mut ids = Vec::new();
        self.collect_depth_first(self.root, &mut ids);
        ids.into_iter()
            .filter_map(|id| self.nodes.get(&id).map(|node| (id, &node.value)))
    }

    pub fn contains_node(&self, ancestor: NodeId, mut id: NodeId) -> bool {
        loop {
            if id == ancestor {
                return true;
            }
            let Some(parent) = self.nodes.get(&id).and_then(|node| node.parent) else {
                return false;
            };
            id = parent;
        }
    }

    fn collect_depth_first(&self, id: NodeId, ids: &mut Vec<NodeId>) {
        let Some(node) = self.nodes.get(&id) else {
            return;
        };
        ids.push(id);
        if let TreeNode::Split { children, .. } = &node.value {
            for child in children {
                self.collect_depth_first(*child, ids);
            }
        }
    }
}
