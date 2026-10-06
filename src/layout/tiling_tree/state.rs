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
    pub(super) fn representation_shape(&self) -> TreeShape {
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

    pub(super) fn update_empty_auto_layout(&mut self, view_size: Size<f64, Logical>) {
        let auto = swayward_config::DefaultOrientation::Auto;
        let old_auto_layout = default_layout(auto, self.view_size);
        let new_auto_layout = default_layout(auto, view_size);
        if self.preserved_auto_layout.is_none()
            && self.is_empty()
            && self.options.layout.default_orientation == swayward_config::DefaultOrientation::Auto
            && matches!(
                self.nodes.get(&self.root).map(|node| &node.value),
                Some(TreeNode::Split { layout, .. }) if *layout == old_auto_layout
            )
        {
            // Following the output's axis is not a `layout` command, so it records
            // no `prev_split_layout` for `layout default` (sway/commands/layout.c:171-189).
            self.set_layout_keeping_previous(self.root, new_auto_layout);
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
        self.update_empty_auto_layout(view_size);
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
