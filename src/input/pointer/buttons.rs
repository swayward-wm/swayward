use super::*;

#[derive(Debug, Clone, Copy)]
pub(super) struct DragPolicy {
    pub(super) mod_down: bool,
    pub(super) move_button: MouseButton,
    pub(super) resize_button: MouseButton,
}

pub(super) fn floating_drag_policy(
    floating: Option<swayward_config::input::FloatingModifier>,
    fallback: ModKey,
    modifiers: Modifiers,
) -> DragPolicy {
    match floating {
        None => DragPolicy {
            mod_down: fallback.is_pressed(modifiers),
            move_button: MouseButton::Left,
            resize_button: MouseButton::Right,
        },
        Some(floating) => {
            let mod_down = floating.modifier.is_pressed(modifiers);
            let (move_button, resize_button) = if floating.inverse {
                (MouseButton::Right, MouseButton::Left)
            } else {
                (MouseButton::Left, MouseButton::Right)
            };
            DragPolicy {
                mod_down,
                move_button,
                resize_button,
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum PressIntent {
    BorderResize(Point<f64, Logical>, ResizeEdge),
    Move { tiling: bool, threshold: f64 },
    CornerResize,
    None,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn classify_press(
    button: Option<MouseButton>,
    policy: DragPolicy,
    is_tiling: bool,
    in_floating_group: bool,
    fullscreen: bool,
    on_titlebar: bool,
    overview: bool,
    grabbed: bool,
    tiling_drag: bool,
    tiling_drag_threshold: f64,
    border_resize: Option<(Point<f64, Logical>, ResizeEdge)>,
) -> PressIntent {
    // Sway gates tiled modifier and titlebar drags independently of floating
    // moves (`sway/input/seatop_default.c:490-500`).
    let regular_move = !fullscreen
        && if is_tiling {
            tiling_drag
                && ((button == Some(policy.move_button) && policy.mod_down)
                    || (button == Some(MouseButton::Left) && on_titlebar))
        } else {
            // A view in a floating group moves the group from a modifier drag
            // or a titlebar drag (`sway/input/seatop_default.c:459-463`).
            (button == Some(policy.move_button) && policy.mod_down)
                || (in_floating_group && button == Some(MouseButton::Left) && on_titlebar)
        };

    // Sway resizes from a border on a plain left press, tiled before any
    // modifier move and floating after one
    // (`sway/sway/input/seatop_default.c:396-474`). A view in a floating
    // group is not floating itself (`container_is_floating`,
    // sway/tree/container.c:1041-1050), so its border takes the tiled branch.
    if !overview
        && button == Some(MouseButton::Left)
        && !grabbed
        && (is_tiling || in_floating_group || !regular_move)
    {
        if let Some((location, edges)) = border_resize {
            return PressIntent::BorderResize(location, edges);
        }
    }
    // Overview click-to-move is inherited niri behavior and remains on left;
    // only floating drags follow sway's inverse modifier policy.
    if (overview && button == Some(MouseButton::Left) || regular_move) && !grabbed {
        let threshold = if is_tiling && !policy.mod_down {
            tiling_drag_threshold
        } else {
            0.
        };
        return PressIntent::Move {
            tiling: is_tiling,
            threshold,
        };
    }
    if button == Some(policy.resize_button) && !grabbed && policy.mod_down {
        return PressIntent::CornerResize;
    }
    PressIntent::None
}

impl State {
    pub(super) fn take_release_button_bind(
        &mut self,
        input_device: &str,
        button_code: u32,
    ) -> bool {
        let suppressed = self.swayward.suppressed_buttons.remove(&button_code);
        if let Some(bind) = self
            .swayward
            .held_release_buttons
            .remove(&(input_device.to_owned(), button_code))
        {
            self.handle_bind(bind);
            return true;
        }
        suppressed
    }

    pub(super) fn resolve_button_bind(
        &mut self,
        button: Option<MouseButton>,
        button_code: u32,
        input_device: &str,
        mod_key: ModKey,
        mods: ModifiersState,
        modifiers: Modifiers,
    ) -> Option<Bind> {
        let trigger = match button? {
            MouseButton::Left => Trigger::MouseLeft,
            MouseButton::Right => Trigger::MouseRight,
            MouseButton::Middle => Trigger::MouseMiddle,
            MouseButton::Back => Trigger::MouseBack,
            MouseButton::Forward => Trigger::MouseForward,
            _ => return None,
        };
        let config = self.swayward.config.borrow();
        let bindings = make_binds_iter(
            &config,
            &self.swayward.binding_mode,
            &mut self.swayward.window_mru_ui,
            modifiers,
        );
        let release = find_configured_bind_for_device(
            bindings.clone().filter(|bind| bind.release),
            mod_key,
            trigger,
            mods,
            input_device,
        );
        let press = find_configured_bind_for_device(
            bindings.filter(|bind| !bind.release),
            mod_key,
            trigger,
            mods,
            input_device,
        );
        drop(config);
        let release = release.filter(|bind| {
            self.mouse_bind_matches_region(bind)
                && (!self.swayward.screenshot_ui.is_open()
                    || allowed_during_screenshot(&bind.action))
        });
        let press = press.filter(|bind| {
            self.mouse_bind_matches_region(bind)
                && (!self.swayward.screenshot_ui.is_open()
                    || allowed_during_screenshot(&bind.action))
        });
        if let Some(release) = release {
            self.swayward
                .held_release_buttons
                .insert((input_device.to_owned(), button_code), release);
        }
        press
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_press_preserves_resize_and_move_order() {
        let policy = DragPolicy {
            mod_down: true,
            move_button: MouseButton::Left,
            resize_button: MouseButton::Right,
        };
        let border = (Point::from((1., 2.)), ResizeEdge::LEFT);
        assert!(matches!(
            classify_press(
                Some(MouseButton::Left),
                policy,
                true,
                false,
                false,
                false,
                false,
                false,
                true,
                8.,
                Some(border),
            ),
            PressIntent::BorderResize(_, ResizeEdge::LEFT)
        ));
        assert_eq!(
            classify_press(
                Some(MouseButton::Left),
                policy,
                false,
                false,
                false,
                false,
                false,
                false,
                true,
                8.,
                Some(border),
            ),
            PressIntent::Move {
                tiling: false,
                threshold: 0.,
            }
        );
        assert_eq!(
            classify_press(
                Some(MouseButton::Right),
                policy,
                false,
                false,
                false,
                false,
                false,
                false,
                true,
                8.,
                None,
            ),
            PressIntent::CornerResize
        );
    }

    /// A view in a floating group is floating-or-child but not floating in sway
    /// (sway/tree/container.c:1041-1050,1338-1340), so its own border takes the tiled
    /// resize branch (sway/input/seatop_default.c:396-411), while a modifier drag, a
    /// titlebar drag and a modifier resize act on the whole group
    /// (sway/input/seatop_default.c:459-487) without the tiling-drag threshold.
    #[test]
    fn classify_press_for_a_floating_group_child() {
        let mod_down = DragPolicy {
            mod_down: true,
            move_button: MouseButton::Left,
            resize_button: MouseButton::Right,
        };
        let plain = DragPolicy {
            mod_down: false,
            ..mod_down
        };
        let border = (Point::from((1., 2.)), ResizeEdge::RIGHT);
        let press = |button, policy, on_titlebar, tiling_drag, border| {
            classify_press(
                Some(button),
                policy,
                false,
                true,
                false,
                on_titlebar,
                false,
                false,
                tiling_drag,
                8.,
                border,
            )
        };
        let group_move = PressIntent::Move {
            tiling: false,
            threshold: 0.,
        };

        // An internal border resizes even with the modifier held.
        assert!(matches!(
            press(MouseButton::Left, mod_down, false, true, Some(border)),
            PressIntent::BorderResize(_, ResizeEdge::RIGHT)
        ));
        assert_eq!(
            press(MouseButton::Left, mod_down, false, true, None),
            group_move
        );
        // A titlebar drag moves the group with no modifier, and `tiling_drag no`
        // does not gate it (it gates only the tiled branch, seatop_default.c:490-492).
        assert_eq!(
            press(MouseButton::Left, plain, true, false, None),
            group_move
        );
        assert_eq!(
            press(MouseButton::Right, mod_down, false, true, None),
            PressIntent::CornerResize
        );
        assert_eq!(
            press(MouseButton::Left, plain, false, true, None),
            PressIntent::None
        );
    }

    #[test]
    fn floating_drag_policy_tracks_default_none_and_inverse() {
        let modifiers = Modifiers::SUPER;
        let default = floating_drag_policy(None, ModKey::Super, modifiers);
        assert!(default.mod_down);
        assert_eq!(default.move_button, MouseButton::Left);
        assert_eq!(default.resize_button, MouseButton::Right);

        let none = floating_drag_policy(
            Some(swayward_config::input::FloatingModifier {
                modifier: ModKey::None,
                inverse: false,
            }),
            ModKey::Super,
            modifiers,
        );
        assert!(!none.mod_down);

        let inverse = floating_drag_policy(
            Some(swayward_config::input::FloatingModifier {
                modifier: ModKey::Super,
                inverse: true,
            }),
            ModKey::Alt,
            modifiers,
        );
        assert!(inverse.mod_down);
        assert_eq!(inverse.move_button, MouseButton::Right);
        assert_eq!(inverse.resize_button, MouseButton::Left);
    }
}
