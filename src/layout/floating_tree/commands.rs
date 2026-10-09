use super::*;
use crate::layout::tiling_tree::Layout as TreeLayout;

impl<W: LayoutElement> FloatingLayout<W> {
    pub(super) fn add_tile_at(&mut self, mut idx: usize, mut tile: Tile<W>, activate: bool) {
        tile.update_config(self.view_size, self.scale, self.options.clone());
        tile.set_sway_csd_floating(true);
        tile.set_border_edges(ResizeEdge::all());
        tile.set_decorated_box(
            crate::layout::tile::DecoratedCorners::ALL,
            tile.has_sway_titlebar(),
            false,
        );

        // Restore the previous floating window size, and in case the tile is fullscreen,
        // unfullscreen it.
        let floating_size = tile.floating_window_size;
        let win = tile.window_mut();
        let mut size = if !win.pending_sizing_mode().is_normal() {
            // If the window was fullscreen or maximized without a floating size, ask for (0, 0).
            floating_size.unwrap_or_default()
        } else {
            // If the window wasn't fullscreen without a floating size (e.g. it was tiled before),
            // ask for the current size. If the current size is unknown (the window was only ever
            // fullscreen until now), fall back to (0, 0).
            floating_size.unwrap_or_else(|| win.expected_size().unwrap_or_default())
        };

        if win.pending_sizing_mode().is_normal() && size.w > 1 && size.h > 1 {
            size = constrain_floating_size(
                size,
                self.options.layout.floating_minimum_size,
                self.options.layout.floating_maximum_size,
                self.view_size,
                win.min_size(),
                win.max_size(),
            );
        } else {
            let min_size = win.min_size();
            let max_size = win.max_size();
            size.w = ensure_min_max_size_maybe_zero(size.w, min_size.w, max_size.w);
            size.h = ensure_min_max_size_maybe_zero(size.h, min_size.h, max_size.h);
        }

        win.request_size_once(size, true);

        if activate || self.entries.is_empty() {
            self.active_window_id = Some(win.id().clone());
        }

        // Make sure the tile isn't inserted below its parent.
        for (i, tile_above) in self
            .entries
            .iter()
            .map(|entry| &entry.tile)
            .enumerate()
            .take(idx)
        {
            if win.is_child_of(tile_above.window()) {
                idx = i;
                break;
            }
        }

        let pos = if tile.floating_pos.is_some() {
            self.stored_or_default_tile_pos(&tile).unwrap()
        } else if tile.window().pending_sizing_mode().is_normal() {
            // Sway centres the natural size clamped by `floating_minimum_size` and
            // `floating_maximum_size` alone, not by the client's size hints
            // (`floating_natural_resize`, sway/tree/container.c:833-847): a view that
            // maps at 1x1 is centred as 75x50.
            let natural = floating_size.unwrap_or_else(|| tile.window().natural_size());
            let (minimum, maximum) = floating_constraints(
                self.options.layout.floating_minimum_size,
                self.options.layout.floating_maximum_size,
                self.view_size,
            );
            let content = Size::from((
                f64::from(natural.w).min(maximum.w).max(minimum.w),
                f64::from(natural.h).min(maximum.h).max(minimum.h),
            ));
            self.centered_content_tile_pos(&tile, content)
        } else if size.w > 1 && size.h > 1 {
            self.centered_content_tile_pos(&tile, size.to_f64())
        } else {
            let tile_size = size.to_f64() + tile.tile_size() - tile.window_size();
            center_preferring_top_left_in_area(self.working_area, tile_size)
        };

        let data = Data::new(self.view_size, self.working_area, &tile, pos);
        // A new root goes on top; one kept below its parent shares the parent's
        // place relative to groups.
        let stamp = match idx.checked_sub(1).and_then(|above| self.entries.get(above)) {
            Some(above) => above.stamp,
            None => self.bump_stamp(),
        };
        self.insert_entry(
            idx,
            FloatingEntry {
                tile,
                data,
                stamp,
                titlebar: Box::default(),
            },
        );

        self.bring_up_descendants_of(idx);
    }

    /// Sway centers a floating view's content box, not its decorated container, on the
    /// workspace, or on the output when the content is larger than the workspace
    /// (`container_floating_resize_and_center`, sway/tree/container.c:878-893).
    fn centered_content_tile_pos(
        &self,
        tile: &Tile<W>,
        content: Size<f64, Logical>,
    ) -> Point<f64, Logical> {
        let area = if content.w > self.working_area.size.w || content.h > self.working_area.size.h {
            Rectangle::from_size(self.view_size)
        } else {
            self.working_area
        };
        // Sway keeps the half pixel and truncates it when it reports the box
        // (`container_get_box` into an int `wlr_box`, sway/ipc-json.c:241-247); flooring
        // gives the same integers.
        let mut content_loc = area.loc + (area.size.to_point() - content.to_point()).downscale(2.);
        content_loc.x = content_loc.x.floor();
        content_loc.y = content_loc.y.floor();
        // Floating tiles draw all four borders, so left equals bottom.
        let side = tile.tile_width_for_window_width(0.) / 2.;
        let top = tile.tile_height_for_window_height(0.) - side;
        content_loc - Point::from((side, top))
    }

    pub fn add_tile_above(&mut self, above: &W::Id, tile: Tile<W>, activate: bool) {
        let idx = if let Some(idx) = self.idx_of(above) {
            idx
        } else if let Some((idx, _)) = self.tree_entry_for_window(above) {
            idx.min(self.entries.len())
        } else {
            return;
        };

        // Sway centres a dialog on the workspace like any floating view, not over
        // its parent (`container_floating_resize_and_center`,
        // sway/tree/container.c:850-894); only the stacking follows the parent.
        self.add_tile_at(idx, tile, activate);
    }

    fn bring_up_descendants_of(&mut self, idx: usize) {
        let tile = &self.entries[idx].tile;
        let win = tile.window();

        // We always maintain the correct stacking order, so walking descendants back to front
        // should give us all of them.
        let mut descendants: Vec<usize> = Vec::new();
        for (i, tile_below) in self
            .entries
            .iter()
            .map(|entry| &entry.tile)
            .enumerate()
            .skip(idx + 1)
            .rev()
        {
            let win_below = tile_below.window();
            if win_below.is_child_of(win)
                || descendants
                    .iter()
                    .any(|idx| win_below.is_child_of(self.entries[*idx].tile.window()))
            {
                descendants.push(i);
            }
        }

        // Now, descendants is in back-to-front order, and repositioning them in the front-to-back
        // order will preserve the subsequent indices and work out right.
        for (offset, descendant_idx) in descendants.into_iter().rev().enumerate() {
            self.raise_window(descendant_idx, idx + offset);
        }
    }

    pub fn remove_tile(
        &mut self,
        id: &W::Id,
        transaction: crate::utils::transaction::Transaction,
    ) -> RemovedTile<W> {
        if let Some(idx) = self.idx_of(id) {
            return self.remove_tile_by_idx(idx);
        }

        let (tree_idx, _) = self
            .tree_entry_for_window(id)
            .expect("window must belong to a floating entry");
        let tile = self.tree_entries[tree_idx]
            .tree
            .remove_tile(id, transaction)
            .expect("floating tree window must remain present until removal");
        if self.tree_entries[tree_idx].tree.is_empty() {
            self.tree_entries.remove(tree_idx);
        }
        if Some(tile.window().id()) == self.active_window_id.as_ref() {
            self.active_window_id = self.fallback_active_window();
        }
        removed_floating_tile(tile, self.working_area)
    }

    fn remove_tile_by_idx(&mut self, idx: usize) -> RemovedTile<W> {
        let FloatingEntry {
            mut tile,
            data,
            stamp,
            ..
        } = self.remove_entry(idx);
        tile.floating_stamp = Some(stamp);

        if Some(tile.window().id()) == self.active_window_id.as_ref() {
            self.active_window_id = self.fallback_active_window();
        }

        // Stop interactive resize.
        if let Some(resize) = &self.interactive_resize {
            if tile.window().id() == &resize.window {
                self.interactive_resize = None;
            }
        }

        // Store the floating position.
        tile.floating_pos = Some(data.pos);

        removed_floating_tile(tile, self.working_area)
    }

    pub fn start_close_animation_for_window(
        &mut self,
        renderer: &mut GlesRenderer,
        id: &W::Id,
        blocker: TransactionBlocker,
    ) {
        if let Some((idx, _)) = self.tree_entry_for_window(id) {
            let entry = &mut self.tree_entries[idx];
            entry
                .tree
                .start_close_animation_for_window(renderer, id, blocker);
            return;
        }

        let Some(idx) = self.idx_of(id) else {
            return;
        };
        let Some((tile, tile_pos)) = self
            .tiles_with_render_positions_mut(false)
            .find(|(tile, _)| tile.window().id() == id)
        else {
            return;
        };

        let Some(snapshot) = tile.take_unmap_snapshot() else {
            return;
        };

        let tile_size = tile.tile_size();

        self.start_close_animation_at(renderer, snapshot, tile_size, tile_pos, blocker, idx);
    }

    pub fn activate_window_without_raising(&mut self, id: &W::Id) -> bool {
        if !self.contains(id) {
            return false;
        }
        if let Some((idx, _)) = self.tree_entry_for_window(id) {
            let entry = &mut self.tree_entries[idx];
            entry.tree.activate_window(id);
        }
        self.active_window_id = Some(id.clone());
        true
    }

    /// Makes `id` the active floating view without touching its group's focus
    /// stack, for a view that arrived already holding seat focus: sway's
    /// `seat_set_focus` returns early for the focused node, so its new
    /// ancestors are not raised (sway/input/seat.c:1146-1150).
    pub fn adopt_focused_window(&mut self, id: &W::Id) -> bool {
        if !self.contains(id) {
            return false;
        }
        self.active_window_id = Some(id.clone());
        true
    }

    pub fn activate_window(&mut self, id: &W::Id) -> bool {
        if let Some(idx) = self.idx_of(id) {
            self.raise_window(idx, 0);
            self.active_window_id = Some(id.clone());
            self.bring_up_descendants_of(0);
            return true;
        }
        let Some((idx, _)) = self.tree_entry_for_window(id) else {
            return false;
        };
        let mut entry = self.tree_entries.remove(idx);
        entry.tree.activate_window(id);
        entry.stamp = self.bump_stamp();
        self.tree_entries.insert(0, entry);
        self.active_window_id = Some(id.clone());
        true
    }

    fn raise_window(&mut self, from_idx: usize, to_idx: usize) {
        assert!(to_idx <= from_idx);

        let mut entry = self.remove_entry(from_idx);
        entry.stamp = match to_idx
            .checked_sub(1)
            .and_then(|above| self.entries.get(above))
        {
            Some(above) => above.stamp,
            None => self.bump_stamp(),
        };
        self.insert_entry(to_idx, entry);
    }

    /// Inserts a live entry, keeping closing snapshots in their stack slots.
    pub(super) fn insert_entry(&mut self, idx: usize, entry: FloatingEntry<W>) {
        for (index, _) in &mut self.closing_windows {
            *index += usize::from(*index >= idx);
        }
        self.entries.insert(idx, entry);
    }

    /// Removes a live entry, keeping closing snapshots in their stack slots.
    pub(super) fn remove_entry(&mut self, idx: usize) -> FloatingEntry<W> {
        for (index, _) in &mut self.closing_windows {
            *index -= usize::from(*index > idx);
        }
        self.entries.remove(idx)
    }

    /// Starts a close animation for a tile that was not in this layout's stack, such as one
    /// being dragged; it renders in front.
    pub fn start_close_animation_for_tile(
        &mut self,
        renderer: &mut GlesRenderer,
        snapshot: TileRenderSnapshot,
        tile_size: Size<f64, Logical>,
        tile_pos: Point<f64, Logical>,
        blocker: TransactionBlocker,
    ) {
        self.start_close_animation_at(renderer, snapshot, tile_size, tile_pos, blocker, 0);
    }

    fn start_close_animation_at(
        &mut self,
        renderer: &mut GlesRenderer,
        snapshot: TileRenderSnapshot,
        tile_size: Size<f64, Logical>,
        tile_pos: Point<f64, Logical>,
        blocker: TransactionBlocker,
        stack_index: usize,
    ) {
        let anim = Animation::new(
            self.clock.clone(),
            0.,
            1.,
            0.,
            self.options.animations.window_close.anim,
        );

        let blocker = if self.options.disable_transactions {
            TransactionBlocker::completed()
        } else {
            blocker
        };

        let scale = Scale::from(self.scale);
        let res = ClosingWindow::new(
            renderer, snapshot, scale, tile_size, tile_pos, blocker, anim,
        );
        match res {
            Ok(closing) => {
                self.closing_windows.push((stack_index, closing));
            }
            Err(err) => {
                warn!("error creating a closing window animation: {err:?}");
            }
        }
    }

    pub fn toggle_window_width(&mut self, id: Option<&W::Id>, forwards: bool) {
        let Some(id) = id.or(self.active_window_id.as_ref()).cloned() else {
            return;
        };
        let Some(idx) = self.idx_of(&id) else {
            return;
        };

        let available_size = self.working_area.size.w;

        let len = self.options.layout.preset_column_widths.len();
        let tile = &mut self.entries[idx].tile;
        let preset_idx = if let Some(idx) = tile.floating_preset_width_idx {
            (idx + if forwards { 1 } else { len - 1 }) % len
        } else {
            let current_window = tile.window_expected_or_current_size().w;
            let current_tile = tile.tile_expected_or_current_size().w;

            let mut it = self
                .options
                .layout
                .preset_column_widths
                .iter()
                .map(|preset| resolve_preset_size(*preset, available_size));

            if forwards {
                it.position(|resolved| {
                    match resolved {
                        // Some allowance for fractional scaling purposes.
                        ResolvedSize::Tile(resolved) => current_tile + 1. < resolved,
                        ResolvedSize::Window(resolved) => current_window + 1. < resolved,
                    }
                })
                .unwrap_or(0)
            } else {
                it.rposition(|resolved| {
                    match resolved {
                        // Some allowance for fractional scaling purposes.
                        ResolvedSize::Tile(resolved) => resolved + 1. < current_tile,
                        ResolvedSize::Window(resolved) => resolved + 1. < current_window,
                    }
                })
                .unwrap_or(len - 1)
            }
        };

        let preset = self.options.layout.preset_column_widths[preset_idx];
        self.set_window_width(
            Some(&id),
            SizeChange::from(preset),
            true,
            self.view_size.to_i32_round(),
        );

        self.entries[idx].tile.floating_preset_width_idx = Some(preset_idx);

        self.interactive_resize_end(Some(&id));
    }

    pub fn start_open_animation(&mut self, id: &W::Id) -> bool {
        let Some(idx) = self.idx_of(id) else {
            return false;
        };

        self.entries[idx].tile.start_open_animation();
        true
    }

    pub fn toggle_window_height(&mut self, id: Option<&W::Id>, forwards: bool) {
        let Some(id) = id.or(self.active_window_id.as_ref()).cloned() else {
            return;
        };
        let Some(idx) = self.idx_of(&id) else {
            return;
        };

        let available_size = self.working_area.size.h;

        let len = self.options.layout.preset_window_heights.len();
        let tile = &mut self.entries[idx].tile;
        let preset_idx = if let Some(idx) = tile.floating_preset_height_idx {
            (idx + if forwards { 1 } else { len - 1 }) % len
        } else {
            let current_window = tile.window_expected_or_current_size().h;
            let current_tile = tile.tile_expected_or_current_size().h;

            let mut it = self
                .options
                .layout
                .preset_window_heights
                .iter()
                .map(|preset| resolve_preset_size(*preset, available_size));

            if forwards {
                it.position(|resolved| {
                    match resolved {
                        // Some allowance for fractional scaling purposes.
                        ResolvedSize::Tile(resolved) => current_tile + 1. < resolved,
                        ResolvedSize::Window(resolved) => current_window + 1. < resolved,
                    }
                })
                .unwrap_or(0)
            } else {
                it.rposition(|resolved| {
                    match resolved {
                        // Some allowance for fractional scaling purposes.
                        ResolvedSize::Tile(resolved) => resolved + 1. < current_tile,
                        ResolvedSize::Window(resolved) => resolved + 1. < current_window,
                    }
                })
                .unwrap_or(len - 1)
            }
        };

        let preset = self.options.layout.preset_window_heights[preset_idx];
        self.set_window_height(
            Some(&id),
            SizeChange::from(preset),
            true,
            self.view_size.to_i32_round(),
        );

        let tile = &mut self.entries[idx].tile;
        tile.floating_preset_height_idx = Some(preset_idx);

        self.interactive_resize_end(Some(&id));
    }

    pub fn set_window_border(
        &mut self,
        id: &W::Id,
        style: swayward_ipc::command::BorderStyle,
        width: Option<u16>,
    ) -> bool {
        self.change_decorations_keeping_content(id, |tile| {
            tile.set_sway_border(style, width, true).is_ok()
        })
    }

    /// See [`Tile::use_client_decorations_from_map`]. A floating root keeps the content
    /// box it was centred on: sway marks the view CSD before `container_set_floating`
    /// centres it (sway/tree/view.c:904-911, sway/tree/container.c:955-968).
    pub fn use_client_decorations_from_map(&mut self, id: &W::Id) -> bool {
        self.change_decorations_keeping_content(id, |tile| {
            tile.use_client_decorations_from_map(true);
            true
        })
    }

    /// Runs `change` on the floating root `id` and moves the container so the content
    /// stays put, as sway keeps a floating view's content box and moves the container
    /// around it (`container_set_geometry_from_content`, sway/commands/border.c:94-96,
    /// sway/tree/container.c:1018-1039). Returns what `change` returned, or false
    /// when `id` is not a floating root.
    fn change_decorations_keeping_content(
        &mut self,
        id: &W::Id,
        change: impl FnOnce(&mut Tile<W>) -> bool,
    ) -> bool {
        let Some(index) = self.idx_of(id) else {
            return false;
        };
        let entry = &mut self.entries[index];
        let content_before = entry.tile.window_loc();
        let changed = change(&mut entry.tile);
        if changed {
            entry.data.update(&entry.tile);
            if entry.tile.sizing_mode().is_normal() {
                let shift = entry.tile.window_loc() - content_before;
                let pos = entry.data.logical_pos - shift;
                entry.data.set_logical_pos(pos);
            }
        }
        changed
    }

    pub fn set_window_width(
        &mut self,
        id: Option<&W::Id>,
        change: SizeChange,
        animate: bool,
        automatic_maximum: Size<i32, Logical>,
    ) -> bool {
        let Some(id) = id.or(self.active_window_id.as_ref()) else {
            return false;
        };
        // A floating group's child is not itself floating, so sway resizes
        // it inside the group like a tiled child (`container_is_floating`,
        // sway/commands/resize.c:523-550).
        if let Some((idx, _)) = self.tree_entry_for_window(id) {
            let id = id.clone();
            return self.tree_entries[idx]
                .tree
                .set_window_width(Some(&id), change);
        }
        let Some(idx) = self.idx_of(id) else {
            return false;
        };

        let tile = &mut self.entries[idx].tile;
        tile.floating_preset_width_idx = None;

        let available_size = self.working_area.size.w;
        let win = tile.window();
        let current_window = win.expected_size().unwrap_or_else(|| win.size()).w;
        let current_tile = tile.tile_expected_or_current_size().w;

        const MAX_PX: f64 = 100000.;
        const MAX_F: f64 = 10000.;

        let win_width = match change {
            SizeChange::SetFixed(win_width) => f64::from(win_width),
            SizeChange::SetProportion(prop) => {
                let prop = (prop / 100.).clamp(0., MAX_F);
                let tile_width = available_size * prop;
                tile.window_width_for_tile_width(tile_width)
            }
            SizeChange::AdjustFixed(delta) => f64::from(current_window.saturating_add(delta)),
            SizeChange::AdjustProportion(delta) => {
                let current_prop = current_tile / available_size;
                let prop = (current_prop + delta / 100.).clamp(0., MAX_F);
                let tile_width = available_size * prop;
                tile.window_width_for_tile_width(tile_width)
            }
        };
        let win_width = win_width.round().clamp(1., MAX_PX) as i32;

        let win = tile.window_mut();
        let min_size = win.min_size();
        let max_size = win.max_size();

        let win_height = win.expected_size().unwrap_or_default().h;
        let win_size = constrain_floating_size(
            Size::from((win_width, win_height)),
            self.options.layout.floating_minimum_size,
            self.options.layout.floating_maximum_size,
            automatic_maximum.to_f64(),
            min_size,
            max_size,
        );
        win.request_size_once(win_size, animate);
        let held = f64::from(
            constrain_floating_size(
                Size::from((win_width, win_width)),
                self.options.layout.floating_minimum_size,
                self.options.layout.floating_maximum_size,
                automatic_maximum.to_f64(),
                Size::default(),
                Size::default(),
            )
            .w,
        );
        tile.resize_floating_content(Some(held), None);
        let entry = &mut self.entries[idx];
        entry.data.update(&entry.tile);
        current_window != win_size.w
    }

    pub fn set_window_outer_width(
        &mut self,
        id: &W::Id,
        change: SizeChange,
        automatic_maximum: Size<i32, Logical>,
    ) {
        let Some(idx) = self.idx_of(id) else { return };
        let change = match change {
            SizeChange::SetFixed(value) => SizeChange::SetFixed(
                self.entries[idx]
                    .tile
                    .window_width_for_tile_width(f64::from(value))
                    .round() as i32,
            ),
            SizeChange::SetProportion(value) => SizeChange::SetFixed(
                self.entries[idx]
                    .tile
                    .window_width_for_tile_width((self.working_area.size.w * value / 100.).trunc())
                    .round() as i32,
            ),
            change => change,
        };
        let before = self.entries[idx].tile.tile_expected_or_current_size();
        self.set_window_width(Some(id), change, true, automatic_maximum);
        self.recenter_after_resize_set(idx, before);
    }

    pub fn set_window_outer_height(
        &mut self,
        id: &W::Id,
        change: SizeChange,
        automatic_maximum: Size<i32, Logical>,
    ) {
        let Some(idx) = self.idx_of(id) else { return };
        let change = match change {
            SizeChange::SetFixed(value) => SizeChange::SetFixed(
                self.entries[idx]
                    .tile
                    .window_height_for_tile_height(f64::from(value))
                    .round() as i32,
            ),
            SizeChange::SetProportion(value) => SizeChange::SetFixed(
                self.entries[idx]
                    .tile
                    .window_height_for_tile_height(
                        (self.working_area.size.h * value / 100.).trunc(),
                    )
                    .round() as i32,
            ),
            change => change,
        };
        let before = self.entries[idx].tile.tile_expected_or_current_size();
        self.set_window_height(Some(id), change, true, automatic_maximum);
        self.recenter_after_resize_set(idx, before);
    }

    /// Sway's `resize set` on a floating view moves the container by half the growth, so
    /// the box keeps its centre (`con->pending.x -= grow_width / 2`, integer division,
    /// sway/commands/resize.c:360-362 and :381-383).
    fn recenter_after_resize_set(&mut self, idx: usize, before: Size<f64, Logical>) {
        let entry = &mut self.entries[idx];
        if !entry.tile.sizing_mode().is_normal() {
            return;
        }
        let after = entry.tile.tile_expected_or_current_size();
        let shift = Point::from((
            ((after.w - before.w).trunc() / 2.).trunc(),
            ((after.h - before.h).trunc() / 2.).trunc(),
        ));
        if shift != Point::from((0., 0.)) {
            let pos = entry.data.logical_pos - shift;
            entry.data.set_logical_pos(pos);
        }
    }

    /// `resize set` on a view inside a floating group resizes it as a tiled
    /// child of the group, converting ppt against the group's splits
    /// (`resize_set_tiled`, sway/commands/resize.c:286-336, reached because
    /// `container_is_floating` is false for the child, resize.c:523).
    /// Returns false when `window` is not inside a group.
    pub fn set_tree_window_size_sway(
        &mut self,
        window: &W::Id,
        width: Option<SizeChange>,
        height: Option<SizeChange>,
    ) -> bool {
        let Some((idx, _)) = self.tree_entry_for_window(window) else {
            return false;
        };
        self.tree_entries[idx]
            .tree
            .set_window_size_sway(window, width, height);
        true
    }

    pub fn resize_window_edge(
        &mut self,
        id: Option<&W::Id>,
        edge: ResizeEdge,
        change: SizeChange,
    ) -> bool {
        let Some(id) = id.or(self.active_window_id.as_ref()).cloned() else {
            return false;
        };
        // A floating group's child is not itself floating, so sway resizes it
        // inside the group like a tiled child (`container_is_floating`,
        // sway/commands/resize.c:523-550).
        if let Some((idx, _)) = self.tree_entry_for_window(&id) {
            let entry = &mut self.tree_entries[idx];
            return entry.tree.resize_window_edge(Some(&id), edge, change);
        }
        let Some(idx) = self.idx_of(&id) else {
            return false;
        };
        let old_size = self.entries[idx].tile.tile_expected_or_current_size();
        if edge.intersects(ResizeEdge::LEFT_RIGHT) {
            self.set_window_width(Some(&id), change, true, self.view_size.to_i32_round());
        } else {
            self.set_window_height(Some(&id), change, true, self.view_size.to_i32_round());
        }
        let new_size = self.entries[idx].tile.tile_expected_or_current_size();
        if old_size == new_size {
            return false;
        }
        let mut offset = Point::from((0., 0.));
        if edge.contains(ResizeEdge::LEFT) {
            offset.x = old_size.w - new_size.w;
        }
        if edge.contains(ResizeEdge::TOP) {
            offset.y = old_size.h - new_size.h;
        }
        let pos = self.entries[idx].data.logical_pos + offset;
        self.entries[idx].data.set_logical_pos(pos);
        true
    }

    pub fn set_window_height(
        &mut self,
        id: Option<&W::Id>,
        change: SizeChange,
        animate: bool,
        automatic_maximum: Size<i32, Logical>,
    ) -> bool {
        let Some(id) = id.or(self.active_window_id.as_ref()) else {
            return false;
        };
        // A floating group's child is not itself floating, so sway resizes
        // it inside the group like a tiled child (`container_is_floating`,
        // sway/commands/resize.c:523-550).
        if let Some((idx, _)) = self.tree_entry_for_window(id) {
            let id = id.clone();
            return self.tree_entries[idx]
                .tree
                .set_window_height(Some(&id), change);
        }
        let Some(idx) = self.idx_of(id) else {
            return false;
        };

        let tile = &mut self.entries[idx].tile;
        tile.floating_preset_height_idx = None;

        let available_size = self.working_area.size.h;
        let win = tile.window();
        let current_window = win.expected_size().unwrap_or_else(|| win.size()).h;
        let current_tile = tile.tile_expected_or_current_size().h;

        const MAX_PX: f64 = 100000.;
        const MAX_F: f64 = 10000.;

        let win_height = match change {
            SizeChange::SetFixed(win_height) => f64::from(win_height),
            SizeChange::SetProportion(prop) => {
                let prop = (prop / 100.).clamp(0., MAX_F);
                let tile_height = available_size * prop;
                tile.window_height_for_tile_height(tile_height)
            }
            SizeChange::AdjustFixed(delta) => f64::from(current_window.saturating_add(delta)),
            SizeChange::AdjustProportion(delta) => {
                let current_prop = current_tile / available_size;
                let prop = (current_prop + delta / 100.).clamp(0., MAX_F);
                let tile_height = available_size * prop;
                tile.window_height_for_tile_height(tile_height)
            }
        };
        let win_height = win_height.round().clamp(1., MAX_PX) as i32;

        let win = tile.window_mut();
        let min_size = win.min_size();
        let max_size = win.max_size();

        let win_width = win.expected_size().unwrap_or_default().w;
        let win_size = constrain_floating_size(
            Size::from((win_width, win_height)),
            self.options.layout.floating_minimum_size,
            self.options.layout.floating_maximum_size,
            automatic_maximum.to_f64(),
            min_size,
            max_size,
        );
        win.request_size_once(win_size, animate);
        let held = f64::from(
            constrain_floating_size(
                Size::from((win_height, win_height)),
                self.options.layout.floating_minimum_size,
                self.options.layout.floating_maximum_size,
                automatic_maximum.to_f64(),
                Size::default(),
                Size::default(),
            )
            .h,
        );
        tile.resize_floating_content(None, Some(held));
        let entry = &mut self.entries[idx];
        entry.data.update(&entry.tile);
        current_window != win_size.h
    }

    fn focus_directional(
        &mut self,
        direction: Direction,
        distance: impl Fn(Point<f64, Logical>, Point<f64, Logical>) -> f64,
        wrap: bool,
    ) -> bool {
        let Some(active_id) = &self.active_window_id else {
            return false;
        };
        if let Some((idx, _)) = self.tree_entry_for_window(active_id) {
            let entry = &mut self.tree_entries[idx];
            let moved = entry.tree.focus_direction(direction);
            self.active_window_id = entry.tree.active_window().map(|window| window.id().clone());
            return moved;
        }
        let Some(active_idx) = self.idx_of(active_id) else {
            return false;
        };
        let center = self.entries[active_idx].data.center();

        let candidates = || {
            self.entries
                .iter()
                .map(|entry| (entry.tile.window().id(), entry.data.center()))
                .chain(self.tree_entries.iter().filter_map(|entry| {
                    entry.tree.active_window().map(|window| {
                        (
                            window.id(),
                            entry.rect.loc + entry.rect.size.to_point().downscale(2.),
                        )
                    })
                }))
                .filter(|(id, _)| *id != active_id)
                .map(|(id, other)| (id, distance(center, other)))
        };
        // Without wrap this is sway's node_get_in_direction_floating: skip only `distance < 0`
        // and leave focus alone when nothing lies that way (sway/commands/focus.c:243-258).
        if !wrap {
            let result = candidates()
                .filter(|(_, dist)| *dist >= 0.)
                .min_by(|(_, dist_a), (_, dist_b)| f64::total_cmp(dist_a, dist_b));
            let Some((id, _)) = result else {
                return false;
            };
            let id = id.clone();
            self.activate_window(&id);
            return true;
        }
        let result = candidates()
            .filter(|(_, dist)| *dist > 0.)
            .min_by(|(_, dist_a), (_, dist_b)| f64::total_cmp(dist_a, dist_b))
            .or_else(|| {
                candidates()
                    .filter(|(_, dist)| *dist <= 0.)
                    .min_by(|(_, dist_a), (_, dist_b)| f64::total_cmp(dist_a, dist_b))
            });
        if let Some((id, _)) = result {
            let id = id.clone();
            self.activate_window(&id);
            true
        } else {
            false
        }
    }

    /// `focus next|prev` on a floating window. A floating root has no parent, so sway takes the
    /// axis from the workspace layout (`container_parent_layout`, sway/tree/container.c:1353-1361);
    /// a window inside a floating split uses that split
    /// (`get_direction_from_next_prev`, sway/commands/focus.c:17-58).
    pub fn focus_next_or_prev(&mut self, next: bool, workspace_layout: TreeLayout) -> bool {
        let Some(active_id) = &self.active_window_id else {
            return false;
        };
        if let Some((idx, _)) = self.tree_entry_for_window(active_id) {
            let entry = &mut self.tree_entries[idx];
            let moved = entry.tree.focus_next_or_prev(next);
            self.active_window_id = entry.tree.active_window().map(|window| window.id().clone());
            return moved;
        }
        // Sway moves among floaters without wrapping (sway/commands/focus.c:458-460).
        match (next, workspace_layout) {
            (false, TreeLayout::SplitH | TreeLayout::Tabbed) => {
                self.focus_directional(Direction::Left, |focus, other| focus.x - other.x, false)
            }
            (true, TreeLayout::SplitH | TreeLayout::Tabbed) => {
                self.focus_directional(Direction::Right, |focus, other| other.x - focus.x, false)
            }
            (false, TreeLayout::SplitV | TreeLayout::Stacked) => {
                self.focus_directional(Direction::Up, |focus, other| focus.y - other.y, false)
            }
            (true, TreeLayout::SplitV | TreeLayout::Stacked) => {
                self.focus_directional(Direction::Down, |focus, other| other.y - focus.y, false)
            }
        }
    }

    pub fn focus_left(&mut self) -> bool {
        self.focus_directional(Direction::Left, |focus, other| focus.x - other.x, true)
    }

    /// Sway's `node_get_in_direction_floating`: the nearest floater that way, never a wrap
    /// (sway/commands/focus.c:226-258).
    pub fn focus_direction_without_wrap(&mut self, direction: Direction) -> bool {
        match direction {
            Direction::Left => {
                self.focus_directional(direction, |focus, other| focus.x - other.x, false)
            }
            Direction::Right => {
                self.focus_directional(direction, |focus, other| other.x - focus.x, false)
            }
            Direction::Up => {
                self.focus_directional(direction, |focus, other| focus.y - other.y, false)
            }
            Direction::Down => {
                self.focus_directional(direction, |focus, other| other.y - focus.y, false)
            }
        }
    }

    pub fn focus_right(&mut self) -> bool {
        self.focus_directional(Direction::Right, |focus, other| other.x - focus.x, true)
    }

    pub fn focus_up(&mut self) -> bool {
        self.focus_directional(Direction::Up, |focus, other| focus.y - other.y, true)
    }

    pub fn focus_down(&mut self) -> bool {
        self.focus_directional(Direction::Down, |focus, other| other.y - focus.y, true)
    }

    pub fn focus_leftmost(&mut self) {
        let result = self
            .tiles_with_offsets()
            .min_by(|(_, pos_a), (_, pos_b)| f64::total_cmp(&pos_a.x, &pos_b.x));
        if let Some((tile, _)) = result {
            let id = tile.window().id().clone();
            self.activate_window(&id);
        }
    }

    pub fn focus_rightmost(&mut self) {
        let result = self
            .tiles_with_offsets()
            .max_by(|(_, pos_a), (_, pos_b)| f64::total_cmp(&pos_a.x, &pos_b.x));
        if let Some((tile, _)) = result {
            let id = tile.window().id().clone();
            self.activate_window(&id);
        }
    }

    pub fn focus_topmost(&mut self) {
        let result = self
            .tiles_with_offsets()
            .min_by(|(_, pos_a), (_, pos_b)| f64::total_cmp(&pos_a.y, &pos_b.y));
        if let Some((tile, _)) = result {
            let id = tile.window().id().clone();
            self.activate_window(&id);
        }
    }

    pub fn focus_bottommost(&mut self) {
        let result = self
            .tiles_with_offsets()
            .max_by(|(_, pos_a), (_, pos_b)| f64::total_cmp(&pos_a.y, &pos_b.y));
        if let Some((tile, _)) = result {
            let id = tile.window().id().clone();
            self.activate_window(&id);
        }
    }

    fn move_to(&mut self, idx: usize, new_pos: Point<f64, Logical>, animate: bool) {
        if animate {
            self.move_and_animate(idx, new_pos);
        } else {
            self.entries[idx].data.set_logical_pos(new_pos);
        }

        self.interactive_resize_end(None);
    }

    fn move_by(&mut self, amount: Point<f64, Logical>) {
        let Some(active_id) = &self.active_window_id else {
            return;
        };
        if let Some((idx, _)) = self.tree_entry_for_window(active_id) {
            let entry = &mut self.tree_entries[idx];
            entry.rect.loc += amount;
            // container_floating_translate moves the children along
            // (sway/sway/tree/container.c:1113-1128).
            if let Some(anchor) = &mut entry.ipc_anchor {
                *anchor += amount;
            }
            entry.pos =
                Data::logical_to_size_frac_in_working_area(self.working_area, entry.rect.loc);
            entry.tree.update_config(
                self.view_size,
                entry.rect,
                false,
                self.scale,
                self.options.clone(),
            );
            return;
        }
        let idx = self.idx_of(active_id).unwrap();

        let new_pos = self.entries[idx].data.logical_pos + amount;
        self.move_to(idx, new_pos, true)
    }

    pub fn move_left(&mut self) {
        self.move_by(Point::from((-DIRECTIONAL_MOVE_PX, 0.)));
    }

    pub fn move_right(&mut self) {
        self.move_by(Point::from((DIRECTIONAL_MOVE_PX, 0.)));
    }

    pub fn move_up(&mut self) {
        self.move_by(Point::from((0., -DIRECTIONAL_MOVE_PX)));
    }

    pub fn move_down(&mut self) {
        self.move_by(Point::from((0., DIRECTIONAL_MOVE_PX)));
    }

    pub fn move_window(
        &mut self,
        id: Option<&W::Id>,
        x: PositionChange,
        y: PositionChange,
        animate: bool,
    ) {
        let Some(id) = id.or(self.active_window_id.as_ref()) else {
            return;
        };
        if let Some((idx, _)) = self.tree_entry_for_window(id) {
            let entry = &mut self.tree_entries[idx];
            let mut pos = entry.rect.loc;
            pos.x =
                apply_position_change(pos.x, x, self.working_area.size.w, self.working_area.loc.x);
            pos.y =
                apply_position_change(pos.y, y, self.working_area.size.h, self.working_area.loc.y);
            if let Some(anchor) = &mut entry.ipc_anchor {
                *anchor += pos - entry.rect.loc;
            }
            entry.rect.loc = pos;
            entry.pos = Data::logical_to_size_frac_in_working_area(self.working_area, pos);
            entry.tree.update_config(
                self.view_size,
                entry.rect,
                false,
                self.scale,
                self.options.clone(),
            );
            return;
        }
        let idx = self.idx_of(id).unwrap();

        let mut pos = self.entries[idx].data.logical_pos;

        let available_width = self.working_area.size.w;
        let available_height = self.working_area.size.h;
        let working_area_loc = self.working_area.loc;

        pos.x = apply_position_change(pos.x, x, available_width, working_area_loc.x);
        pos.y = apply_position_change(pos.y, y, available_height, working_area_loc.y);

        self.move_to(idx, pos, animate);
    }

    pub fn center_window(&mut self, id: Option<&W::Id>) {
        let Some(id) = id.or(self.active_window_id.as_ref()).cloned() else {
            return;
        };
        if let Some((idx, _)) = self.tree_entry_for_window(&id) {
            let entry = &mut self.tree_entries[idx];
            let center = center_preferring_top_left_in_area(self.working_area, entry.rect.size);
            if let Some(anchor) = &mut entry.ipc_anchor {
                *anchor += center - entry.rect.loc;
            }
            entry.rect.loc = center;
            entry.pos =
                Data::logical_to_size_frac_in_working_area(self.working_area, entry.rect.loc);
            entry.tree.update_config(
                self.view_size,
                entry.rect,
                false,
                self.scale,
                self.options.clone(),
            );
            return;
        }
        let Some(idx) = self.idx_of(&id) else {
            return;
        };

        let new_pos =
            center_preferring_top_left_in_area(self.working_area, self.entries[idx].data.size);
        self.move_to(idx, new_pos, true);
    }

    pub fn descendants_added(&mut self, id: &W::Id) -> bool {
        let Some(idx) = self.idx_of(id) else {
            return false;
        };

        self.bring_up_descendants_of(idx);
        true
    }

    pub fn update_window(&mut self, id: &W::Id, serial: Option<Serial>) -> bool {
        if let Some((idx, _)) = self.tree_entry_for_window(id) {
            let entry = &mut self.tree_entries[idx];
            return entry.tree.update_window(id, serial);
        }
        let Some(tile_idx) = self.idx_of(id) else {
            return false;
        };

        let entry = &mut self.entries[tile_idx];
        let tile = &mut entry.tile;
        let data = &mut entry.data;

        let resize = tile.window_mut().interactive_resize_data();

        // Do this before calling update_window() so it can get up-to-date info.
        if let Some(serial) = serial {
            tile.window_mut().on_commit(serial);
        }

        let prev_size = data.size;

        tile.update_window();
        data.update(tile);

        // When resizing by top/left edge, update the position accordingly.
        if let Some(resize) = resize {
            let mut offset = Point::from((0., 0.));
            if resize.edges.contains(ResizeEdge::LEFT) {
                offset.x += prev_size.w - data.size.w;
            }
            if resize.edges.contains(ResizeEdge::TOP) {
                offset.y += prev_size.h - data.size.h;
            }
            data.set_logical_pos(data.logical_pos + offset);
        }

        true
    }
}

impl<W: LayoutElement> FloatingLayout<W> {
    /// Moves floating root `moved` directly above `anchor` in the stacking
    /// order. Sway inserts a container moved onto a floating view into
    /// `workspace->floating` right after the view (`container_add_sibling`,
    /// sway/tree/container.c:1410-1423), and that list is the stacking order.
    pub fn restack_above(&mut self, moved: &StackSlot<W::Id>, anchor: &StackSlot<W::Id>) {
        let mut order = self.stacking();
        if moved == anchor || !order.contains(anchor) {
            return;
        }
        let Some(from) = order.iter().position(|slot| slot == moved) else {
            return;
        };
        let slot = order.remove(from);
        let Some(at) = order.iter().position(|slot| slot == anchor) else {
            return;
        };
        order.insert(at, slot);

        // Renumber every root bottom to top, which keeps the merged order.
        let base = self.next_stamp;
        for (offset, slot) in order.iter().rev().enumerate() {
            let stamp = base + 1 + offset as u64;
            match slot {
                StackSlot::Window(id) => {
                    if let Some(entry) = self
                        .entries
                        .iter_mut()
                        .find(|entry| entry.tile.window().id() == id)
                    {
                        entry.stamp = stamp;
                    }
                }
                StackSlot::Tree(root) => {
                    if let Some(entry) = self
                        .tree_entries
                        .iter_mut()
                        .find(|entry| entry.root == *root)
                    {
                        entry.stamp = stamp;
                    }
                }
            }
        }
        self.next_stamp = base + order.len() as u64;

        // Each vector stays sorted top first; only the moved root changed place.
        match moved {
            StackSlot::Window(id) => {
                if let Some(idx) = self.idx_of(id) {
                    let entry = self.remove_entry(idx);
                    let idx = self
                        .entries
                        .iter()
                        .position(|other| other.stamp < entry.stamp)
                        .unwrap_or(self.entries.len());
                    self.insert_entry(idx, entry);
                }
            }
            StackSlot::Tree(_) => self
                .tree_entries
                .sort_by_key(|entry| std::cmp::Reverse(entry.stamp)),
        }
    }
}
