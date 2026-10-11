use super::*;

/// The xdg toplevel bounds for a tiled window: the strut-reduced working area minus gaps and
/// the window's border, as niri's compute_toplevel_bounds.
pub(super) fn toplevel_bounds(
    options: &Options,
    working_area: Rectangle<f64, Logical>,
    gaps: f64,
    rules: &ResolvedWindowRules,
) -> Size<i32, Logical> {
    let border = options.layout.border.merged_with(&rules.border);
    let padding = gaps * 2. + if border.off { 0. } else { border.width * 2. };
    Size::from((
        (working_area.size.w - padding).max(1.),
        (working_area.size.h - padding).max(1.),
    ))
    .to_i32_floor()
}

/// The layout a new or emptied workspace root takes: the configured orientation, or for `auto`
/// SplitV on a portrait output and SplitH otherwise.
pub(super) fn default_layout(
    orientation: swayward_config::DefaultOrientation,
    view_size: Size<f64, Logical>,
) -> Layout {
    match orientation {
        swayward_config::DefaultOrientation::Horizontal => Layout::SplitH,
        swayward_config::DefaultOrientation::Vertical => Layout::SplitV,
        swayward_config::DefaultOrientation::Auto if view_size.h > view_size.w => Layout::SplitV,
        swayward_config::DefaultOrientation::Auto => Layout::SplitH,
    }
}

impl<W: LayoutElement> TilingTree<W> {
    pub fn new(
        view_size: Size<f64, Logical>,
        parent_area: Rectangle<f64, Logical>,
        gaps_to_edge: bool,
        scale: f64,
        clock: Clock,
        options: Rc<Options>,
    ) -> Self {
        let root = NodeId(NODE_ID_COUNTER.next());
        let root_layout = default_layout(options.layout.default_orientation, view_size);
        let nodes = HashMap::from([(
            root,
            Node {
                parent: None,
                value: TreeNode::Split {
                    layout: root_layout,
                    children: Vec::new(),
                    percents: Vec::new(),
                    meta: SplitMeta::default(),
                },
            },
        )]);
        Self {
            nodes,
            root,
            focus: None,
            ipc_stale_nodes: HashSet::new(),
            last_entered_by: HashMap::new(),
            entered_by_departed: HashMap::new(),
            capped_entry_stamps: HashMap::new(),
            has_had_tile: false,
            empty_representation_layout: None,
            focus_history: Vec::new(),
            pending_modes: HashMap::new(),
            mapped_under_fullscreen: HashSet::new(),
            moved_under_fullscreen: HashMap::new(),
            ipc_focus_follows_history: false,
            fullscreen_tile_slot: false,
            orphaned_global_fullscreen: None,
            fullscreen_arrived: false,
            wrapper_arranged_boxes: HashMap::new(),
            unarranged_wrappers: HashSet::new(),
            fullscreen_rearranged: false,
            fullscreen_layout_wrappers: HashSet::new(),
            pre_layout_ipc_rects: HashMap::new(),
            stale_fullscreen_rects: HashMap::new(),
            unarranged_under_fullscreen: HashMap::new(),
            split_under_fullscreen: HashSet::new(),
            fullscreen_in_floating: false,
            unarranged_after_sticky_carry: false,
            fullscreen_pending_box: None,
            arrange_epoch: 0,
            interactive_resize: None,
            tab_indicators: HashMap::new(),
            titlebars: Default::default(),
            tab_active: HashMap::new(),
            closing_windows: Vec::new(),
            view_size,
            parent_area,
            gaps_to_edge,
            resident_root: false,
            scale,
            titlebar_height: crate::layout::titlebar::height(scale, &options.layout.titlebar),
            clock,
            gaps: options.layout.gaps,
            options,
            preserved_auto_layout: None,
            stale_root_representation: None,
        }
    }

    pub fn from_detached_subtree(
        view_size: Size<f64, Logical>,
        parent_area: Rectangle<f64, Logical>,
        scale: f64,
        clock: Clock,
        options: Rc<Options>,
        subtree: DetachedSubtree<W>,
    ) -> (Self, NodeId, Vec<(NodeId, NodeId)>) {
        let mut tree = Self::new(view_size, parent_area, false, scale, clock, options);
        tree.resident_root = true;
        let focus_history = subtree.focus_history;
        let root_focused = subtree.root_focused;
        let mut remapped = Vec::new();
        let id = tree.insert_detached_node(subtree.node, None, &mut remapped);
        tree.insert_child(tree.root, id, None);
        tree.restore_transferred_focus(focus_history);
        if root_focused {
            tree.set_focus(id);
        }
        tree.has_had_tile = true;
        tree.request_window_sizes();
        (tree, id, remapped)
    }

    pub fn resident_root(&self) -> Option<NodeId> {
        if !self.resident_root {
            return None;
        }
        let Some(TreeNode::Split { children, .. }) =
            self.nodes.get(&self.root).map(|node| &node.value)
        else {
            return None;
        };
        match children.as_slice() {
            [only] => Some(*only),
            _ => None,
        }
    }

    pub fn detach_resident_root(mut self, id: NodeId) -> Option<DetachedSubtree<W>> {
        (self.resident_root()? == id)
            .then(|| self.detach_subtree(id).map(|(subtree, _)| subtree))
            .flatten()
    }

    pub fn is_empty(&self) -> bool {
        self.focus.is_none()
    }

    pub fn has_had_tile(&self) -> bool {
        self.has_had_tile
    }

    /// Restores the flag after a fullscreen floating view passed through the
    /// tiling tree: sway keeps such a view in `ws->floating`, and
    /// `workspace_add_floating` never refreshes the workspace representation
    /// (sway/tree/workspace.c:960-970).
    pub(in crate::layout) fn restore_has_had_tile(&mut self, has_had_tile: bool) {
        self.has_had_tile = has_had_tile;
    }

    /// What sway left unarranged under this tree's fullscreen container: the views mapped or
    /// moved under it and the boxes every other node last had. For a fullscreen container
    /// that leaves the tree for a floating one; pass it to
    /// [`Self::keep_unarranged_for_floating_fullscreen`] once it has left.
    pub(in crate::layout) fn hidden_under_fullscreen(&self) -> HiddenUnderFullscreen {
        let mut boxes = HashMap::new();
        if let Some(fullscreen) = self.fullscreen_node() {
            let excluded = self.split_excluded();
            boxes.extend(
                self.compute_geometry()
                    .ipc_nodes
                    .into_iter()
                    .filter(|(id, _)| {
                        *id != self.root
                            && !excluded.contains(id)
                            && !self.contains_node(fullscreen, *id)
                    }),
            );
            boxes.extend(self.active_stale_fullscreen_rects());
        }
        HiddenUnderFullscreen {
            mapped: self.mapped_under_fullscreen.clone(),
            moved: self.moved_under_fullscreen.clone(),
            boxes,
        }
    }

    /// The workspace stays fullscreen while its fullscreen container moves into a floating
    /// tree, and the arrange that follows reaches only that container
    /// (sway/tree/arrange.c:310-316), so the tiled nodes keep their boxes until the workspace
    /// has no fullscreen container left ([`Self::forget_floating_fullscreen`]).
    pub(in crate::layout) fn keep_unarranged_for_floating_fullscreen(
        &mut self,
        hidden: HiddenUnderFullscreen,
    ) {
        self.restore_hidden_under_fullscreen(hidden);
        self.fullscreen_in_floating = true;
    }

    /// A floating container became the workspace's fullscreen without passing through this
    /// tree. `arrange_root` then reached only that container (sway/commands/fullscreen.c:55,
    /// sway/tree/arrange.c:310-316), so every tiled node keeps the box it had until the
    /// fullscreen ends ([`Self::forget_floating_fullscreen`]).
    pub(in crate::layout) fn enter_floating_fullscreen(&mut self) {
        if self.fullscreen_in_floating || self.fullscreen_node().is_some() {
            return;
        }
        let root = self.root;
        let boxes = self
            .compute_geometry()
            .ipc_nodes
            .into_iter()
            .filter(|(id, _)| *id != root)
            .collect();
        self.restore_hidden_under_fullscreen(HiddenUnderFullscreen {
            mapped: HashSet::new(),
            moved: HashMap::new(),
            boxes,
        });
        self.fullscreen_in_floating = true;
    }

    /// This floating tree's fullscreen container gave the workspace fullscreen to a tiled
    /// one (`container_set_fullscreen`, sway/tree/container.c:1308-1313). The arrange that
    /// follows reaches only the new fullscreen container (sway/tree/arrange.c:310-316), so
    /// every node here keeps the box it had until the workspace fullscreen ends
    /// ([`Self::forget_floating_fullscreen`]).
    pub(in crate::layout) fn yield_fullscreen(&mut self) {
        let Some(fullscreen) = self.fullscreen_node() else {
            return;
        };
        let root = self.root;
        let boxes = self
            .compute_geometry()
            .ipc_nodes
            .into_iter()
            .filter(|(id, _)| *id != root)
            .collect();
        self.set_node_fullscreen(fullscreen, None);
        self.restore_hidden_under_fullscreen(HiddenUnderFullscreen {
            mapped: HashSet::new(),
            moved: HashMap::new(),
            boxes,
        });
        self.fullscreen_in_floating = true;
    }

    /// The floating fullscreen container is returning to this tree (`floating disable`). It
    /// stays fullscreen, and `arrange_workspace` reaches only it
    /// (sway/commands/floating.c:55, sway/tree/arrange.c:310-316), so the hidden state goes
    /// back to the tree's own fullscreen: take it here and pass it to
    /// [`Self::restore_hidden_under_fullscreen`] after the attach.
    pub(in crate::layout) fn take_floating_fullscreen(&mut self) -> Option<HiddenUnderFullscreen> {
        if !std::mem::take(&mut self.fullscreen_in_floating) {
            return None;
        }
        Some(HiddenUnderFullscreen {
            mapped: std::mem::take(&mut self.mapped_under_fullscreen),
            moved: std::mem::take(&mut self.moved_under_fullscreen),
            boxes: std::mem::take(&mut self.unarranged_under_fullscreen),
        })
    }

    /// The sticky fullscreen floater holding this tree's fullscreen was carried to another
    /// workspace (see `unarranged_after_sticky_carry`): the tiled nodes keep the boxes in
    /// `hidden`, taken before it left, and the views mapped or moved under it keep their
    /// never-arranged boxes.
    pub(in crate::layout) fn keep_unarranged_after_sticky_carry(
        &mut self,
        hidden: HiddenUnderFullscreen,
    ) {
        if self.fullscreen_node().is_some() {
            return;
        }
        self.restore_hidden_under_fullscreen(hidden);
        self.unarranged_after_sticky_carry = true;
    }

    pub(in crate::layout) fn restore_hidden_under_fullscreen(
        &mut self,
        hidden: HiddenUnderFullscreen,
    ) {
        let HiddenUnderFullscreen {
            mut mapped,
            mut moved,
            mut boxes,
        } = hidden;
        mapped.retain(|id| self.nodes.contains_key(id));
        moved.retain(|id, _| self.nodes.contains_key(id));
        boxes.retain(|id, _| self.nodes.contains_key(id));
        self.mapped_under_fullscreen = mapped;
        self.moved_under_fullscreen = moved;
        self.unarranged_under_fullscreen = boxes;
    }

    /// The floating fullscreen container that took over this tree's fullscreen ended, and
    /// `cmd_fullscreen` arranged the whole root (sway/commands/fullscreen.c:55), so every
    /// tiled node has its box, border and percent again.
    pub(in crate::layout) fn forget_floating_fullscreen(&mut self) {
        if !std::mem::take(&mut self.fullscreen_in_floating) {
            return;
        }
        self.mapped_under_fullscreen.clear();
        self.moved_under_fullscreen.clear();
        self.unarranged_under_fullscreen.clear();
        self.request_window_sizes();
    }

    pub fn representation_layout(&self) -> Layout {
        let Some(&TreeNode::Split { layout, .. }) =
            self.nodes.get(&self.root).map(|node| &node.value)
        else {
            unreachable!()
        };
        if self.is_empty() {
            self.empty_representation_layout.unwrap_or(layout)
        } else {
            match &self.stale_root_representation {
                Some((stale, shape)) if *shape == self.representation_shape() => *stale,
                _ => layout,
            }
        }
    }

    /// Every node with its split layout, depth first. Sway refreshes a workspace's
    /// representation whenever a child is attached or detached or a layout changes below it
    /// (`container_update_representation`, sway/tree/container.c:750-773), so a different
    /// shape means the cached representation was rebuilt.
    pub(in crate::layout) fn representation_shape(&self) -> TreeShape {
        let mut shape = Vec::new();
        let mut stack = vec![self.root];
        while let Some(id) = stack.pop() {
            match self.nodes.get(&id).map(|node| &node.value) {
                Some(TreeNode::Split {
                    layout, children, ..
                }) => {
                    shape.push((id, Some(*layout)));
                    stack.extend(children.iter().rev());
                }
                Some(TreeNode::Leaf { .. }) => shape.push((id, None)),
                None => {}
            }
        }
        shape
    }

    pub fn reset_empty_layout(&mut self) {
        assert!(self.is_empty());
        let layout = match self.preserved_auto_layout {
            Some(layout) => layout,
            None => default_layout(self.options.layout.default_orientation, self.view_size),
        };
        // Not a `layout` command: records no `prev_split_layout`
        // (sway/commands/layout.c:171-189).
        self.set_layout_keeping_previous(self.root, layout);
        self.empty_representation_layout = Some(layout);
    }

    /// Sets an empty tree's layout without recording a `layout` command.
    pub fn set_empty_layout(&mut self, layout: Layout) {
        if !self.is_empty() {
            return;
        }
        self.set_layout_keeping_previous(self.root, layout);
        self.empty_representation_layout = Some(layout);
    }

    pub fn preserve_empty_auto_layout(&mut self) {
        let Some(&TreeNode::Split { layout, .. }) =
            self.nodes.get(&self.root).map(|node| &node.value)
        else {
            unreachable!();
        };
        self.preserved_auto_layout = Some(layout);
    }

    pub fn track_empty_auto_layout(&mut self) {
        let Some(preserved) = self.preserved_auto_layout.take() else {
            return;
        };
        if self.is_empty()
            && self.options.layout.default_orientation == swayward_config::DefaultOrientation::Auto
            && matches!(
                self.nodes.get(&self.root).map(|node| &node.value),
                Some(TreeNode::Split { layout, .. }) if *layout == preserved
            )
        {
            self.reset_empty_layout();
        }
    }

    pub fn update_config(
        &mut self,
        view_size: Size<f64, Logical>,
        parent_area: Rectangle<f64, Logical>,
        gaps_to_edge: bool,
        scale: f64,
        options: Rc<Options>,
    ) {
        for tile in self.tiles_mut() {
            tile.update_config(view_size, scale, options.clone());
        }
        for indicator in self.tab_indicators.values_mut() {
            indicator.update_config(options.layout.tab_indicator);
        }
        self.view_size = view_size;
        self.parent_area = parent_area;
        self.gaps_to_edge = gaps_to_edge;
        self.scale = scale;
        self.titlebar_height = crate::layout::titlebar::height(scale, &options.layout.titlebar);
        self.gaps = options.layout.gaps;
        self.options = options;
        self.request_window_sizes_with(None, false);
    }

    pub fn update_shaders(&mut self) {
        for tile in self.tiles_mut() {
            tile.update_shaders();
        }
        for indicator in self.tab_indicators.values_mut() {
            indicator.update_shaders();
        }
    }

    pub fn advance_animations(&mut self) {
        for tile in self.tiles_mut() {
            tile.advance_animations();
        }
        for indicator in self.tab_indicators.values_mut() {
            indicator.advance_animations();
        }
        self.closing_windows.retain_mut(|closing| {
            closing.advance_animations();
            closing.are_animations_ongoing()
        });
    }

    pub fn are_animations_ongoing(&self) -> bool {
        self.tiles().any(Tile::are_animations_ongoing)
            || self
                .tab_indicators
                .values()
                .any(TabIndicator::are_animations_ongoing)
            || !self.closing_windows.is_empty()
    }

    pub fn are_transitions_ongoing(&self) -> bool {
        self.tiles().any(Tile::are_transitions_ongoing)
            || self
                .tab_indicators
                .values()
                .any(TabIndicator::are_animations_ongoing)
            || !self.closing_windows.is_empty()
    }

    pub fn view_size(&self) -> Size<f64, Logical> {
        self.view_size
    }

    pub fn parent_area(&self) -> Rectangle<f64, Logical> {
        self.parent_area
    }

    pub fn clock(&self) -> &Clock {
        &self.clock
    }

    pub fn options(&self) -> &Rc<Options> {
        &self.options
    }

    pub fn new_window_toplevel_bounds(&self, rules: &ResolvedWindowRules) -> Size<i32, Logical> {
        toplevel_bounds(&self.options, self.working_area(), self.gaps, rules)
    }

    /// Return a new tiled view's initial size.
    ///
    /// Sway's initial configure carries no size (`handle_commit` schedules a bare configure,
    /// sway/desktop/xdg_shell.c:297-306), so the client maps at a size of its own choosing.
    /// That size is the view's natural size (`handle_map`, xdg_shell.c:481-482), which
    /// floating it later restores (`floating_natural_resize`, sway/tree/container.c:833-847).
    /// The view gets its tiled slot once it is mapped. A height preset from a window rule
    /// still applies.
    pub fn new_window_size(
        &self,
        height: Option<PresetSize>,
        rules: &ResolvedWindowRules,
    ) -> Size<i32, Logical> {
        let height = match height {
            Some(PresetSize::Fixed(value)) => value.max(1),
            Some(PresetSize::Proportion(value)) => {
                let bounds = self.new_window_toplevel_bounds(rules);
                (f64::from(bounds.h) * value).floor().max(1.) as i32
            }
            None => 0,
        };
        Size::from((0, height))
    }
}
