use super::*;

impl<W: LayoutElement> FloatingLayout<W> {
    pub fn interactive_resize_begin(&mut self, window: W::Id, edges: ResizeEdge) -> bool {
        if let Some((idx, node)) = self.tree_entry_for_window(&window) {
            // A view inside a group is not floating, so its border drag is a tiled resize
            // among its siblings (sway/input/seatop_default.c:396-411,
            // sway/input/seatop_resize_tiling.c:106-149). A view that is itself the group
            // root is the floating container; `Layout::interactive_resize_begin` sends it to
            // `tree_root_resize_begin` with the output layout box.
            if self.tree_entries[idx].root == node {
                return false;
            }
            return self.tree_entries[idx]
                .tree
                .interactive_resize_begin(window, edges);
        }

        if self.interactive_resize.is_some() {
            return false;
        }

        let Some(tile) = self
            .entries
            .iter_mut()
            .map(|entry| &mut entry.tile)
            .find(|tile| tile.window().id() == &window)
        else {
            return false;
        };

        let original_window_size = tile.window_size();

        let resize = InteractiveResize {
            window,
            original_window_size,
            data: InteractiveResizeData { edges },
        };
        self.interactive_resize = Some(resize);

        true
    }

    /// Starts resizing the floating group holding `window` from `edges`, as sway's
    /// modifier resize does on the toplevel ancestor (sway/input/seatop_default.c:476-487).
    pub fn tree_root_resize_begin(
        &mut self,
        window: W::Id,
        edges: ResizeEdge,
        automatic_maximum: Size<f64, Logical>,
    ) -> bool {
        // One pointer holds one grab, so a new resize replaces any stale one.
        let Some(root) = self.tree_root_for_window(&window) else {
            return false;
        };
        let Some(original) = self.tree_rect(root) else {
            return false;
        };
        // An edge of NONE resizes from the bottom-right corner
        // (sway/input/seatop_resize_floating.c:176).
        let edges = if edges.is_empty() {
            ResizeEdge::BOTTOM_RIGHT
        } else {
            edges
        };
        self.tree_resize = Some(TreeResize {
            window,
            root,
            edges,
            original,
            automatic_maximum,
        });
        true
    }

    /// Sway's floating resize motion (sway/input/seatop_resize_floating.c:35-146) on a group
    /// root. A container without a view has no border (`container_create` zeroes it,
    /// sway/tree/container.c:47) and no view constraints, so only the floating limits clamp
    /// it. The resident tree then rearranges the children at their split ratios, as
    /// `arrange_container` does (sway/tree/arrange.c:249-262).
    fn tree_root_resize_update(&mut self, delta: Point<f64, Logical>) -> bool {
        let Some(resize) = &self.tree_resize else {
            return false;
        };
        let Some(current) = self.tree_rect(resize.root) else {
            self.tree_resize = None;
            return false;
        };
        let edges = resize.edges;
        let original = resize.original;
        let root = resize.root;
        let (min, max) = floating_constraints(
            self.options.layout.floating_minimum_size,
            self.options.layout.floating_maximum_size,
            resize.automatic_maximum,
        );

        let mut moved = delta;
        if edges == ResizeEdge::TOP || edges == ResizeEdge::BOTTOM {
            moved.x = 0.;
        }
        if edges == ResizeEdge::LEFT || edges == ResizeEdge::RIGHT {
            moved.y = 0.;
        }
        let grow_w = if edges.contains(ResizeEdge::LEFT) {
            -moved.x
        } else {
            moved.x
        };
        let grow_h = if edges.contains(ResizeEdge::TOP) {
            -moved.y
        } else {
            moved.y
        };
        let width = (original.size.w + grow_w).min(max.w).max(min.w).max(1.);
        let height = (original.size.h + grow_h).min(max.h).max(min.h).max(1.);
        let grow_w = width - original.size.w;
        let grow_h = height - original.size.h;
        let grow_x = if edges.contains(ResizeEdge::LEFT) {
            -grow_w
        } else if edges.contains(ResizeEdge::RIGHT) {
            0.
        } else {
            -grow_w / 2.
        };
        let grow_y = if edges.contains(ResizeEdge::TOP) {
            -grow_h
        } else if edges.contains(ResizeEdge::BOTTOM) {
            0.
        } else {
            -grow_h / 2.
        };

        // Sway applies each change as an int step from the current box
        // (sway/input/seatop_resize_floating.c:128-142).
        let step = |target: f64, current: f64| current + (target - current).trunc();
        let rect = Rectangle::new(
            Point::from((
                step(original.loc.x + grow_x, current.loc.x),
                step(original.loc.y + grow_y, current.loc.y),
            )),
            Size::from((step(width, current.size.w), step(height, current.size.h))),
        );
        if rect != current {
            self.move_tree(root, rect);
        }
        true
    }

    pub fn interactive_resize_update(
        &mut self,
        window: &W::Id,
        delta: Point<f64, Logical>,
    ) -> bool {
        if self
            .tree_resize
            .as_ref()
            .is_some_and(|resize| &resize.window == window)
        {
            return self.tree_root_resize_update(delta);
        }
        if let Some(entry) = self.tree_entry_with_window_mut(window) {
            return entry.tree.interactive_resize_update(window, delta);
        }

        let Some(resize) = &self.interactive_resize else {
            return false;
        };

        if window != &resize.window {
            return false;
        }

        let original_window_size = resize.original_window_size;
        let edges = resize.data.edges;

        if edges.intersects(ResizeEdge::LEFT_RIGHT) {
            let mut dx = delta.x;
            if edges.contains(ResizeEdge::LEFT) {
                dx = -dx;
            };

            let window_width = (original_window_size.w + dx).round() as i32;
            self.set_window_width(
                Some(window),
                SizeChange::SetFixed(window_width),
                false,
                self.view_size.to_i32_round(),
            );
        }

        if edges.intersects(ResizeEdge::TOP_BOTTOM) {
            let mut dy = delta.y;
            if edges.contains(ResizeEdge::TOP) {
                dy = -dy;
            };

            let window_height = (original_window_size.h + dy).round() as i32;
            self.set_window_height(
                Some(window),
                SizeChange::SetFixed(window_height),
                false,
                self.view_size.to_i32_round(),
            );
        }

        true
    }

    pub fn interactive_resize_end(&mut self, window: Option<&W::Id>) {
        if let Some(window) = window {
            if self
                .tree_resize
                .as_ref()
                .is_some_and(|resize| &resize.window == window)
            {
                self.tree_resize = None;
                return;
            }
            if let Some(entry) = self.tree_entry_with_window_mut(window) {
                entry.tree.interactive_resize_end(Some(window));
                return;
            }
        }

        let Some(resize) = &self.interactive_resize else {
            return;
        };

        if let Some(window) = window {
            if window != &resize.window {
                return;
            }
        }

        self.interactive_resize = None;
    }

    pub fn refresh(&mut self, is_active: bool, is_focused: bool) {
        for entry in &mut self.tree_entries {
            entry.tree.refresh_floating(is_active, is_focused);
        }
        let active = self.active_window_id.clone();
        for entry in &mut self.entries {
            let win = entry.tile.window_mut();

            win.set_active_in_column(true);
            win.set_floating(true);
            win.set_untiled(true);

            let mut is_active = is_active && Some(win.id()) == active.as_ref();
            if self.options.deactivate_unfocused_windows {
                is_active &= is_focused;
            }
            win.set_activated(is_active);

            let resize_data = self
                .interactive_resize
                .as_ref()
                .filter(|resize| &resize.window == win.id())
                .map(|resize| resize.data);
            win.set_interactive_resize(resize_data);

            // Sway never sends configure bounds (sway/sway/desktop/xdg_shell.c:305), so a
            // floater's configure carries only its own size. Updating the bounds here on an
            // output rescale would send a configure holding the size from before the client
            // reacted to the new scale, and the client would take that stale size back
            // where sway lets it resize itself (xdg_shell.c:319-331).

            // If transactions are disabled, also disable combined throttling, for more
            // intuitive behavior.
            let intent = if self.options.disable_resize_throttling {
                ConfigureIntent::CanSend
            } else {
                win.configure_intent()
            };

            if matches!(
                intent,
                ConfigureIntent::CanSend | ConfigureIntent::ShouldSend
            ) {
                win.send_pending_configure();
            }

            win.refresh();
        }
    }

    pub fn clamp_within_working_area(
        &self,
        pos: Point<f64, Logical>,
        size: Size<f64, Logical>,
    ) -> Point<f64, Logical> {
        let mut rect = Rectangle::new(pos, size);
        clamp_preferring_top_left_in_area(self.working_area, &mut rect);
        rect.loc
    }

    pub fn scale_by_working_area(&self, pos: Point<f64, SizeFrac>) -> Point<f64, Logical> {
        Data::scale_by_working_area(self.working_area, pos)
    }

    pub fn logical_to_size_frac(&self, logical_pos: Point<f64, Logical>) -> Point<f64, SizeFrac> {
        Data::logical_to_size_frac_in_working_area(self.working_area, logical_pos)
    }

    pub(super) fn move_and_animate(&mut self, idx: usize, new_pos: Point<f64, Logical>) {
        // Moves up to this logical pixel distance are not animated.
        const ANIMATION_THRESHOLD_SQ: f64 = 10. * 10.;

        let entry = &mut self.entries[idx];
        let tile = &mut entry.tile;
        let data = &mut entry.data;

        let prev_pos = data.logical_pos;
        data.set_logical_pos(new_pos);
        let new_pos = data.logical_pos;

        let diff = prev_pos - new_pos;
        if diff.x * diff.x + diff.y * diff.y > ANIMATION_THRESHOLD_SQ {
            tile.animate_move_from(prev_pos - new_pos);
        }
    }

    pub fn new_window_size(
        &self,
        width: Option<PresetSize>,
        height: Option<PresetSize>,
        rules: &ResolvedWindowRules,
    ) -> Size<i32, Logical> {
        let border = self.options.layout.border.merged_with(&rules.border);

        let resolve = |size: Option<PresetSize>, working_area_size: f64| {
            if let Some(size) = size {
                let size = match resolve_preset_size(size, working_area_size) {
                    ResolvedSize::Tile(mut size) => {
                        if !border.off {
                            size -= border.width * 2.;
                        }
                        size
                    }
                    ResolvedSize::Window(size) => size,
                };

                max(1, size.floor() as i32)
            } else {
                0
            }
        };

        let width = resolve(width, self.working_area.size.w);
        let height = resolve(height, self.working_area.size.h);

        Size::from((width, height))
    }

    pub fn remap_stored_tile_pos(
        &self,
        tile: &mut Tile<W>,
        old_area: Option<Rectangle<f64, Logical>>,
    ) {
        let Some(old_area) = old_area.filter(|area| area.size.w > 0. && area.size.h > 0.) else {
            tile.floating_pos = None;
            return;
        };
        let Some(pos) = tile.floating_pos else {
            return;
        };
        let old_rect = Rectangle::new(Data::scale_by_working_area(old_area, pos), tile.tile_size());
        let new_rect = remap_rect_center(old_rect, old_area, self.working_area);
        tile.floating_pos = Some(self.logical_to_size_frac(new_rect.loc));
    }

    pub fn stored_or_default_tile_pos(&self, tile: &Tile<W>) -> Option<Point<f64, Logical>> {
        let pos = tile.floating_pos.map(|pos| self.scale_by_working_area(pos));
        pos.or_else(|| {
            tile.window().rules().default_floating_position.map(|pos| {
                let relative_to = pos.relative_to;
                let size = tile.tile_size();
                let area = self.working_area;

                let mut pos = Point::from((pos.x.0, pos.y.0));
                if relative_to == RelativeTo::TopRight
                    || relative_to == RelativeTo::BottomRight
                    || relative_to == RelativeTo::Right
                {
                    pos.x = area.size.w - size.w - pos.x;
                }
                if relative_to == RelativeTo::BottomLeft
                    || relative_to == RelativeTo::BottomRight
                    || relative_to == RelativeTo::Bottom
                {
                    pos.y = area.size.h - size.h - pos.y;
                }
                if relative_to == RelativeTo::Top || relative_to == RelativeTo::Bottom {
                    pos.x += area.size.w / 2.0 - size.w / 2.0
                }
                if relative_to == RelativeTo::Left || relative_to == RelativeTo::Right {
                    pos.y += area.size.h / 2.0 - size.h / 2.0
                }

                pos + self.working_area.loc
            })
        })
    }

    #[cfg(test)]
    pub fn view_size(&self) -> Size<f64, Logical> {
        self.view_size
    }

    pub fn working_area(&self) -> Rectangle<f64, Logical> {
        self.working_area
    }

    #[cfg(test)]
    pub fn scale(&self) -> f64 {
        self.scale
    }

    #[cfg(test)]
    pub fn clock(&self) -> &Clock {
        &self.clock
    }

    #[cfg(test)]
    pub fn options(&self) -> &Rc<Options> {
        &self.options
    }

    #[cfg(test)]
    pub fn verify_invariants(&self) {
        assert!(self.scale > 0.);
        assert!(self.scale.is_finite());
        for entry in &self.tree_entries {
            assert_eq!(entry.tree.parent_area(), entry.rect);
            assert!(
                !entry.tree.is_empty(),
                "floating tree entry must not be empty"
            );
            assert!(entry.tree.contains(entry.root));
            entry.tree.check_invariants();
        }
        let mut node_ids = std::collections::HashSet::new();
        for entry in &self.tree_entries {
            for (id, _) in entry.tree.iter_depth_first() {
                assert!(
                    node_ids.insert(id),
                    "a node must belong to exactly one floating tree"
                );
            }
        }

        for (i, (tile, data)) in self
            .entries
            .iter()
            .map(|entry| (&entry.tile, &entry.data))
            .enumerate()
        {
            use crate::layout::SizingMode;

            assert!(Rc::ptr_eq(&self.options, &tile.options));
            assert_eq!(self.view_size, tile.view_size());
            assert_eq!(self.clock, tile.clock);
            assert_eq!(self.scale, tile.scale());
            tile.verify_invariants();

            if let Some(idx) = tile.floating_preset_width_idx {
                assert!(idx < self.options.layout.preset_column_widths.len());
            }
            if let Some(idx) = tile.floating_preset_height_idx {
                assert!(idx < self.options.layout.preset_window_heights.len());
            }

            assert_eq!(
                tile.window().pending_sizing_mode(),
                SizingMode::Normal,
                "floating windows cannot be maximized or fullscreen"
            );

            data.verify_invariants();

            let mut data2 = *data;
            data2.update(tile);
            data2.update_config(self.view_size, self.working_area, data.logical_pos);
            assert_eq!(data, &data2, "tile data must be up to date");

            for entry_below in &self.entries[i + 1..] {
                assert_ne!(
                    entry_below.tile.window().id(),
                    tile.window().id(),
                    "a window must belong to exactly one floating entry"
                );
            }
        }

        if let Some(id) = &self.active_window_id {
            assert!(!self.is_empty());
            assert!(self.contains(id), "active window must be present in tiles");
        } else {
            assert!(self.is_empty());
        }

        if let Some(resize) = &self.interactive_resize {
            assert!(
                self.contains(&resize.window),
                "interactive resize window must be present in tiles"
            );
        }
    }
}
