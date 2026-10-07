use super::*;

impl<W: LayoutElement> TilingTree<W> {
    pub fn focus(&self) -> Option<NodeId> {
        self.focus
    }

    /// Each container paired with the window whose focus last entered it.
    pub fn last_entered_windows(&self) -> impl Iterator<Item = (NodeId, &W::Id)> + '_ {
        self.last_entered_by
            .iter()
            .filter_map(|(node, leaf)| Some((*node, self.tile(*leaf)?.window().id())))
    }

    pub fn focus_rank_for_window(&self, window: &W::Id) -> Option<usize> {
        let node = self.node_for_window(window)?;
        self.focus_history
            .iter()
            .position(|candidate| *candidate == node)
    }

    pub(crate) fn parent_of_window(&self, window: &W::Id) -> Option<NodeId> {
        self.nodes.get(&self.node_for_window(window)?)?.parent
    }

    pub fn parent_of_node(&self, id: NodeId) -> Option<NodeId> {
        self.nodes.get(&id)?.parent
    }

    pub fn non_root_parent_for_window(&self, window: &W::Id) -> Option<NodeId> {
        self.parent_of_window(window)
            .filter(|parent| *parent != self.root && self.split_len(*parent).is_some_and(|n| n > 1))
    }

    pub fn restore_focus_rank(&mut self, window: &W::Id, rank: usize) {
        let Some(node) = self.node_for_window(window) else {
            return;
        };
        self.focus_history.retain(|candidate| *candidate != node);
        self.focus_history
            .insert(rank.min(self.focus_history.len()), node);
    }

    pub(super) fn window_focus_history(&self) -> Vec<W::Id> {
        self.focus_history
            .iter()
            .filter_map(|node| self.tile(*node).map(|tile| tile.window().id().clone()))
            .collect()
    }

    pub(super) fn restore_window_focus_history(&mut self, history: Vec<W::Id>) {
        self.focus_history = history
            .into_iter()
            .filter_map(|window| self.node_for_window(&window))
            .collect();
    }

    /// Ranks a window that arrived without being activated by when it last had focus. Sway keeps
    /// one seat-wide focus stack, so a moved view that was focused more recently than the
    /// destination's focus-inactive container becomes the destination's focus-inactive
    /// container, and the next move to that workspace lands beside it
    /// (`seat_get_focus_inactive_tiling`, sway/input/seat.c:1374-1389; sway/commands/move.c:515).
    pub(crate) fn rank_arrived_window_by_focus_timestamp(&mut self, window: &W::Id) {
        let Some(leaf) = self.node_for_window(window) else {
            return;
        };
        let Some(stamp) = self
            .tile(leaf)
            .and_then(|tile| tile.window().focus_timestamp())
        else {
            return;
        };
        self.focus_history.retain(|candidate| *candidate != leaf);
        let rank = self
            .focus_history
            .iter()
            .position(|candidate| {
                self.tile(*candidate)
                    .is_some_and(|tile| tile.window().focus_timestamp() < Some(stamp))
            })
            .unwrap_or(self.focus_history.len());
        self.focus_history.insert(rank, leaf);
        if rank == 0 {
            // The view heads the stack, but its new parent is not raised: the
            // move attaches it without focusing it (`container_add_child`,
            // sway/tree/container.c:1426-1438).
            self.focus = Some(leaf);
            self.ipc_focus_follows_history = false;
        }
    }

    pub(crate) fn sort_focus_history_by_timestamp(&mut self) {
        let mut history = self
            .focus_history
            .iter()
            .filter_map(|node| {
                self.tile(*node)
                    .map(|tile| (*node, tile.window().focus_timestamp()))
            })
            .collect::<Vec<_>>();
        history.sort_by_key(|(_, timestamp)| std::cmp::Reverse(*timestamp));
        self.focus_history = history.into_iter().map(|(node, _)| node).collect();
    }

    /// The most recent focus entry strictly inside `node`, like sway's
    /// `seat_get_focus_inactive` (sway/input/seat.c:1357-1372).
    pub fn focus_inactive_in(&self, node: NodeId) -> Option<NodeId> {
        self.focus_inactive_in_matching(node, |_| true)
    }

    /// The workspace's focus-inactive tiling container, sway's
    /// `seat_get_focus_inactive_tiling` (sway/input/seat.c:1374-1389).
    ///
    /// Sway's focus stack is seat-wide, but this tree's history ranks views
    /// that arrived by a move or swap behind its own. A view focused more
    /// recently than the head of the history, and outside it, is therefore
    /// above it on sway's stack.
    pub fn focus_inactive_tiling(&self) -> Option<NodeId> {
        let head = self
            .focus_history
            .iter()
            .copied()
            .find(|candidate| *candidate != self.root && self.nodes.contains_key(candidate))?;
        let newest_in = |node: NodeId| {
            self.leaf_ids_in(node)
                .into_iter()
                .filter_map(|leaf| Some((leaf, self.tile(leaf)?.window().focus_timestamp()?)))
                .max_by_key(|(_, stamp)| *stamp)
        };
        let head_stamp = newest_in(head).map(|(_, stamp)| stamp);
        match newest_in(self.root) {
            Some((leaf, stamp)) if Some(stamp) > head_stamp && !self.contains_node(head, leaf) => {
                Some(leaf)
            }
            _ => Some(head),
        }
    }

    /// As `focus_inactive_in`, ignoring `excluded` and everything inside it: sway runs this
    /// after detaching the moved container, so its entries are no longer under `node`.
    pub fn focus_inactive_in_excluding(&self, node: NodeId, excluded: NodeId) -> Option<NodeId> {
        self.focus_inactive_in_matching(node, |candidate| !self.contains_node(excluded, candidate))
    }

    fn focus_inactive_in_matching(
        &self,
        node: NodeId,
        keep: impl Fn(NodeId) -> bool,
    ) -> Option<NodeId> {
        self.focus_history.iter().copied().find(|candidate| {
            *candidate != node
                && self.nodes.contains_key(candidate)
                && self.contains_node(node, *candidate)
                && keep(*candidate)
        })
    }

    /// Picks sway's refocus target for a container leaving `old_parent`: the most recent entry
    /// under the old parent, else under the workspace (sway/commands/move.c:598-608). `moved`
    /// names the container when it is still attached, so its own entries are skipped. Returns
    /// the target with its ancestors so `resolve_transfer_focus` can follow a reaped target.
    pub(super) fn transfer_focus_target(
        &self,
        moved: Option<NodeId>,
        old_parent: Option<NodeId>,
    ) -> Option<(NodeId, Vec<NodeId>)> {
        let search = |node: NodeId| match moved {
            Some(moved) => self.focus_inactive_in_excluding(node, moved),
            None => self.focus_inactive_in(node),
        };
        let target = old_parent
            .filter(|parent| *parent != self.root)
            .and_then(search)
            .or_else(|| search(self.root))?;
        let mut ancestors = Vec::new();
        let mut parent = self.nodes.get(&target).and_then(|node| node.parent);
        while let Some(ancestor) = parent {
            ancestors.push(ancestor);
            parent = self.nodes.get(&ancestor).and_then(|node| node.parent);
        }
        Some((target, ancestors))
    }

    /// Focuses a target from `transfer_focus_target` after the move reaped empty containers.
    /// When the target itself was reaped, sway's destroy handler focuses the most recent view
    /// under its nearest surviving ancestor (sway/input/seat.c:273-286).
    pub(super) fn resolve_transfer_focus(&mut self, target: Option<(NodeId, Vec<NodeId>)>) -> bool {
        let Some((target, ancestors)) = target else {
            return false;
        };
        let focus = if self.nodes.contains_key(&target) {
            Some(target)
        } else {
            ancestors
                .into_iter()
                .find(|ancestor| self.nodes.contains_key(ancestor))
                .and_then(|ancestor| self.focused_leaf_in(ancestor))
        };
        if focus.is_some() {
            self.set_focus_id(focus);
        }
        focus.is_some()
    }

    /// Sway raises a split's new wrapper just below its focused child (`container_split`,
    /// sway/tree/container.c:1554-1560).
    pub(super) fn raise_split_wrapper(&mut self, child: NodeId, wrapper: NodeId) {
        if self.focus != Some(child) {
            return;
        }
        self.focus_history.retain(|candidate| *candidate != wrapper);
        let index = self
            .focus_history
            .iter()
            .position(|candidate| *candidate == child)
            .map_or(0, |index| index + 1);
        self.focus_history.insert(index, wrapper);
    }

    pub fn root_is_focused(&self) -> bool {
        self.focus == Some(self.root)
    }

    pub fn focused_leaf_is_only_child_of_resident_root(&self) -> bool {
        let Some(root) = self.resident_root() else {
            return false;
        };
        self.focus.is_some_and(|focus| {
            self.tile(focus).is_some()
                && self.nodes.get(&focus).and_then(|node| node.parent) == Some(root)
                && self.split_len(root) == Some(1)
        })
    }

    pub fn is_root(&self, id: NodeId) -> bool {
        id == self.root
    }

    /// Focuses the most recent focus entry under the root, if any; returns whether it did.
    pub fn focus_inactive_below_root(&mut self) -> bool {
        let target = self.focus_inactive_in(self.root);
        self.set_focus_id(target.or(self.focus));
        target.is_some()
    }

    /// Points focus at the most recently focused view without reordering the
    /// focus history, as sway's seat stack is left alone when focus stays on
    /// a container that left the tree (`swap_focus`,
    /// sway/tree/container.c:1766-1798).
    pub fn focus_inactive_view_keeping_history(&mut self) {
        if let Some(leaf) = self.focused_leaf_in(self.root) {
            self.focus = Some(leaf);
        }
    }

    /// Focuses the root without raising it in the focus history.
    pub fn focus_root_keeping_history(&mut self) {
        self.focus = Some(self.root);
    }

    pub fn focus_root(&mut self) {
        self.set_focus_id(Some(self.root));
    }

    pub fn set_focus(&mut self, id: NodeId) {
        if self.nodes.contains_key(&id) {
            self.set_focus_id(Some(id));
        }
    }

    /// Re-raise the focused node and its ancestors when an ancestor is a
    /// fresh wrapper. Sway runs a mapped view's criteria before it focuses
    /// the view (`view_map`, sway/tree/view.c:943-956), so a container a
    /// `for_window` command wrapped the view in is raised with it.
    pub fn raise_focus_into_fresh_wrappers(&mut self, window: &W::Id) {
        let Some(focus) = self
            .focus
            .filter(|focus| self.node_for_window(window) == Some(*focus))
        else {
            return;
        };
        if self
            .ipc_stale_nodes
            .iter()
            .any(|wrapper| self.contains_node(*wrapper, focus))
        {
            self.set_focus_id(Some(focus));
        }
    }

    pub fn contains(&self, id: NodeId) -> bool {
        self.nodes.contains_key(&id)
    }

    pub fn is_split(&self, id: NodeId) -> bool {
        matches!(
            self.nodes.get(&id).map(|node| &node.value),
            Some(TreeNode::Split { .. })
        )
    }

    pub fn focus_parent(&mut self) -> bool {
        let Some(focus) = self.focus else {
            return false;
        };
        if self.fullscreen_node() == Some(focus) {
            return false;
        }
        let Some(parent) = self.nodes.get(&focus).and_then(|node| node.parent) else {
            return false;
        };
        self.set_focus_id(Some(parent));
        true
    }

    /// Moves focus from `id` down to its most recently focused leaf, as sway's
    /// `seat_set_focus(seat_get_focus_inactive(node))` focuses a view.
    pub fn focus_inactive_leaf_of(&mut self, id: NodeId) {
        if let Some(leaf) = self.focused_leaf_in(id) {
            self.set_focus_id(Some(leaf));
        }
    }

    pub fn focus_child(&mut self) -> bool {
        let Some(focus) = self.focus else {
            return false;
        };
        let child = self.focused_child_in(focus);
        if let Some(child) = child {
            self.set_focus_id(Some(child));
            true
        } else {
            false
        }
    }

    pub fn focus_from_output_direction(&mut self, dir: Direction) -> bool {
        let target = if let Some(fullscreen) = self.fullscreen_node() {
            self.focused_leaf_in(fullscreen)
        } else {
            let Some(TreeNode::Split {
                layout, children, ..
            }) = self.nodes.get(&self.root).map(|node| &node.value)
            else {
                return false;
            };
            let matching_axis = matches!(
                (dir, layout),
                (
                    Direction::Left | Direction::Right,
                    Layout::SplitH | Layout::Tabbed
                ) | (
                    Direction::Up | Direction::Down,
                    Layout::SplitV | Layout::Stacked
                )
            );
            if matching_axis {
                let branch = if dir.is_backwards() {
                    children.last()
                } else {
                    children.first()
                };
                branch.and_then(|branch| self.focused_leaf_in(*branch))
            } else {
                self.focused_leaf_in(self.root)
            }
        };
        self.set_focus_id(target);
        target.is_some()
    }

    pub fn scroll_tab_indicator(&mut self, window: &W::Id, steps: i32) -> Option<W::Id> {
        let id = self.node_for_window(window)?;
        let mut child = id;
        let (parent, children) = loop {
            let parent = self.nodes.get(&child)?.parent?;
            let TreeNode::Split {
                layout, children, ..
            } = &self.nodes.get(&parent)?.value
            else {
                return None;
            };
            if matches!(layout, Layout::Tabbed | Layout::Stacked) {
                break (parent, children.clone());
            }
            child = parent;
        };
        let active = self.focused_child_in(parent)?;
        let index = children.iter().position(|child| *child == active)?;
        let desired = (index as i32 + steps).clamp(0, children.len() as i32 - 1) as usize;
        let focus = children
            .get(desired)
            .and_then(|child| self.focused_leaf_in(*child));
        self.set_focus_id(focus);
        focus.and_then(|focus| self.tile(focus).map(|tile| tile.window().id().clone()))
    }

    /// `focus next|prev [sibling]`, where `allow_wrap` is false while sway still has another
    /// output to try before its wrap candidate (`node_get_in_direction_tiling`,
    /// sway/commands/focus.c:138-224).
    pub fn focus_next_prev_sibling(&mut self, next: bool, allow_wrap: bool) -> bool {
        let Some(focus) = self.focus else {
            return false;
        };
        let Some(parent) = self.nodes.get(&focus).and_then(|node| node.parent) else {
            return false;
        };
        let Some(TreeNode::Split { layout, .. }) = self.nodes.get(&parent).map(|node| &node.value)
        else {
            return false;
        };
        let direction_layout = match layout {
            Layout::SplitH | Layout::Tabbed => Layout::SplitH,
            Layout::SplitV | Layout::Stacked => Layout::SplitV,
        };
        let mut current = focus;
        let mut wrap = None;
        while let Some(parent) = self.nodes.get(&current).and_then(|node| node.parent) {
            // A fullscreen container ends the walk: sway leaves for another output or does
            // nothing (`node_get_in_direction_tiling`, sway/commands/focus.c:143-155).
            if self.fullscreen_mode(current).is_some() {
                return false;
            }
            let Some(TreeNode::Split {
                layout, children, ..
            }) = self.nodes.get(&parent).map(|node| &node.value)
            else {
                return false;
            };
            if Self::layouts_parallel(*layout, direction_layout) {
                let Some(index) = children.iter().position(|child| *child == current) else {
                    return false;
                };
                let target = if next {
                    children.get(index + 1).copied()
                } else {
                    index
                        .checked_sub(1)
                        .and_then(|index| children.get(index).copied())
                };
                if target.is_some() {
                    self.set_focus_id(target);
                    return true;
                }
                if allow_wrap
                    && self.options.layout.focus_wrapping != swayward_config::FocusWrapping::No
                    && children.len() > 1
                    && wrap.is_none()
                {
                    wrap = if next {
                        children.first().copied()
                    } else {
                        children.last().copied()
                    };
                    // Even `sibling` descends into a wrap candidate's focus-inactive view
                    // (sway/commands/focus.c:187-191, 216-220).
                    wrap = wrap.and_then(|wrap| self.focused_leaf_in(wrap));
                    if self.options.layout.focus_wrapping == swayward_config::FocusWrapping::Force {
                        break;
                    }
                }
            }
            current = parent;
        }
        // Even `sibling` descends into a wrap candidate (`seat_get_focus_inactive_view`,
        // sway/commands/focus.c:186-189, 217-221).
        let wrap = wrap.and_then(|id| self.focused_leaf_in(id));
        if wrap.is_some() {
            self.set_focus_id(wrap);
        }
        wrap.is_some()
    }

    pub fn focus_next_or_prev(&mut self, next: bool) -> bool {
        self.next_prev_direction(next)
            .is_some_and(|direction| self.focus_direction(direction))
    }

    /// The direction `focus next|prev` takes from the focused container's parent layout, or
    /// `None` with the workspace itself focused (`get_direction_from_next_prev`,
    /// sway/commands/focus.c:17-58).
    pub fn next_prev_direction(&self, next: bool) -> Option<Direction> {
        let parent = self
            .focus
            .and_then(|focus| self.nodes.get(&focus))
            .and_then(|node| node.parent)?;
        let &TreeNode::Split { layout, .. } = self.nodes.get(&parent).map(|node| &node.value)?
        else {
            return None;
        };
        Some(match (next, layout) {
            (false, Layout::SplitH | Layout::Tabbed) => Direction::Left,
            (true, Layout::SplitH | Layout::Tabbed) => Direction::Right,
            (false, Layout::SplitV | Layout::Stacked) => Direction::Up,
            (true, Layout::SplitV | Layout::Stacked) => Direction::Down,
        })
    }

    pub fn focus_direction(&mut self, dir: Direction) -> bool {
        self.focus_direction_inner(dir, true)
    }

    pub fn focus_direction_without_wrap(&mut self, dir: Direction) -> bool {
        self.focus_direction_inner(dir, false)
    }

    fn focus_direction_inner(&mut self, dir: Direction, allow_wrap: bool) -> bool {
        let next = self.directional_focus_target(dir, allow_wrap);
        self.set_focus_id(next.or(self.focus));
        next.is_some()
    }

    /// The container `focus <direction>` lands on: the neighbour in the nearest ancestor along
    /// that axis, or with wrapping the far end of that ancestor
    /// (`node_get_in_direction_tiling`, sway/commands/focus.c:138-224).
    fn directional_focus_target(&self, dir: Direction, allow_wrap: bool) -> Option<NodeId> {
        let mut current = self.focus?;
        let barrier = self.fullscreen_node();
        let direction_layout = dir.axis();
        let backwards = dir.is_backwards();
        let mut wrap = None;

        while let Some(parent) = self.nodes.get(&current).and_then(|node| node.parent) {
            // The fullscreen container ends the walk before any non-force wrap candidate is
            // used: sway leaves for another output or returns NULL
            // (`node_get_in_direction_tiling`, sway/commands/focus.c:143-155).
            if Some(current) == barrier {
                return None;
            }
            let Some(TreeNode::Split {
                layout, children, ..
            }) = self.nodes.get(&parent).map(|node| &node.value)
            else {
                return None;
            };
            if Self::layouts_parallel(*layout, direction_layout) {
                let index = children.iter().position(|child| *child == current)?;
                let desired = if backwards {
                    index.checked_sub(1)
                } else {
                    children.get(index + 1).map(|_| index + 1)
                };
                if let Some(desired) = desired {
                    return children
                        .get(desired)
                        .and_then(|child| self.focused_leaf_in(*child));
                }
                if allow_wrap
                    && self.options.layout.focus_wrapping != swayward_config::FocusWrapping::No
                    && children.len() > 1
                {
                    let candidate = if backwards {
                        children.last().copied()
                    } else {
                        children.first().copied()
                    };
                    if self.options.layout.focus_wrapping == swayward_config::FocusWrapping::Force {
                        return candidate.and_then(|id| self.focused_leaf_in(id));
                    }
                    if let Some(candidate) = candidate {
                        wrap.get_or_insert(candidate);
                    }
                }
            }
            if Some(parent) == barrier {
                return None;
            }
            current = parent;
        }

        wrap.and_then(|id| self.focused_leaf_in(id))
    }

    pub fn tiles(&self) -> impl Iterator<Item = &Tile<W>> {
        self.iter_depth_first().filter_map(|(_, node)| match node {
            TreeNode::Leaf { tile } => Some(tile.as_ref()),
            TreeNode::Split { .. } => None,
        })
    }

    pub fn tiles_mut(&mut self) -> impl Iterator<Item = &mut Tile<W>> {
        self.nodes
            .values_mut()
            .filter_map(|node| match &mut node.value {
                TreeNode::Leaf { tile } => Some(tile.as_mut()),
                TreeNode::Split { .. } => None,
            })
    }

    pub fn active_window(&self) -> Option<&W> {
        let id = self.focused_leaf_in(self.focus?)?;
        self.tile(id).map(Tile::window)
    }

    pub fn active_window_mut(&mut self) -> Option<&mut W> {
        let id = self.focused_leaf_in(self.focus?)?;
        self.tile_mut(id).map(Tile::window_mut)
    }

    pub fn active_tile(&self) -> Option<&Tile<W>> {
        self.focus.and_then(|id| self.tile(id))
    }

    pub fn active_tile_mut(&mut self) -> Option<&mut Tile<W>> {
        self.focus.and_then(|id| self.tile_mut(id))
    }

    pub fn activate_window(&mut self, window: &W::Id) -> bool {
        let Some(id) = self.node_for_window(window) else {
            return false;
        };
        self.set_focus_id(Some(id));
        true
    }

    pub fn focus_left(&mut self) -> bool {
        self.focus_direction(Direction::Left)
    }

    pub fn focus_right(&mut self) -> bool {
        self.focus_direction(Direction::Right)
    }

    pub fn focus_up(&mut self) -> bool {
        self.focus_direction(Direction::Up)
    }

    pub fn focus_down(&mut self) -> bool {
        self.focus_direction(Direction::Down)
    }

    pub fn focus_first(&mut self) {
        self.set_focus_id(self.first_leaf());
    }

    pub fn focus_last(&mut self) {
        let focus = self
            .iter_depth_first()
            .filter_map(|(id, node)| matches!(node, TreeNode::Leaf { .. }).then_some(id))
            .last();
        self.set_focus_id(focus);
    }

    pub fn focus_window_in_subtree(&mut self, subtree: NodeId, index: usize) {
        let leaf = self.leaf_ids_in(subtree).get(index).copied();
        if let Some(leaf) = leaf {
            self.set_focus_id(Some(leaf));
        }
    }

    pub fn focus_window_in_parent(&mut self, index: u8) {
        let Some(subtree) = self.focus.and_then(|id| self.nodes.get(&id)?.parent) else {
            return;
        };
        self.focus_window_in_subtree(subtree, usize::from(index));
    }

    pub fn focus_root_child(&mut self, index: usize) {
        let branch = self
            .root_children()
            .and_then(|children| children.get(index))
            .copied();
        if let Some(branch) = branch {
            self.set_focus_id(self.first_leaf_in(branch).or(self.focus));
        }
    }

    pub fn focus_first_root_child(&mut self) {
        self.focus_root_child(0);
    }

    pub fn focus_last_root_child(&mut self) {
        if let Some(last) = self
            .root_children()
            .and_then(|children| children.len().checked_sub(1))
        {
            self.focus_root_child(last);
        }
    }

    pub fn focus_top(&mut self) {
        self.focus_extreme(false);
    }

    pub fn focus_bottom(&mut self) {
        self.focus_extreme(true);
    }

    pub fn focus_up_or_left(&mut self) {
        if !self.focus_up() {
            self.focus_left();
        }
    }

    pub fn focus_up_or_right(&mut self) {
        if !self.focus_up() {
            self.focus_right();
        }
    }

    pub fn focus_down_or_left(&mut self) {
        if !self.focus_down() {
            self.focus_left();
        }
    }

    pub fn focus_down_or_right(&mut self) {
        if !self.focus_down() {
            self.focus_right();
        }
    }

    fn focus_extreme(&mut self, bottom: bool) {
        // Hidden tabs share the shown tab's box, so only visible leaves compete. Ties go to the
        // first in tree order, not to HashMap order.
        let geometries = self.compute_geometry();
        let visible = self.visible_leaves();
        let edge = |rect: &Rectangle<f64, Logical>| {
            if bottom {
                -(rect.loc.y + rect.size.h)
            } else {
                rect.loc.y
            }
        };
        let focus = self
            .iter_depth_first()
            .filter(|(id, _)| visible.contains(id))
            .filter_map(|(id, _)| Some((id, edge(geometries.leaf_boxes.get(&id)?))))
            .min_by(|(_, a), (_, b)| a.total_cmp(b))
            .map(|(id, _)| id)
            .or(self.focus);
        self.set_focus_id(focus);
    }

    pub(super) fn set_focus_id(&mut self, focus: Option<NodeId>) {
        self.focus = focus;
        self.ipc_focus_follows_history = false;
        if let Some(id) = focus {
            // Sway raises every ancestor, outermost last, before the focused node itself, so the
            // node's parents become focus-inactive entries (`seat_set_workspace_focus`,
            // sway/input/seat.c:1178-1190).
            let mut ancestors = Vec::new();
            let mut parent = self.nodes.get(&id).and_then(|node| node.parent);
            while let Some(ancestor) = parent.filter(|ancestor| *ancestor != self.root) {
                ancestors.push(ancestor);
                parent = self.nodes.get(&ancestor).and_then(|node| node.parent);
            }
            self.focus_history
                .retain(|candidate| *candidate != id && !ancestors.contains(candidate));
            for ancestor in ancestors {
                self.focus_history.insert(0, ancestor);
                if self.tile(id).is_some() {
                    self.last_entered_by.insert(ancestor, id);
                }
            }
            self.focus_history.insert(0, id);
            let mut stale = std::mem::take(&mut self.ipc_stale_nodes);
            stale.retain(|candidate| !self.contains_node(*candidate, id));
            self.ipc_stale_nodes = stale;
        }
    }

    /// The child a tabbed or stacked `parent` shows: the one holding focus,
    /// else the one its focus last visited. Sway arranges the container's
    /// focused-inactive child (sway/desktop/transaction.c:468-470) and
    /// disables the rest (:316-321).
    pub(super) fn shown_child_in(&self, parent: NodeId) -> Option<NodeId> {
        let TreeNode::Split { children, .. } = &self.nodes.get(&parent)?.value else {
            return None;
        };
        self.focus
            .and_then(|focus| {
                children
                    .iter()
                    .copied()
                    .find(|child| self.contains_node(*child, focus))
            })
            .filter(|child| !self.ipc_stale_nodes.contains(child))
            .or_else(|| self.focused_child_in(parent))
    }

    pub(super) fn focused_child_in(&self, parent: NodeId) -> Option<NodeId> {
        let TreeNode::Split { children, .. } = &self.nodes.get(&parent)?.value else {
            return None;
        };
        self.children_in_focus_order(children, |entry| self.ipc_stale_nodes.contains(&entry))
            .first()
            .copied()
    }

    /// `children` most recent first, the order of sway's `focus` list and
    /// `seat_get_active_tiling_child` (sway/ipc-json.c:786-807,
    /// sway/input/seat.c:1408-1429). Sway ranks a child by its own seat
    /// stack entry. Focusing a view raises its ancestors with it, so that
    /// entry is usually its newest descendant's, but a directional move
    /// raises nothing (sway/commands/move.c:672-745): a view moved into a
    /// split leaves the split's entry where it was. A child with no entry
    /// of its own ranks by its newest descendant, and never-focused children
    /// follow in tree order. `skip` names nodes whose entries do not count:
    /// a wrapper the tree created without focusing it (`ipc_stale_nodes`)
    /// joined the tail of sway's stack (`seat_node_from_node`,
    /// sway/input/seat.c:327-349), so it ranks there even when a moved view
    /// now sits inside it.
    pub(super) fn children_in_focus_order(
        &self,
        children: &[NodeId],
        skip: impl Fn(NodeId) -> bool,
    ) -> Vec<NodeId> {
        let entries = self
            .focus_history
            .iter()
            .copied()
            .filter(|entry| !skip(*entry))
            .collect::<Vec<_>>();
        let rank = |child: NodeId| {
            if skip(child) {
                return None;
            }
            entries
                .iter()
                .position(|entry| *entry == child)
                .or_else(|| {
                    entries
                        .iter()
                        .position(|entry| self.contains_node(child, *entry))
                })
        };
        let mut ranked = children
            .iter()
            .copied()
            .enumerate()
            .map(|(index, child)| (rank(child).unwrap_or(usize::MAX), index, child))
            .collect::<Vec<_>>();
        ranked.sort_unstable();
        ranked.into_iter().map(|(_, _, child)| child).collect()
    }

    pub(super) fn focused_leaf_in(&self, id: NodeId) -> Option<NodeId> {
        self.focus_history
            .iter()
            .copied()
            .find(|candidate| self.tile(*candidate).is_some() && self.contains_node(id, *candidate))
            .or_else(|| self.first_leaf_in(id))
    }
}
