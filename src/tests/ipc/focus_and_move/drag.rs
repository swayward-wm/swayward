#[test]
fn popup_during_fullscreen_policies_use_xdg_toplevel_parent() {
    assert_eq!(
        fullscreen_parent_after_child_map(swayward_config::PopupDuringFullscreen::Smart),
        (true, true)
    );
    assert_eq!(
        fullscreen_parent_after_child_map(swayward_config::PopupDuringFullscreen::Ignore),
        (true, false)
    );
    assert_eq!(
        fullscreen_parent_after_child_map(swayward_config::PopupDuringFullscreen::LeaveFullscreen),
        (false, false)
    );
}

#[test]
fn dialog_placement_uses_parent_layout_position_during_animation() {
    assert_eq!(
        dialog_rect_after_parent_move(false),
        dialog_rect_after_parent_move(true)
    );
}

#[test]
fn disabled_focus_follows_mouse_keeps_focus_when_pointer_crosses_outputs() {
    let mut f = Fixture::new();
    f.add_output(1, (1024, 768));
    f.add_output(2, (1024, 768));
    let client = f.add_client();

    f.niri_focus_output(2);
    let focused = f.client(client).create_window();
    focused.commit();
    let surface = focused.surface.clone();
    f.roundtrip(client);
    let focused = f.client(client).window(&surface);
    focused.attach_new_buffer();
    focused.ack_last_and_commit();
    f.double_roundtrip(client);
    let focused_id = f.swayward().layout.focus().unwrap().id();

    let location = (500., 0.).into();
    let under = f.swayward().contents_under(location);
    f.swayward().handle_focus_follows_mouse(&under);
    f.niri_state().move_cursor(location);

    assert_eq!(f.swayward().layout.focus().unwrap().id(), focused_id);
    let active = f.swayward().layout.active_output().unwrap().clone();
    assert_eq!(active, f.niri_output(2));
    assert_eq!(
        f.swayward().seat.get_pointer().unwrap().current_location(),
        location
    );
}

/// Focus a second window by command with the pointer parked over the first,
/// then re-run the focus-follows-mouse hook without moving the pointer.
///
/// Sway's `yes` leaves focus alone because the hovered window did not change,
/// while `always` pulls focus back under the pointer
/// (`sway/sway/input/seatop_default.c:590-598`). Returns whether focus
/// returned to the hovered window.
fn focus_returns_under_stationary_pointer(
    mode: swayward_config::input::FocusFollowsMouseMode,
) -> bool {
    let mut config = swayward_config::Config::default();
    config.input.focus_follows_mouse = Some(swayward_config::input::FocusFollowsMouse {
        mode,
        max_scroll_amount: None,
    });
    config.animations.off = true;
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1280, 800));
    let client = f.add_client();

    // Two tiled windows side by side, so a point exists over each.
    let mut ids = Vec::new();
    for _ in 0..2 {
        let window = f.client(client).create_window();
        let surface = window.surface.clone();
        window.commit();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.set_size(100, 100);
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        // Commit the size the compositor asked for, so the surface has a
        // real hit-testable region.
        commit_configured_size(&mut f, client, &surface);
        let focus = f.swayward().layout.focus().unwrap();
        ids.push((focus.id(), focus.window.clone()));
    }
    let (first, first_window) = ids[0].clone();
    let (second, second_window) = ids[1].clone();

    // Find a point the compositor itself reports as over the first window,
    // rather than assuming a tiling geometry.
    let location = (0..128)
        .map(|step| {
            smithay::utils::Point::<f64, smithay::utils::Logical>::from((
                f64::from(step) * 10. + 5.,
                400.,
            ))
        })
        .find(|&point| {
            f.swayward()
                .contents_under(point)
                .window
                .is_some_and(|(window, _)| window == first_window)
        })
        .expect("no point over the first window");

    // Park the pointer over the first window; it becomes the hovered node.
    f.niri_state().move_cursor(location);

    // Move focus to the other window for a reason unrelated to the pointer,
    // which stays exactly where it is. This is sway's "focus got moved due to,
    // say, a workspace switch" case.
    f.swayward().layout.activate_window(&second_window);
    assert_eq!(f.swayward().layout.focus().unwrap().id(), second);

    // Re-run the hook with the pointer still over the first window. The
    // hovered node did not change, so only `always` acts.
    let under = f.swayward().contents_under(location);
    f.swayward().handle_focus_follows_mouse(&under);
    f.swayward().layout.focus().unwrap().id() == first
}

#[test]
fn focus_follows_mouse_always_refocuses_the_still_hovered_window() {
    use swayward_config::input::FocusFollowsMouseMode;

    // The two modes must disagree here. If they agreed, the assertion would
    // also pass against an implementation that stored `always` as `yes`.
    assert!(
        !focus_returns_under_stationary_pointer(FocusFollowsMouseMode::Yes),
        "`yes` must not re-focus a window the pointer never left"
    );
    assert!(
        focus_returns_under_stationary_pointer(FocusFollowsMouseMode::Always),
        "`always` must re-focus the hovered window after focus moved away"
    );
}

/// Warp policy applied to a focus change within the output the pointer is
/// already on.
///
/// `WARP_OUTPUT` returns early when the pointer already sits on the focused
/// output, so a same-output focus change does not move it. `WARP_CONTAINER`
/// warps to the focused container regardless
/// (`sway/sway/input/seat.c:1526-1547`). Returns whether the pointer moved.
fn pointer_moves_on_same_output_focus(mode: swayward_config::input::MouseWarping) -> bool {
    let mut config = swayward_config::Config::default();
    config.input.mouse_warping = mode;
    config.layout.gaps = 16.;
    config.animations.off = true;
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1280, 800));
    let client = f.add_client();

    for _ in 0..2 {
        let window = f.client(client).create_window();
        let surface = window.surface.clone();
        window.commit();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.set_size(100, 100);
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        commit_configured_size(&mut f, client, &surface);
    }

    // Put the pointer somewhere on the focused output that is not the centre
    // of the focused window.
    let start = smithay::utils::Point::<f64, smithay::utils::Logical>::from((5., 5.));
    f.niri_state().move_cursor(start);
    assert_eq!(
        f.swayward().seat.get_pointer().unwrap().current_location(),
        start
    );

    // A focus change that stays on this one output.
    assert!(crate::command::execute(f.niri_state(), "focus left")[0].success);
    f.niri_state().maybe_warp_cursor_to_focus();

    f.swayward().seat.get_pointer().unwrap().current_location() != start
}

/// What a modifier-held click on the given button starts.
///
/// Sway derives the move and resize buttons from the inverse bit, so `normal`
/// is left-move/right-resize and `inverse` swaps them
/// (`sway/sway/input/seatop_default.c:359-363`).
#[derive(Debug, PartialEq, Eq)]
enum DragKind {
    Nothing,
    Move,
    Resize,
}

fn drag_started_by(inverse: bool, button: u32) -> DragKind {
    let mut config = swayward_config::Config::default();
    config.input.floating_modifier = Some(swayward_config::input::FloatingModifier {
        modifier: swayward_config::input::ModKey::Super,
        inverse,
    });
    // Float the window: a lone tiled window has no neighbour to take space
    // from, so a tiling resize would refuse and hide the button mapping.
    config.window_rules.push(swayward_config::WindowRule {
        open_floating: Some(true),
        ..Default::default()
    });
    config.animations.off = true;
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1280, 800));
    let client = f.add_client();

    let window = f.client(client).create_window();
    let surface = window.surface.clone();
    // A floating window is configured with a zero size, meaning the client
    // picks, so ask for a concrete one.
    window.set_size(600, 400);
    window.commit();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.set_size(600, 400);
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    // A resize needs a resize edge under the cursor, so pick a point that is
    // both over a window and on one of its edges. Then the same position can
    // start either gesture and only the button decides which.
    let output = f.niri_output(1);
    let location = (0..1280)
        .map(|step| {
            smithay::utils::Point::<f64, smithay::utils::Logical>::from((f64::from(step), 400.))
        })
        .find(|&point| {
            f.niri_state().move_cursor(point);
            let over_window = f.swayward().window_under_cursor().is_some();
            let on_edge = f
                .swayward()
                .global_space
                .output_geometry(&output)
                .and_then(|geo| {
                    f.swayward()
                        .layout
                        .resize_edges_under(&output, point - geo.loc.to_f64())
                })
                .is_some_and(|edges| !edges.is_empty());
            over_window && on_edge
        })
        .expect("no point both over a window and on a resize edge");

    pointer_motion_absolute(&mut f, location.x, location.y);

    // Hold the floating modifier, then press the button.
    key_event(&mut f, 133, true);
    pointer_button(&mut f, button, true);

    // Inspect the concrete grab the press installed, which names the gesture
    // exactly rather than inferring it.
    let pointer = f.swayward().seat.get_pointer().unwrap();
    let kind = pointer
        .with_grab(|_, grab| {
            if grab.is::<crate::input::move_grab::MoveGrab>() {
                DragKind::Move
            } else if grab.is::<crate::input::resize_grab::ResizeGrab>() {
                DragKind::Resize
            } else {
                DragKind::Nothing
            }
        })
        .unwrap_or(DragKind::Nothing);

    pointer_button(&mut f, button, false);
    key_event(&mut f, 133, false);
    kind
}

#[test]
fn floating_modifier_inverse_swaps_the_move_and_resize_buttons() {
    const LEFT: u32 = 0x110;
    const RIGHT: u32 = 0x111;

    // normal: left moves, right resizes.
    assert_eq!(drag_started_by(false, LEFT), DragKind::Move);
    assert_eq!(drag_started_by(false, RIGHT), DragKind::Resize);
    // inverse: the two swap. If the inverse bit were dropped, these two
    // assertions would match the normal case above and fail.
    assert_eq!(drag_started_by(true, LEFT), DragKind::Resize);
    assert_eq!(drag_started_by(true, RIGHT), DragKind::Move);
}

fn tiled_drag_fixture() -> Fixture {
    let mut config = swayward_config::Config::default();
    config.window_rules.push(swayward_config::WindowRule {
        sway_border: Some(swayward_config::SwayWindowBorderStyle::Normal),
        ..Default::default()
    });
    config.animations.off = true;
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1280, 800));
    let client = f.add_client();
    let window = f.client(client).create_window();
    let surface = window.surface.clone();
    window.commit();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.set_size(100, 100);
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    commit_configured_size(&mut f, client, &surface);
    pointer_motion_absolute(&mut f, 640., 400.);
    f
}

fn tiled_titlebar_point(f: &mut Fixture) -> smithay::utils::Point<f64, smithay::utils::Logical> {
    let rect = f
        .swayward()
        .layout
        .active_workspace()
        .unwrap()
        .tiling()
        .titlebar_rects()[0]
        .1;
    smithay::utils::Point::from((rect.loc.x + rect.size.w / 2., rect.loc.y + rect.size.h / 2.))
}

fn move_grab_state(f: &mut Fixture) -> Option<bool> {
    f.swayward()
        .seat
        .get_pointer()
        .unwrap()
        .with_grab(|_, grab| {
            grab.downcast_ref::<crate::input::move_grab::MoveGrab>()
                .map(|grab| grab.is_move())
        })
        .flatten()
}

#[test]
fn tiling_drag_disabled_does_not_start_a_modifier_drag() {
    let mut f = tiled_drag_fixture();
    assert!(crate::command::execute(f.niri_state(), "tiling_drag no")[0].success);

    key_event(&mut f, 133, true);
    pointer_button(&mut f, 0x110, true);

    assert_eq!(move_grab_state(&mut f), None);
}

#[test]
fn tiling_drag_threshold_delays_a_titlebar_move() {
    let mut f = tiled_drag_fixture();
    assert!(crate::command::execute(f.niri_state(), "tiling_drag_threshold 100")[0].success);
    let start = tiled_titlebar_point(&mut f);
    pointer_motion_absolute(&mut f, start.x, start.y);
    pointer_button(&mut f, 0x110, true);
    pointer_motion_absolute(&mut f, start.x + 100., start.y);
    assert_eq!(move_grab_state(&mut f), Some(false));

    pointer_motion_absolute(&mut f, start.x + 101., start.y);
    assert_eq!(move_grab_state(&mut f), Some(true));
}

#[test]
fn tiling_drag_defaults_remain_enabled_with_a_nine_pixel_threshold() {
    let config = swayward_config::Config::default();
    assert!(config.input.tiling_drag);
    assert_eq!(config.input.tiling_drag_threshold, 9);

    let mut f = tiled_drag_fixture();
    let start = tiled_titlebar_point(&mut f);
    pointer_motion_absolute(&mut f, start.x, start.y);
    pointer_button(&mut f, 0x110, true);
    pointer_motion_absolute(&mut f, start.x + 9., start.y);
    assert_eq!(move_grab_state(&mut f), Some(false));
    pointer_motion_absolute(&mut f, start.x + 10., start.y);
    assert_eq!(move_grab_state(&mut f), Some(true));
}

#[test]
fn floating_modifier_none_disables_the_drag() {
    let mut config = swayward_config::Config::default();
    config.input.floating_modifier = Some(swayward_config::input::FloatingModifier {
        modifier: swayward_config::input::ModKey::None,
        inverse: false,
    });
    config.animations.off = true;
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1280, 800));
    let client = f.add_client();
    let window = f.client(client).create_window();
    let surface = window.surface.clone();
    window.commit();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.set_size(100, 100);
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    commit_configured_size(&mut f, client, &surface);

    pointer_motion_absolute(&mut f, 640., 400.);
    // Super is held, but `none` means no modifier can arm the drag.
    key_event(&mut f, 133, true);
    pointer_button(&mut f, 0x110, true);
    let pointer = f.swayward().seat.get_pointer().unwrap();
    let dragging = pointer
        .with_grab(|_, grab| {
            grab.is::<crate::input::move_grab::MoveGrab>()
                || grab.is::<crate::input::resize_grab::ResizeGrab>()
        })
        .unwrap_or(false);
    assert!(!dragging, "`floating_modifier none` must not start a drag");
    pointer_button(&mut f, 0x110, false);
    key_event(&mut f, 133, false);
}

#[test]
fn mouse_warping_output_warps_when_focus_crosses_outputs() {
    // The other half of `output`: the pointer is not on the newly focused
    // output, so the early return does not apply and it warps.
    let mut config = swayward_config::Config::default();
    config.input.mouse_warping = swayward_config::input::MouseWarping::Output;
    config.animations.off = true;
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1280, 800));
    f.add_output(2, (1280, 800));
    let client = f.add_client();

    // A window on the second output, with the pointer left on the first.
    f.niri_focus_output(2);
    let window = f.client(client).create_window();
    let surface = window.surface.clone();
    window.commit();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.set_size(100, 100);
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    commit_configured_size(&mut f, client, &surface);

    let start = smithay::utils::Point::<f64, smithay::utils::Logical>::from((5., 5.));
    f.niri_state().move_cursor(start);
    let first_output = f.niri_output(1);
    let second_output = f.niri_output(2);
    let output_at = |f: &mut Fixture, point| {
        f.swayward()
            .global_space
            .output_under(point)
            .next()
            .cloned()
    };
    assert_eq!(
        output_at(&mut f, start),
        Some(first_output),
        "the pointer should start on the output that is not focused"
    );

    f.niri_state().maybe_warp_cursor_to_focus();

    let moved = f.swayward().seat.get_pointer().unwrap().current_location();
    assert_ne!(moved, start, "`output` must warp across an output boundary");
    assert_eq!(
        output_at(&mut f, moved),
        Some(second_output),
        "the warp must land on the focused output"
    );
}

#[test]
fn mouse_warping_container_warps_within_an_output_and_output_does_not() {
    use swayward_config::input::MouseWarping;

    // The two modes must disagree here, otherwise the assertion would also
    // pass against an implementation that collapsed them to one boolean.
    assert!(
        !pointer_moves_on_same_output_focus(MouseWarping::Output),
        "`output` must leave the pointer alone while it is already on the focused output"
    );
    assert!(
        pointer_moves_on_same_output_focus(MouseWarping::Container),
        "`container` must warp to the focused container on a same-output focus change"
    );
    assert!(
        !pointer_moves_on_same_output_focus(MouseWarping::No),
        "`none` must never warp"
    );
}

#[test]
fn dialog_with_hidden_scratchpad_parent_does_not_panic() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let parent = f.client(client).create_window();
    let parent_surface = parent.surface.clone();
    let parent_toplevel = parent.xdg_toplevel.clone();
    parent.commit();
    f.roundtrip(client);
    let parent = f.client(client).window(&parent_surface);
    parent.attach_new_buffer();
    parent.ack_last_and_commit();
    f.double_roundtrip(client);
    assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);

    let child = f.client(client).create_window();
    child.set_parent(Some(&parent_toplevel));
    let child_surface = child.surface.clone();
    child.commit();
    f.roundtrip(client);
    let child = f.client(client).window(&child_surface);
    child.attach_new_buffer();
    child.ack_last_and_commit();
    f.double_roundtrip(client);

    assert_eq!(f.swayward().layout.windows().count(), 2);
}

#[test]
fn floating_rejects_hidden_scratchpad_window_without_panicking() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("hidden".into());
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);
    let outcome = crate::command::execute(f.niri_state(), r#"[app_id="hidden"] floating enable"#);
    assert_eq!(outcome.len(), 1);
    assert!(!outcome[0].success);
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("Can't change floating on hidden scratchpad container")
    );
    // Differential seeds 30470 and 30479 (v3): sway refuses with CMD_INVALID
    // (sway/commands/floating.c:35-38), so the reply sets `parse_error`.
    assert_eq!(outcome[0].parse_error, Some(true));
}

// Differential seed 31583 (v3): a view inside a hidden scratchpad group is
// not a scratchpad container, so sway walks to the group root, which already
// counts as floating, and `floating enable` succeeds without a change
// (sway/commands/floating.c:35-50, sway/tree/container.c:942-944).
#[test]
fn floating_enable_on_view_in_hidden_scratchpad_group_is_a_no_op() {
    let (mut f, _) = ipc_fixture();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    for app_id in ["one", "two"] {
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(app_id.into());
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    }
    for command in ["focus parent", "move scratchpad"] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success);
    }
    assert_eq!(f.swayward().layout.scratchpad_trees().count(), 1);
    let before = get_tree(&mut f);
    let outcome =
        crate::command::execute(f.niri_state(), r#"[app_id="^(one|two)$"] floating enable"#);
    assert_eq!(outcome.len(), 1);
    assert!(outcome[0].success, "{outcome:?}");
    assert_eq!(get_tree(&mut f), before);
    let disable = crate::command::execute(f.niri_state(), r#"[app_id="^two$"] floating disable"#);
    assert_eq!(disable[0].parse_error, Some(true), "{disable:?}");
}

/// Two views in a tabbed split marked `z`, the split focused. With `hide`
/// the split is hidden in the scratchpad as one group.
fn marked_tabbed_group(hide: bool) -> Fixture {
    let (mut f, _) = ipc_fixture();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    for app_id in ["one", "two"] {
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(app_id.into());
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    }
    let hide = hide.then_some("move scratchpad");
    for command in [
        "layout tabbed",
        "focus parent",
        "[con_id=__focused__] mark --add z",
    ]
    .into_iter()
    .chain(hide)
    {
        assert!(
            crate::command::execute(f.niri_state(), command)[0].success,
            "{command}"
        );
    }
    assert_eq!(
        f.swayward().layout.scratchpad_trees().count(),
        usize::from(hide.is_some())
    );
    f
}

/// What sway answers for the last command of a comma list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FloatingReply {
    Success,
    /// CMD_INVALID "Can't change floating on hidden scratchpad container"
    /// (sway/commands/floating.c:35-38).
    HiddenScratchpad,
}

// Sway resolves `floating`'s target from the live tree when the handler runs
// (sway/commands/floating.c:35-50), and a comma list keeps its criteria
// matches, so an earlier command in the list decides what `floating` sees:
//
// - A hidden group's root is the scratchpad container itself, so any mode is CMD_INVALID. A view
//   inside it is not (sway/tree/container.c:1696-1698): sway walks to the root, which counts as
//   floating, so `enable` is a no-op. `disable` and `toggle` then tile a root with no workspace and
//   crash sway 1.12, so swayward keeps refusing them (no sway reply to match).
// - `scratchpad show` puts the group on the workspace, so the root and its views are a plain
//   floating container (sway/commands/scratchpad.c:108-126).
// - `move scratchpad` on a tiled view hides that view alone; on the split it floats and hides the
//   whole group (sway/commands/move.c:935-947). Either way the matched container is now hidden.
//
// Oracle rows: state scenarios floating_hidden_scratchpad_is_invalid,
// floating_enable_in_hidden_scratchpad_group,
// floating_hidden_scratchpad_group_root_is_invalid,
// floating_hidden_scratchpad_group_root_after_show and
// floating_hidden_scratchpad_chain_matrix.
#[test]
fn floating_resolves_hidden_scratchpad_when_the_handler_runs() {
    use FloatingReply::{HiddenScratchpad, Success};
    let view = r#"[app_id="^two$"]"#;
    let root = r#"[con_mark="z"]"#;
    // (criteria, start hidden, prefix, mode, sway's reply to `floating`)
    #[rustfmt::skip]
    let matrix = [
        (view, true, "", "enable", Success),
        (view, true, "", "disable", HiddenScratchpad),
        (view, true, "", "toggle", HiddenScratchpad),
        (root, true, "", "enable", HiddenScratchpad),
        (root, true, "", "disable", HiddenScratchpad),
        (root, true, "", "toggle", HiddenScratchpad),
        (view, true, "scratchpad show", "enable", Success),
        (view, true, "scratchpad show", "disable", Success),
        (view, true, "scratchpad show", "toggle", Success),
        (root, true, "scratchpad show", "enable", Success),
        (root, true, "scratchpad show", "disable", Success),
        (root, true, "scratchpad show", "toggle", Success),
        (view, false, "move scratchpad", "enable", HiddenScratchpad),
        (view, false, "move scratchpad", "disable", HiddenScratchpad),
        (view, false, "move scratchpad", "toggle", HiddenScratchpad),
        (root, false, "move scratchpad", "enable", HiddenScratchpad),
        (root, false, "move scratchpad", "disable", HiddenScratchpad),
        (root, false, "move scratchpad", "toggle", HiddenScratchpad),
    ];
    for (criteria, hidden, prefix, mode, expected) in matrix {
        let mut f = marked_tabbed_group(hidden);
        let command = if prefix.is_empty() {
            format!("{criteria} floating {mode}")
        } else {
            format!("{criteria} {prefix}, floating {mode}")
        };
        let before = get_tree(&mut f);
        let replies = crate::command::execute(f.niri_state(), &command);
        let replies_len = usize::from(!prefix.is_empty()) + 1;
        assert_eq!(replies.len(), replies_len, "{command}: {replies:?}");
        assert!(
            replies[..replies_len - 1].iter().all(|reply| reply.success),
            "{command}: {replies:?}"
        );
        let last = replies.last().unwrap();
        match expected {
            Success => assert!(last.success, "{command}: {replies:?}"),
            HiddenScratchpad => {
                assert!(!last.success, "{command}: {replies:?}");
                assert_eq!(last.parse_error, Some(true), "{command}: {replies:?}");
                assert_eq!(
                    last.error.as_deref(),
                    Some("Can't change floating on hidden scratchpad container"),
                    "{command}"
                );
            }
        }
        let layout = &f.swayward().layout;
        let hidden_windows = layout.scratchpad_windows().count();
        match (prefix, criteria == root, mode) {
            // A refused or no-op `floating` leaves the hidden group alone.
            ("", ..) => assert_eq!(get_tree(&mut f), before, "{command}"),
            ("scratchpad show", ..) => assert_eq!(hidden_windows, 0, "{command}"),
            // The view alone, or the whole group, stays hidden.
            ("move scratchpad", false, _) => assert_eq!(hidden_windows, 1, "{command}"),
            ("move scratchpad", true, _) => {
                assert_eq!(hidden_windows, 2, "{command}");
                assert_eq!(layout.scratchpad_trees().count(), 1, "{command}");
            }
            _ => unreachable!(),
        }
    }
}

/// A floating group `r` = H[one, `z` = V[two, three]], built as sway builds
/// it from a workspace (`floating enable` on the workspace wraps its
/// children, sway/commands/floating.c:28-33). With `hide` the group is hidden
/// in the scratchpad.
fn marked_nested_group(hide: bool) -> Fixture {
    let (mut f, _) = ipc_fixture();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let open = |f: &mut Fixture, app_id: &str| {
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(app_id.into());
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    };
    open(&mut f, "one");
    open(&mut f, "two");
    assert!(crate::command::execute(f.niri_state(), "splitv")[0].success);
    open(&mut f, "three");
    let hide = hide.then_some("move scratchpad");
    for command in [
        "focus parent",
        "[con_id=__focused__] mark --add z",
        "focus parent",
        "floating enable",
        "[con_id=__focused__] mark --add r",
    ]
    .into_iter()
    .chain(hide)
    {
        assert!(
            crate::command::execute(f.niri_state(), command)[0].success,
            "{command}"
        );
    }
    let layout = &f.swayward().layout;
    assert_eq!(
        layout.scratchpad_trees().count(),
        usize::from(hide.is_some())
    );
    f
}

// The same handler-time resolution for every container a nested floating
// group holds: its root `r`, a non-root split `z` and a view inside `z`.
// Only the root is a scratchpad container; `z` and its views walk up to it
// (sway/commands/floating.c:35-46, sway/tree/container.c:1696-1698), and
// `move scratchpad` or `scratchpad show` on any of them acts on the whole
// group (sway/commands/move.c:935-947, sway/commands/scratchpad.c:108-126).
//
// Oracle rows: the chain matrix rows cited above, and for the split and view
// inside the group rv4_inner_split_move_enable, rv4_inner_split_hidden_enable
// and rv4_view_in_group_move_enable (sway 1.12-88869399: success).
#[test]
fn floating_resolves_nested_scratchpad_group_when_the_handler_runs() {
    use FloatingReply::{HiddenScratchpad, Success};
    let root = r#"[con_mark="r"]"#;
    let split = r#"[con_mark="z"]"#;
    let view = r#"[app_id="^three$"]"#;
    // A refused `disable`/`toggle` on a split or view inside a hidden group
    // crashes sway 1.12; swayward refuses it like the root.
    let inside = |mode: &str| match mode {
        "enable" => Success,
        _ => HiddenScratchpad,
    };
    for criteria in [root, split, view] {
        for mode in ["enable", "disable", "toggle"] {
            let hidden_reply = if criteria == root {
                HiddenScratchpad
            } else {
                inside(mode)
            };
            for (hidden, prefix, expected) in [
                (true, "", hidden_reply),
                (true, "scratchpad show", Success),
                (false, "move scratchpad", hidden_reply),
            ] {
                let mut f = marked_nested_group(hidden);
                let command = if prefix.is_empty() {
                    format!("{criteria} floating {mode}")
                } else {
                    format!("{criteria} {prefix}, floating {mode}")
                };
                let before = get_tree(&mut f);
                let replies = crate::command::execute(f.niri_state(), &command);
                let replies_len = usize::from(!prefix.is_empty()) + 1;
                assert_eq!(replies.len(), replies_len, "{command}: {replies:?}");
                assert!(
                    replies[..replies_len - 1].iter().all(|reply| reply.success),
                    "{command}: {replies:?}"
                );
                let last = replies.last().unwrap();
                match expected {
                    Success => assert!(last.success, "{command}: {replies:?}"),
                    HiddenScratchpad => {
                        assert!(!last.success, "{command}: {replies:?}");
                        assert_eq!(last.parse_error, Some(true), "{command}: {replies:?}");
                        assert_eq!(
                            last.error.as_deref(),
                            Some("Can't change floating on hidden scratchpad container"),
                            "{command}"
                        );
                    }
                }
                let layout = &f.swayward().layout;
                let hidden_windows = layout.scratchpad_windows().count();
                let groups = layout.scratchpad_trees().count();
                match prefix {
                    "" => assert_eq!(get_tree(&mut f), before, "{command}"),
                    "scratchpad show" => {
                        assert_eq!(hidden_windows, 0, "{command}");
                        // Shown, the group is a plain floating container:
                        // `enable` keeps it floating, `disable` and `toggle`
                        // tile all three views.
                        let tiled = f
                            .swayward()
                            .layout
                            .active_workspace()
                            .unwrap()
                            .tiling()
                            .tiles()
                            .count();
                        let want = if mode == "enable" { 0 } else { 3 };
                        assert_eq!(tiled, want, "{command}");
                    }
                    _ => {
                        assert_eq!((hidden_windows, groups), (3, 1), "{command}");
                    }
                }
            }
        }
    }
}

#[test]
fn resize_rejects_hidden_scratchpad_window_without_panicking() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("hidden".into());
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);
    let outcome = crate::command::execute(
        f.niri_state(),
        r#"[app_id="hidden"] resize grow width 10 px"#,
    );
    assert_eq!(outcome.len(), 1);
    assert!(!outcome[0].success);
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("Cannot resize a hidden scratchpad container")
    );
}

/// The initial configure carries no size, so a client maps at its own size and then
/// commits the tiled slot the compositor configures after mapping.
fn commit_configured_size(
    f: &mut Fixture,
    client: super::client::ClientId,
    surface: &wayland_client::protocol::wl_surface::WlSurface,
) {
    let window = f.client(client).window(surface);
    let size = window.configures_received.last().unwrap().1.size;
    window.set_size(size.0 as u16, size.1 as u16);
    window.ack_last_and_commit();
    f.double_roundtrip(client);
}
