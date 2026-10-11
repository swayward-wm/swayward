use super::*;

impl Swayward {
    /// Keyboard focus moved onto `surface`. Sway clears a view's urgency, or
    /// starts its timer, only when seat focus lands on the view
    /// (`seat_set_workspace_focus`, sway/sway/input/seat.c:1223-1240). The
    /// timer runs when the view's output showed another workspace before the
    /// change (`last_workspace`, seat.c:1158-1161), whatever the keyboard
    /// focused before.
    pub(super) fn focus_clears_urgency(&mut self, surface: &WlSurface) {
        // swayward keeps the keyboard on a view while a split or the
        // workspace holds seat focus; sway focuses no view then.
        if !self.seat_focus_is_on_view() {
            return;
        }
        let Some((mapped, _)) = self.layout.find_window_and_output(surface) else {
            return;
        };
        let window = mapped.window.clone();
        let changed_workspace = self
            .layout
            .window_workspace_id(&window)
            .is_some_and(|workspace| {
                !self.urgency_active_workspaces.is_empty()
                    && !self.urgency_active_workspaces.contains(&workspace)
            });
        let Some((mapped, _)) = self.layout.find_window_and_output_mut(surface) else {
            return;
        };
        if !mapped.is_urgent() || self.urgency_timers.contains_key(&mapped.id()) {
            return;
        }

        let id = mapped.id();
        let timeout_ms = self.config.borrow().urgent_timeout_ms.0;
        if !changed_workspace || timeout_ms == 0 {
            mapped.set_urgent(false);
            return;
        }

        let token = self
            .event_loop
            .insert_source(
                Timer::from_duration(Duration::from_millis(u64::from(timeout_ms))),
                move |_, _, state| {
                    state.swayward.urgency_timers.remove(&id);
                    state.swayward.clear_window_urgency(id);
                    TimeoutAction::Drop
                },
            )
            .unwrap();
        self.urgency_timers.insert(id, token);
    }

    /// Whether sway's seat focus would be on a view rather than on a split or
    /// the workspace itself.
    fn seat_focus_is_on_view(&self) -> bool {
        let Some(workspace) = self.layout.active_workspace() else {
            return true;
        };
        !workspace.is_workspace_focused()
            && !workspace
                .focused_container_node()
                .is_some_and(|node| workspace.is_tiling_split(node))
    }

    /// Records each output's active workspace, the `last_workspace` of the
    /// next seat focus change (sway/sway/input/seat.c:1158-1161).
    pub(super) fn record_urgency_active_workspaces(&mut self) {
        self.urgency_active_workspaces.clear();
        self.urgency_active_workspaces.extend(
            self.layout
                .monitors()
                .map(|monitor| monitor.active_workspace_ref().id()),
        );
    }

    /// Seat focus reached a view that already held keyboard focus, as after
    /// `focus parent` then `focus child` or a criteria `focus`. Sway clears
    /// its urgency on that seat focus change, on the same workspace, unless a
    /// timer is pending (`seat_set_workspace_focus`,
    /// sway/sway/input/seat.c:1223-1240); keyboard focus did not move, so
    /// `focus_clears_urgency` never ran.
    pub fn seat_refocus_clears_urgency(&mut self, id: MappedId) {
        if self.urgency_timers.contains_key(&id) {
            return;
        }
        self.clear_window_urgency(id);
    }

    fn clear_window_urgency(&mut self, id: MappedId) {
        self.layout.with_windows_mut(|mapped, _| {
            if mapped.id() == id {
                mapped.set_urgent(false);
            }
        });
        self.queue_redraw_all();
    }

    #[cfg(test)]
    pub fn fire_urgency_timer_for_test(&mut self, id: MappedId) -> bool {
        let Some(token) = self.urgency_timers.remove(&id) else {
            return false;
        };
        self.event_loop.remove(token);
        self.clear_window_urgency(id);
        true
    }

    pub fn cancel_urgency_timer(&mut self, id: MappedId) {
        if let Some(token) = self.urgency_timers.remove(&id) {
            self.event_loop.remove(token);
        }
    }

    pub fn set_window_urgent(&mut self, id: MappedId, urgent: bool) {
        if !urgent {
            self.cancel_urgency_timer(id);
        }
        self.layout.with_windows_mut(|mapped, _| {
            if mapped.id() == id {
                mapped.set_urgent_unguarded(urgent);
            }
        });
    }
}
