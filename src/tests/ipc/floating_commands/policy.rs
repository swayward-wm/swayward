#[test]
fn runtime_command_refusals_are_sway_shaped() {
    let mut f = Fixture::new();
    assert_eq!(
        crate::command::execute(f.niri_state(), "urgent allow"),
        [swayward_ipc::CommandOutcome {
            success: false,
            error: Some("urgent allow|deny requires client urgency-request policy support".into()),
            parse_error: Some(true),
        }]
    );
}

#[test]
fn opacity_updates_focused_and_criteria_targeted_windows() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    let first = f.client(client).create_window();
    first.commit();
    let first_surface = first.surface.clone();
    f.roundtrip(client);
    let first = f.client(client).window(&first_surface);
    first.attach_new_buffer();
    first.ack_last_and_commit();
    f.double_roundtrip(client);

    let first_id = f.swayward().layout.focus().unwrap().id();
    assert!(crate::command::execute(f.niri_state(), "opacity 0.5")[0].success);
    assert_eq!(f.swayward().layout.focus().unwrap().command_opacity(), 0.5);
    assert!(crate::command::execute(f.niri_state(), "opacity plus 0.25")[0].success);
    assert_eq!(f.swayward().layout.focus().unwrap().command_opacity(), 0.75);
    let outcome = crate::command::execute(f.niri_state(), "opacity minus 1");
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("opacity value out of bounds")
    );
    assert_eq!(f.swayward().layout.focus().unwrap().command_opacity(), 0.75);

    let second = f.client(client).create_window();
    second.xdg_toplevel.set_app_id("opacity-target".into());
    second.commit();
    let second_surface = second.surface.clone();
    f.roundtrip(client);
    let second = f.client(client).window(&second_surface);
    second.attach_new_buffer();
    second.ack_last_and_commit();
    f.double_roundtrip(client);

    let second_id = f.swayward().layout.focus().unwrap().id();
    let outcome = crate::command::execute(
        f.niri_state(),
        r#"[app_id="^opacity-target$"] opacity set 0.4"#,
    );
    assert!(outcome[0].success, "{outcome:?}");
    let opacities = f
        .swayward()
        .layout
        .windows()
        .map(|(_, mapped)| (mapped.id(), mapped.command_opacity()))
        .collect::<Vec<_>>();
    assert!(opacities.contains(&(second_id, 0.4)));
    assert!(opacities.contains(&(first_id, 0.75)));
}

#[test]
fn shortcuts_inhibitor_disable_sets_future_policy_and_deactivates_current() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    let current = f.client(client).inhibit_shortcuts(&surface);
    f.double_roundtrip(client);
    assert_eq!(f.client(client).state.shortcut_inhibitor_events, [true]);

    let outcome = crate::command::execute(f.niri_state(), "shortcuts_inhibitor disable");
    assert!(outcome[0].success, "{outcome:?}");
    f.double_roundtrip(client);
    assert_eq!(
        f.client(client).state.shortcut_inhibitor_events,
        [true, false]
    );

    current.destroy();
    f.double_roundtrip(client);
    f.client(client).state.shortcut_inhibitor_events.clear();
    let future = f.client(client).inhibit_shortcuts(&surface);
    f.double_roundtrip(client);
    assert!(f.client(client).state.shortcut_inhibitor_events.is_empty());

    let outcome = crate::command::execute(f.niri_state(), "shortcuts_inhibitor enable");
    assert!(outcome[0].success, "{outcome:?}");
    f.double_roundtrip(client);
    assert!(f.client(client).state.shortcut_inhibitor_events.is_empty());

    future.destroy();
    f.double_roundtrip(client);
    let _enabled_future = f.client(client).inhibit_shortcuts(&surface);
    f.double_roundtrip(client);
    assert_eq!(f.client(client).state.shortcut_inhibitor_events, [true]);
}

/// Oracle: state scenario per_view_render_with_view; differential seeds 10018,
/// 10020, 10045 and 10058 (diff-fam-per-view-render-with-view). With a view
/// focused, sway stores both values on the view after its argument checks and
/// reports them in GET_TREE (`sway/sway/commands/allow_tearing.c:17-22`,
/// `max_render_time.c:9-31`, `sway/sway/ipc-json.c:609-611`). `toggle` parses
/// with `current` true, so it always clears the override.
#[test]
fn per_view_render_commands_store_their_value_on_the_view() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    fn find_view(node: &Value) -> Option<&Value> {
        if node["type"] == "con" && node["pid"].is_number() {
            return Some(node);
        }
        node["nodes"].as_array()?.iter().find_map(find_view)
    }
    let view = |f: &mut Fixture| {
        let tree = command_tree(f);
        let view = find_view(&tree).expect("one view");
        (
            view["allow_tearing"].as_bool().unwrap(),
            view["max_render_time"].as_i64().unwrap(),
        )
    };
    assert_eq!(view(&mut f), (false, 0));
    for (command, expected) in [
        ("allow_tearing yes", (true, 0)),
        ("max_render_time 5", (true, 5)),
        ("allow_tearing toggle", (false, 5)),
        ("allow_tearing enable", (true, 5)),
        ("allow_tearing no", (false, 5)),
        ("max_render_time off", (false, 0)),
    ] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(outcome[0].success, "{command}: {outcome:?}");
        assert_eq!(view(&mut f), expected, "{command}");
    }

    for (command, error) in [
        ("max_render_time 0", "Invalid max render time."),
        ("max_render_time 5ms", "Invalid max render time."),
        ("max_render_time", "Missing max render time argument."),
        (
            "allow_tearing",
            "Invalid allow_tearing command (expected at least 1 argument, got 0)",
        ),
    ] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(!outcome[0].success, "{command}");
        assert_eq!(outcome[0].error.as_deref(), Some(error), "{command}");
        assert_eq!(outcome[0].parse_error, Some(true), "{command}");
    }
    assert_eq!(view(&mut f), (false, 0));
}

/// Oracle: differential_seed_1003, 1019, 1119, 1121, 1244, 1275 and 1579
/// (diff-fam-no-view-error-precedence). These sway handlers check for a view,
/// or for opacity a container, before they read their value, so with no view
/// focused every well-formed value gets the precondition error, as CMD_INVALID
/// except for opacity's CMD_FAILURE (`sway/sway/commands/allow_tearing.c:12-15`,
/// `max_render_time.c:23-27`, `shortcuts_inhibitor.c:14-18`,
/// `inhibit_idle.c:13-17`, `opacity.c:15-18`).
#[test]
fn view_commands_check_for_a_view_before_their_value() {
    let cases = [
        (
            "allow_tearing no",
            "Tearing can only be allowed on views",
            true,
        ),
        (
            "allow_tearing yes",
            "Tearing can only be allowed on views",
            true,
        ),
        (
            "max_render_time 5",
            "Only views can have a max_render_time",
            true,
        ),
        (
            "max_render_time off",
            "Only views can have a max_render_time",
            true,
        ),
        (
            "shortcuts_inhibitor enable",
            "Only views can have shortcuts inhibitors",
            true,
        ),
        (
            "shortcuts_inhibitor disable",
            "Only views can have shortcuts inhibitors",
            true,
        ),
        (
            "inhibit_idle none",
            "Only views can have idle inhibitors",
            true,
        ),
        (
            "inhibit_idle visible",
            "Only views can have idle inhibitors",
            true,
        ),
        (
            "inhibit_idle bogus",
            "Only views can have idle inhibitors",
            true,
        ),
        ("opacity minus 0.2", "No current container", false),
        (
            "title_format %title",
            "Only valid containers can have a title_format",
            true,
        ),
        ("urgent toggle", "No current container", false),
        ("urgent enable", "No current container", false),
    ];
    let check = |f: &mut Fixture, context: &str| {
        for (command, error, parse_error) in cases {
            assert_eq!(
                crate::command::execute(f.niri_state(), command),
                [swayward_ipc::CommandOutcome {
                    success: false,
                    error: Some(error.into()),
                    parse_error: Some(parse_error),
                }],
                "{command} ({context})"
            );
        }
    };

    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    check(&mut f, "empty workspace");

    // `focus parent` from a lone window focuses the workspace, which is no
    // container in sway's handler context.
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
    check(&mut f, "workspace focused");

    // A focused split is a container but no view
    // (`sway/sway/commands/urgent.c:18-20`).
    assert!(crate::command::execute(f.niri_state(), "focus child")[0].success);
    crate::tests::windows::map_window(&mut f, client, Default::default());
    let outcomes = crate::command::execute(f.niri_state(), "splitv; focus parent");
    assert!(
        outcomes.iter().all(|outcome| outcome.success),
        "{outcomes:?}"
    );
    assert_eq!(
        crate::command::execute(f.niri_state(), "urgent toggle"),
        [swayward_ipc::CommandOutcome {
            success: false,
            error: Some("Only views can be urgent".into()),
            parse_error: Some(true),
        }]
    );
    assert!(crate::command::execute(f.niri_state(), "title_format %title")[0].success);
}

/// Oracle: state scenario chained-command-focus
/// (diff-fam-chained-command-focus). Sway sets the handler context for each
/// command of a list after the earlier ones have run
/// (`sway/sway/commands.c:288-293`), so a view check sees the focus that
/// `focus parent` or `focus child` left behind, in both directions.
#[test]
fn chained_commands_check_the_focus_left_by_earlier_commands() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    let refusal = |error: &str, parse_error| swayward_ipc::CommandOutcome {
        success: false,
        error: Some(error.into()),
        parse_error: Some(parse_error),
    };
    let ok = swayward_ipc::CommandOutcome {
        success: true,
        error: None,
        parse_error: None,
    };
    for (command, second) in [
        (
            "focus parent; allow_tearing yes",
            refusal("Tearing can only be allowed on views", true),
        ),
        (
            "focus parent; opacity bogus",
            refusal("No current container", false),
        ),
        (
            "focus parent; border pixel 3",
            refusal("Only views can have borders", true),
        ),
    ] {
        let outcomes = crate::command::execute(f.niri_state(), command);
        assert_eq!(outcomes, [ok.clone(), second], "{command}");
        assert!(crate::command::execute(f.niri_state(), "focus child")[0].success);
    }

    // `opacity bogus` resolves to CMD_FAILURE without a container, which
    // does not end the list (`sway/sway/commands.c:295-299`), so the
    // following `focus child` still runs and gives the view back.
    let outcomes =
        crate::command::execute(f.niri_state(), "focus parent; opacity bogus; focus child");
    assert_eq!(
        outcomes,
        [
            ok.clone(),
            refusal("No current container", false),
            ok.clone()
        ]
    );
    assert_eq!(
        crate::command::execute(f.niri_state(), "border pixel 3"),
        std::slice::from_ref(&ok),
        "focus child ran after the failure"
    );

    // The workspace is focused at the start; `focus child` gives the view
    // back before the view check runs.
    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
    for (command, second) in [
        ("focus child; border pixel 3", ok.clone()),
        (
            "focus child; opacity bogus",
            refusal("opacity float invalid", true),
        ),
    ] {
        let outcomes = crate::command::execute(f.niri_state(), command);
        assert_eq!(outcomes, [ok.clone(), second], "{command}");
        assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
    }
}

#[test]
fn create_output_adds_a_headless_output() {
    let (mut f, socket) = ipc_fixture();
    f.add_output(1, (1280, 720));
    let mut stream = UnixStream::connect(socket).unwrap();

    assert!(crate::command::execute(f.niri_state(), "create_output")[0].success);
    let outputs = query_ipc(&mut f, &mut stream, MessageType::GetOutputs);
    assert_eq!(
        outputs
            .as_array()
            .unwrap()
            .iter()
            .map(|output| output["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["headless-1", "headless-2"]
    );
    assert_eq!(
        f.niri_output(2).current_mode().unwrap().size,
        (1920, 1080).into()
    );

    let removed = f.niri_output(2);
    f.swayward().remove_output(&removed);
    assert!(crate::command::execute(f.niri_state(), "create_output")[0].success);
    let outputs = query_ipc(&mut f, &mut stream, MessageType::GetOutputs);
    assert_eq!(
        outputs
            .as_array()
            .unwrap()
            .iter()
            .map(|output| output["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["headless-1", "headless-3"]
    );
}

#[test]
fn create_output_rejects_unsupported_backends_without_changing_output_state() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let snapshot = |f: &mut Fixture| {
        let swayward = f.swayward();
        serde_json::json!({
            "outputs": describe_outputs(&swayward.layout, &swayward.global_space),
            "workspaces": describe_workspaces(&swayward.layout, &swayward.global_space),
        })
    };
    let before = snapshot(&mut f);

    let state = f.niri_state();
    let outcome = crate::command::create_output(None, &mut state.swayward);
    assert_eq!(
        outcome,
        swayward_ipc::CommandOutcome {
            success: false,
            error: Some("Can only create outputs for Wayland, X11 or headless backends".into()),
            parse_error: Some(false),
        }
    );
    assert_eq!(snapshot(&mut f), before);
}

#[test]
fn urgent_command_updates_a_hidden_scratchpad_window() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("hidden-urgent".into());
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);
    let other = f.client(client).create_window();
    other.commit();
    let other_surface = other.surface.clone();
    f.roundtrip(client);
    let other = f.client(client).window(&other_surface);
    other.attach_new_buffer();
    other.ack_last_and_commit();
    f.double_roundtrip(client);
    let outcome = crate::command::execute(f.niri_state(), "[app_id=hidden-urgent] urgent enable");
    assert!(outcome[0].success, "{outcome:?}");
    assert!(test_window_is_urgent(&mut f, "hidden-urgent"));
}

#[test]
fn urgent_command_changes_only_an_unfocused_selected_window() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();

    for app_id in ["target", "focused"] {
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

    let urgent = |f: &mut Fixture| {
        f.swayward()
            .layout
            .windows()
            .find(|(_, window)| {
                crate::utils::with_toplevel_role(window.toplevel(), |role| {
                    role.app_id.as_deref() == Some("target")
                })
            })
            .unwrap()
            .1
            .is_urgent()
    };

    for (command, expected) in [
        ("[app_id=target] urgent enable", true),
        ("[app_id=target] urgent toggle", false),
        ("[app_id=target] urgent toggle", true),
        ("[app_id=target] urgent disable", false),
    ] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(outcome[0].success, "{command}: {outcome:?}");
        assert_eq!(urgent(&mut f), expected, "{command}");
    }
}

/// Sway compares against the seat's focused container, which is the split
/// after `focus parent`, so the keyboard-focused view still becomes urgent
/// (`view_set_urgent`, sway/sway/tree/view.c:1209-1213).
#[test]
fn urgent_command_marks_the_focused_view_when_its_parent_is_focused() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();

    for app_id in ["first", "second"] {
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

    let outcome = crate::command::execute(f.niri_state(), "[app_id=second] urgent enable");
    assert!(outcome[0].success, "{outcome:?}");
    assert!(!test_window_is_urgent(&mut f, "second"));

    let outcome = crate::command::execute(f.niri_state(), "focus parent");
    assert!(outcome[0].success, "{outcome:?}");
    let outcome = crate::command::execute(f.niri_state(), "[app_id=second] urgent enable");
    assert!(outcome[0].success, "{outcome:?}");
    assert!(test_window_is_urgent(&mut f, "second"));
}

#[test]
fn scratchpad_show_remaps_floating_center_between_asymmetric_outputs() {
    let mut f = Fixture::new();
    f.add_named_output_at("left".into(), (683, 768), Some((0, 0)));
    f.add_named_output_at("right".into(), (1024, 768), Some((683, 0)));
    assert!(
        crate::command::execute(f.niri_state(), "focus output left, workspace left")[0].success
    );
    assert!(
        crate::command::execute(f.niri_state(), "focus output right, workspace right")[0].success
    );
    assert!(crate::command::execute(f.niri_state(), "workspace left")[0].success);

    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    let rect = |f: &mut Fixture| {
        let swayward = f.swayward();
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &Default::default(),
            &Default::default(),
        ))
        .unwrap();
        find_json_node(&tree, "floating_con", true).unwrap()["rect"].clone()
    };

    assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);
    assert!(crate::command::execute(f.niri_state(), "scratchpad show")[0].success);
    let window = f.client(client).window(&surface);
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    assert!(crate::command::execute(f.niri_state(), "move position 40 px 100 px")[0].success);
    let left = rect(&mut f);
    assert!(crate::command::execute(f.niri_state(), "scratchpad show")[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus output right")[0].success);
    assert!(crate::command::execute(f.niri_state(), "scratchpad show")[0].success);
    let right = rect(&mut f);
    assert_eq!(right["x"], 743, "left={left} right={right}");
    assert_ne!(right["x"], left["x"]);

    assert!(crate::command::execute(f.niri_state(), "move position 600 px 100 px")[0].success);
    assert!(crate::command::execute(f.niri_state(), "scratchpad show")[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus output left")[0].success);
    assert!(crate::command::execute(f.niri_state(), "scratchpad show")[0].success);
    let left_again = rect(&mut f);
    assert_eq!(left_again["x"], 400);
    assert_ne!(left_again["x"], right["x"]);
}

#[test]
fn initially_floating_window_uses_and_clamps_client_size() {
    fn mapped_rect(requested: (u16, u16), honor_requested_size: bool) -> Value {
        let mut config = swayward_config::Config::default();
        config.animations.off = true;
        config.window_rules.push(swayward_config::WindowRule {
            open_floating: Some(true),
            ..Default::default()
        });
        let mut f = Fixture::with_config(config);
        f.add_output(1, (1280, 800));
        let client = f.add_client();
        let window = f.client(client).create_window();
        window.set_size(requested.0, requested.1);
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.set_size(
            if honor_requested_size {
                requested.0
            } else {
                1280
            },
            if honor_requested_size {
                requested.1
            } else {
                800
            },
        );
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        let requested = f
            .swayward()
            .layout
            .focus()
            .unwrap()
            .expected_size()
            .unwrap();
        let window = f.client(client).window(&surface);
        window.set_size(
            requested.w.try_into().unwrap(),
            requested.h.try_into().unwrap(),
        );
        window.ack_last_and_commit();
        f.double_roundtrip(client);

        let swayward = f.swayward();
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &Default::default(),
            &Default::default(),
        ))
        .unwrap();
        find_json_node(&tree, "floating_con", false)
            .unwrap()
            .clone()
    }

    let requested = mapped_rect((400, 150), true);
    assert_eq!(requested["geometry"]["width"], 400);
    assert_eq!(requested["geometry"]["height"], 150);
    assert_eq!(requested["rect"]["width"], 400);
    assert_eq!(requested["rect"]["height"], 150);

    let clamped = mapped_rect((1600, 1000), true);
    assert_eq!(clamped["geometry"]["width"], 1600);
    assert_eq!(clamped["geometry"]["height"], 1000);
    assert_eq!(clamped["rect"]["width"], 1280);
    assert_eq!(clamped["rect"]["height"], 800);

    assert_ne!(
        mapped_rect((400, 150), false)["geometry"],
        requested["geometry"]
    );
}

#[test]
fn focused_split_rejects_border_and_resizes_as_one_container() {
    let mut f = Fixture::new();
    f.add_output(1, (1200, 800));
    let client = f.add_client();
    for app_id in ["outside", "upper", "lower"] {
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
    f.swayward().layout.nest_or_unnest_window_left(None);
    assert!(crate::command::execute(f.niri_state(), "move down")[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);

    let border = crate::command::execute(f.niri_state(), "border pixel 7");
    assert!(!border[0].success, "{border:?}");
    assert_eq!(
        border[0].error.as_deref(),
        Some("Only views can have borders")
    );

    let widths = |f: &mut Fixture| {
        f.niri_state().ipc_refresh_layout();
        let swayward = f.swayward();
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &swayward.marks_by_window,
            &swayward.marks_by_container,
        ))
        .unwrap();
        ["outside", "upper", "lower"].map(|app_id| {
            find_json_node_with_app_id(&tree, app_id).unwrap()["rect"]["width"]
                .as_i64()
                .unwrap()
        })
    };
    let before = widths(&mut f);
    let resize = crate::command::execute(f.niri_state(), "resize grow width 10 ppt");
    assert!(resize[0].success, "{resize:?}");
    let after = widths(&mut f);
    assert!(after[0] < before[0], "before={before:?} after={after:?}");
    assert!(after[1] > before[1], "before={before:?} after={after:?}");
    assert!(after[2] > before[2], "before={before:?} after={after:?}");

    assert!(crate::command::execute(f.niri_state(), "mark group")[0].success);
    let resize =
        crate::command::execute(f.niri_state(), "[con_mark=group] resize shrink width 5 ppt");
    assert!(resize[0].success, "{resize:?}");
    let shrunk = widths(&mut f);
    assert!(shrunk[0] > after[0], "after={after:?} shrunk={shrunk:?}");
    assert!(shrunk[1] < after[1], "after={after:?} shrunk={shrunk:?}");
    assert!(shrunk[2] < after[2], "after={after:?} shrunk={shrunk:?}");
}

#[test]
fn tiled_axis_resize_without_parallel_siblings_reports_failure() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 800));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    for command in ["resize grow width 10 px", "resize shrink height 10 px"] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert_eq!(
            outcome,
            [swayward_ipc::CommandOutcome {
                success: false,
                error: Some("Cannot resize any further".into()),
                parse_error: Some(true),
            }],
            "{command}"
        );
    }
}

/// Oracle: random seeds 169, 332, 346. A view mapped beside a fullscreen view
/// is never arranged, so its box stays zero-sized and sway refuses to take
/// any width from it (sway/commands/resize.c:113-120).
#[test]
fn tiled_resize_beside_a_view_mapped_under_fullscreen_reports_failure() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 800));
    let client = f.add_client();
    for index in 0..2 {
        if index == 1 {
            assert!(crate::command::execute(f.niri_state(), "fullscreen toggle")[0].success);
        }
        let window = f.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    }

    let outcome = crate::command::execute(f.niri_state(), "resize grow width 10 px");
    assert_eq!(
        outcome,
        [swayward_ipc::CommandOutcome {
            success: false,
            error: Some("Cannot resize any further".into()),
            parse_error: Some(true),
        }]
    );
}

#[test]
fn tiled_axis_resize_with_workspace_focus_reports_no_target() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 800));
    let client = f.add_client();
    for _ in 0..2 {
        let window = f.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    }
    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
    assert!(crate::command::execute(f.niri_state(), "layout tabbed")[0].success);

    let outcome = crate::command::execute(f.niri_state(), "resize shrink height 10 px");
    assert_eq!(
        outcome,
        [swayward_ipc::CommandOutcome {
            success: false,
            error: Some("Cannot resize nothing".into()),
            parse_error: Some(true),
        }]
    );
}

#[test]
fn tiled_resize_that_only_changes_an_ancestor_reports_failure() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 800));
    let client = f.add_client();
    assert!(crate::command::execute(f.niri_state(), "split v")[0].success);
    for _ in 0..2 {
        let window = f.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    }
    assert!(crate::command::execute(f.niri_state(), "split h")[0].success);

    let outcome = crate::command::execute(f.niri_state(), "resize grow up 10 px or 25 ppt");
    assert_eq!(
        outcome,
        [swayward_ipc::CommandOutcome {
            success: false,
            error: Some("Cannot resize any further".into()),
            parse_error: Some(true),
        }]
    );
}

#[test]
fn tiled_grow_at_workspace_edge_reports_failure() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 800));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    let outcome = crate::command::execute(f.niri_state(), "resize grow right 10 px");

    // sway answers CMD_INVALID here (sway/commands/resize.c:217,278), so the
    // reply carries parse_error = true.
    assert_eq!(
        outcome,
        [swayward_ipc::CommandOutcome {
            success: false,
            error: Some("Cannot resize any further".into()),
            parse_error: Some(true),
        }]
    );
}

/// Oracle: floating_group_root_resize. A floated split is itself the
/// floating container, so `resize grow|shrink` takes resize_adjust_floating
/// (px, keeping the centre for width/height and the far edge for left/up)
/// and `resize set` takes resize_set_floating (sway/commands/resize.c:180-230,
/// 341-401, 521-537). ppt alone is refused.
#[test]
fn resize_of_a_floating_group_root_resizes_the_group() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    for app_id in ["group-first", "group-second"] {
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
    for command in ["focus parent", "floating enable"] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(outcome[0].success, "{command}: {outcome:?}");
    }
    let group_rect = |f: &mut Fixture| {
        f.niri_state().ipc_refresh_layout();
        let swayward = f.swayward();
        let tree = describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &swayward.marks_by_window,
            &swayward.marks_by_container,
        );
        let group = tree
            .nodes
            .iter()
            .flat_map(|output| &output.nodes)
            .flat_map(|workspace| &workspace.floating_nodes)
            .find(|node| !node.nodes.is_empty())
            .unwrap();
        (
            group.rect.x,
            group.rect.y,
            group.rect.width,
            group.rect.height,
        )
    };
    let (x, y, w, h) = group_rect(&mut f);

    let run = |f: &mut Fixture, command: &str| {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(outcome[0].success, "{command}: {outcome:?}");
    };
    run(&mut f, "resize grow width 10 px");
    assert_eq!(group_rect(&mut f), (x - 5, y, w + 10, h));
    run(&mut f, "resize grow left 20 px");
    assert_eq!(group_rect(&mut f), (x - 25, y, w + 30, h));
    run(&mut f, "resize shrink height 30");
    assert_eq!(group_rect(&mut f), (x - 25, y + 15, w + 30, h - 30));

    let (x, y, w, h) = group_rect(&mut f);
    run(&mut f, "resize set 500 px 400 px");
    assert_eq!(
        group_rect(&mut f),
        (x - (500 - w) / 2, y - (400 - h) / 2, 500, 400)
    );

    let outcome = crate::command::execute(f.niri_state(), "resize grow width 5 ppt");
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("Floating containers cannot use ppt measurements")
    );
}

/// Oracle: floating_group_child_resize. A group child is not floating, so
/// `resize grow width 10 ppt` resizes it inside the group in ppt, like a
/// tiled child (`container_is_floating`, sway/commands/resize.c:523).
#[test]
fn ppt_resize_of_a_floating_group_child_resizes_inside_the_group() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    for app_id in ["group-first", "group-second"] {
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
    for command in [
        "focus parent",
        "floating enable",
        "focus child",
        "resize grow width 10 ppt",
    ] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(outcome[0].success, "{command}: {outcome:?}");
    }
    f.niri_state().ipc_refresh_layout();
    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let width = |app_id| {
        find_json_node_with_app_id(&tree, app_id).unwrap()["rect"]["width"]
            .as_i64()
            .unwrap()
    };
    assert!(width("group-second") > width("group-first"));
}

/// Random oracle seed 290: a group floated first and a window floated after it
/// share sway's one floating list, so GET_TREE lists the group below the window
/// and only the window is focused (workspace_add_floating appends,
/// sway/tree/workspace.c:961-971; sway/ipc-json.c:532-540).
#[test]
fn floating_nodes_list_groups_and_windows_in_one_stacking_order() {
    let (mut f, _) = ipc_fixture();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let map = |f: &mut Fixture| {
        let window = f.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    };
    map(&mut f);
    for command in ["focus parent", "floating toggle"] {
        let reply = crate::command::execute(f.niri_state(), command);
        assert!(reply[0].success, "{command}: {reply:?}");
    }
    map(&mut f);
    let reply = crate::command::execute(f.niri_state(), "floating toggle");
    assert!(reply[0].success, "{reply:?}");

    let tree: swayward_ipc::Node = serde_json::from_value(get_tree(&mut f)).unwrap();
    let floating = &tree.nodes[1].nodes[0].floating_nodes;
    assert_eq!(floating.len(), 2);
    assert!(
        !floating[0].nodes.is_empty(),
        "the older group is listed first"
    );
    assert!(
        floating[1].nodes.is_empty(),
        "the newer window is listed last"
    );
    assert!(!floating[0].focused && floating[0].nodes.iter().all(|node| !node.focused));
    assert!(floating[1].focused);
}

/// A window mapped while a floating group's child is focused joins that group
/// beside the child; with the group root itself focused it tiles instead
/// (`view_map`, sway/tree/view.c:849-901). Random oracle seeds 290 and 330.
#[test]
fn mapping_with_a_floating_group_child_focused_joins_the_group() {
    let (mut f, _) = ipc_fixture();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let map = |f: &mut Fixture| {
        let window = f.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    };
    let group_sizes = |f: &mut Fixture| {
        let tree: swayward_ipc::Node = serde_json::from_value(get_tree(f)).unwrap();
        let workspace = &tree.nodes[1].nodes[0];
        (
            workspace.nodes.len(),
            workspace
                .floating_nodes
                .iter()
                .map(|node| node.nodes.len())
                .collect::<Vec<_>>(),
        )
    };
    map(&mut f);
    map(&mut f);
    for command in ["focus parent", "floating enable"] {
        let reply = crate::command::execute(f.niri_state(), command);
        assert!(reply[0].success, "{command}: {reply:?}");
    }

    // The root is focused: the new window tiles.
    map(&mut f);
    assert_eq!(group_sizes(&mut f), (1, vec![2]));

    // A child is focused: the new window joins the group.
    for command in ["focus floating", "focus child"] {
        let reply = crate::command::execute(f.niri_state(), command);
        assert!(reply[0].success, "{command}: {reply:?}");
    }
    map(&mut f);
    assert_eq!(group_sizes(&mut f), (1, vec![3]));
}

/// Oracle: floating_group_child_move_direction. A floating group's child is
/// not floating, so `move left` moves it inside the group like a tiled child
/// instead of moving the whole group by pixels (`container_is_floating`,
/// sway/commands/move.c:326-330, 722-728).
#[test]
fn directional_move_of_a_floating_group_child_reorders_inside_the_group() {
    let (mut f, _) = ipc_fixture();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    for app_id in ["group-first", "group-second"] {
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
    for command in ["focus parent", "floating enable", "focus child"] {
        let reply = crate::command::execute(f.niri_state(), command);
        assert!(reply[0].success, "{command}: {reply:?}");
    }
    let focus = crate::command::execute(f.niri_state(), r#"[app_id="^group-second$"] focus"#);
    assert!(focus[0].success, "{focus:?}");
    let before: swayward_ipc::Node = serde_json::from_value(get_tree(&mut f)).unwrap();
    let group_rect = before.nodes[1].nodes[0].floating_nodes[0].rect;

    let reply = crate::command::execute(f.niri_state(), "move left");
    assert!(reply[0].success, "{reply:?}");

    let json = get_tree(&mut f);
    let tree: swayward_ipc::Node = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(
        tree.nodes[1].nodes[0].floating_nodes[0].rect, group_rect,
        "the group did not move"
    );
    let order = json["nodes"][1]["nodes"][0]["floating_nodes"][0]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| node["app_id"].as_str().unwrap_or_default().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(order, ["group-second", "group-first"]);
}

/// Random oracle seed 323: a fullscreen child of a floating group hides its
/// siblings in the group (`view_is_visible`, sway/tree/view.c:1187-1193).
#[test]
fn fullscreen_floating_group_child_hides_its_siblings() {
    let (mut f, _) = ipc_fixture();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    for app_id in ["group-first", "group-second"] {
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
    for command in [
        "focus parent",
        "floating enable",
        "focus child",
        r#"[app_id="^group-second$"] focus"#,
        "fullscreen toggle",
    ] {
        let reply = crate::command::execute(f.niri_state(), command);
        assert!(reply[0].success, "{command}: {reply:?}");
    }
    let tree = get_tree(&mut f);
    let visible = |app_id| {
        find_json_node_with_app_id(&tree, app_id).unwrap()["visible"]
            .as_bool()
            .unwrap()
    };
    assert!(visible("group-second"));
    assert!(!visible("group-first"));
}

/// Oracle: sticky_move_to_same_output_refused. A sticky floating container,
/// or a child of one, is on every workspace of its output, so moving it to a
/// workspace there is refused, the current one included
/// (sway/commands/move.c:498-511, 542-546).
#[test]
fn moving_a_sticky_floating_container_on_its_output_is_refused() {
    let (mut f, _) = ipc_fixture();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    for command in ["floating toggle", "sticky toggle"] {
        let reply = crate::command::execute(f.niri_state(), command);
        assert!(reply[0].success, "{command}: {reply:?}");
    }
    for command in [
        "move container to workspace 3",
        "move container to workspace 1",
    ] {
        let reply = crate::command::execute(f.niri_state(), command);
        assert_eq!(
            reply[0].error.as_deref(),
            Some("Can't move sticky container to another workspace on the same output"),
            "{command}"
        );
    }
    let swayward = f.swayward();
    let names = swayward
        .layout
        .workspaces()
        .filter_map(|(_, _, workspace)| workspace.sway_name())
        .collect::<Vec<_>>();
    assert_eq!(names, ["1"], "no workspace was created");
}

#[test]
fn directional_resize_of_a_floating_group_child_resizes_inside_the_group() {
    // A floating group's child is not itself floating, so sway resizes it like
    // a tiled child of the group (`container_is_floating` in
    // sway/commands/resize.c:523 selects resize_adjust_tiled). The first child
    // has no left sibling, so `grow left` cannot resize any further, while
    // `grow right` moves the shared edge with its sibling
    // (container_find_resize_parent, sway/commands/resize.c:44-63).
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    for app_id in ["group-first", "group-second"] {
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
    for command in ["focus parent", "floating enable", "focus child"] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(outcome[0].success, "{command}: {outcome:?}");
    }
    let focus = crate::command::execute(f.niri_state(), r#"[app_id="^group-first$"] focus"#);
    assert!(focus[0].success, "{focus:?}");

    let widths = |f: &mut Fixture| {
        f.niri_state().ipc_refresh_layout();
        let swayward = f.swayward();
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &swayward.marks_by_window,
            &swayward.marks_by_container,
        ))
        .unwrap();
        ["group-first", "group-second"].map(|app_id| {
            find_json_node_with_app_id(&tree, app_id).unwrap()["rect"]["width"]
                .as_i64()
                .unwrap()
        })
    };
    let before = widths(&mut f);

    for command in [
        "resize grow left 10 px",
        "resize shrink up 10 px",
        "resize grow down 10 px",
    ] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert_eq!(
            outcome,
            [swayward_ipc::CommandOutcome {
                success: false,
                error: Some("Cannot resize any further".into()),
                parse_error: Some(true),
            }],
            "{command}"
        );
    }
    assert_eq!(widths(&mut f), before);

    let grow = crate::command::execute(f.niri_state(), "resize grow right 10 px");
    assert!(grow[0].success, "{grow:?}");
    let after = widths(&mut f);
    assert!(after[0] > before[0], "before={before:?} after={after:?}");
    assert!(after[1] < before[1], "before={before:?} after={after:?}");

    let focus = crate::command::execute(f.niri_state(), r#"[app_id="^group-second$"] focus"#);
    assert!(focus[0].success, "{focus:?}");
    let grow = crate::command::execute(f.niri_state(), "resize grow left 10 px");
    assert!(grow[0].success, "{grow:?}");
    let left = widths(&mut f);
    assert!(left[1] > after[1], "after={after:?} left={left:?}");
    assert!(left[0] < after[0], "after={after:?} left={left:?}");
    f.swayward().layout.verify_invariants();
}

fn floating_group_with_focused_child(f: &mut Fixture) {
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    assert!(crate::command::execute(f.niri_state(), "splith")[0].success);
    for app_id in ["group-first", "group-second"] {
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
    for command in ["focus parent", "floating enable", "focus child"] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(outcome[0].success, "{command}: {outcome:?}");
    }
}

fn workspace_shapes(f: &mut Fixture) -> Vec<(String, Vec<String>, Vec<String>)> {
    f.niri_state().ipc_refresh_layout();
    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    fn shape(node: &serde_json::Value) -> String {
        match node["app_id"].as_str() {
            Some(app_id) => app_id.to_owned(),
            None => format!(
                "{}[{}]",
                node["layout"].as_str().unwrap_or_default(),
                node["nodes"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(shape)
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
        }
    }
    let mut workspaces = Vec::new();
    for output in tree["nodes"].as_array().unwrap() {
        for workspace in output["nodes"].as_array().into_iter().flatten() {
            if workspace["name"] == "__i3_scratch" {
                continue;
            }
            let shapes = |key: &str| {
                workspace[key]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(shape)
                    .collect::<Vec<_>>()
            };
            workspaces.push((
                workspace["name"].as_str().unwrap_or_default().to_owned(),
                shapes("nodes"),
                shapes("floating_nodes"),
            ));
        }
    }
    workspaces
}

#[test]
fn moving_a_floating_group_child_to_a_workspace_leaves_its_sibling_floating() {
    // Oracle scenario floating_group_child_workspace_move (sway-ipc-oracle 418bad7): sway
    // treats only the group root as floating (`container_is_floating`,
    // sway/tree/container.c:1041-1049), so the child moves as a tiled container.
    let mut f = Fixture::new();
    floating_group_with_focused_child(&mut f);
    assert!(crate::command::execute(f.niri_state(), "move container to workspace 2")[0].success);
    assert_eq!(
        workspace_shapes(&mut f),
        [
            (
                "1".to_owned(),
                vec![],
                vec!["splith[group-first]".to_owned()]
            ),
            ("2".to_owned(), vec!["group-second".to_owned()], vec![]),
        ]
    );
}

#[test]
fn floating_toggle_on_a_floating_group_child_tiles_the_whole_group() {
    // Oracle scenario floating_group_child_floating_toggle (sway-ipc-oracle 418bad7): sway
    // toggles the group root (sway/commands/floating.c:41-46) and re-adds it with
    // `workspace_add_tiling`, so the group stays a split under the workspace
    // (sway/tree/container.c:976-1003).
    let mut f = Fixture::new();
    floating_group_with_focused_child(&mut f);
    assert!(crate::command::execute(f.niri_state(), "floating toggle")[0].success);
    assert_eq!(
        workspace_shapes(&mut f),
        [(
            "1".to_owned(),
            vec!["splith[group-first group-second]".to_owned()],
            vec![]
        )]
    );
}

#[test]
fn inhibit_idle_updates_get_tree_user_policy_and_effective_state() {
    let (mut f, _) = ipc_fixture();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    windows::map_window(
        &mut f,
        client,
        windows::WindowSpec {
            app_id: Some("idle-policy"),
            ..Default::default()
        },
    );

    for (mode, effective) in [
        ("open", true),
        ("none", false),
        ("focus", true),
        ("visible", true),
        ("fullscreen", false),
    ] {
        let outcome = crate::command::execute(f.niri_state(), &format!("inhibit_idle {mode}"));
        assert!(outcome[0].success, "{mode}: {outcome:?}");
        let tree = get_tree(&mut f);
        let node = find_json_node_with_app_id(&tree, "idle-policy").unwrap();
        assert_eq!(node["idle_inhibitors"]["user"], mode);
        assert_eq!(node["inhibit_idle"], effective, "{mode}");
    }

    assert!(crate::command::execute(f.niri_state(), "fullscreen enable")[0].success);
    let tree = get_tree(&mut f);
    assert_eq!(
        find_json_node_with_app_id(&tree, "idle-policy").unwrap()["inhibit_idle"],
        true
    );
}

/// Random oracle seeds 180, 215, 294, 309, 383: `floating toggle` on a
/// fullscreen view keeps it fullscreen. `container_set_floating` detaches
/// and re-adds the container without touching `fullscreen_mode`, and
/// `workspace_add_floating` reinstates it as the workspace's fullscreen
/// (`container_handle_fullscreen_reparent`, sway/tree/container.c:941-975,
/// 1380-1391; sway/tree/workspace.c:961-972). The tiled sibling stays hidden
/// and keeps the half share it had before the fullscreen.
#[test]
fn floating_toggle_keeps_a_fullscreen_view_fullscreen() {
    let (mut f, _) = ipc_fixture();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    for app_id in ["fs-first", "fs-second"] {
        windows::map_window(
            &mut f,
            client,
            windows::WindowSpec {
                app_id: Some(app_id),
                ..Default::default()
            },
        );
    }
    for command in ["fullscreen toggle", "floating toggle"] {
        let reply = crate::command::execute(f.niri_state(), command);
        assert!(reply[0].success, "{command}: {reply:?}");
    }
    f.double_roundtrip(client);
    let tree = get_tree(&mut f);
    let second = find_json_node_with_app_id(&tree, "fs-second").unwrap();
    assert_eq!(second["type"], "floating_con");
    assert_eq!(second["fullscreen_mode"], 1);
    let first = find_json_node_with_app_id(&tree, "fs-first").unwrap();
    assert_eq!(first["visible"], false);
    assert_eq!(first["percent"], 0.5);

    // Toggling back tiles it again, still fullscreen.
    let reply = crate::command::execute(f.niri_state(), "floating toggle");
    assert!(reply[0].success, "{reply:?}");
    f.double_roundtrip(client);
    let tree = get_tree(&mut f);
    let second = find_json_node_with_app_id(&tree, "fs-second").unwrap();
    assert_eq!(second["type"], "con");
    assert_eq!(second["fullscreen_mode"], 1);
}

/// Random oracle seeds 180 and 294: a view floated while fullscreen is a
/// floating container, so `layout` refuses it
/// (`cmd_layout`, sway/commands/layout.c:129-132).
#[test]
fn layout_refuses_a_fullscreen_floating_view() {
    let (mut f, _) = ipc_fixture();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    for app_id in ["fs-first", "fs-second"] {
        windows::map_window(
            &mut f,
            client,
            windows::WindowSpec {
                app_id: Some(app_id),
                ..Default::default()
            },
        );
    }
    for command in ["fullscreen toggle", "floating toggle", "focus right"] {
        let reply = crate::command::execute(f.niri_state(), command);
        assert!(reply[0].success, "{command}: {reply:?}");
    }
    let reply = crate::command::execute(f.niri_state(), "layout tabbed");
    assert_eq!(
        reply[0].error.as_deref(),
        Some("Unable to change layout of floating windows")
    );
}

/// Random oracle seed 309: `fullscreen toggle` on a view floated while
/// fullscreen returns it to the floating layer, not to its old tiled slot
/// (`container_fullscreen_disable`, sway/tree/container.c).
#[test]
fn unfullscreen_of_a_floated_fullscreen_view_stays_floating() {
    let (mut f, _) = ipc_fixture();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    for app_id in ["fs-first", "fs-second"] {
        windows::map_window(
            &mut f,
            client,
            windows::WindowSpec {
                app_id: Some(app_id),
                ..Default::default()
            },
        );
    }
    for command in ["fullscreen toggle", "floating toggle", "fullscreen toggle"] {
        let reply = crate::command::execute(f.niri_state(), command);
        assert!(reply[0].success, "{command}: {reply:?}");
    }
    f.double_roundtrip(client);
    let tree = get_tree(&mut f);
    let second = find_json_node_with_app_id(&tree, "fs-second").unwrap();
    assert_eq!(second["type"], "floating_con");
    assert_eq!(second["fullscreen_mode"], 0);
    let first = find_json_node_with_app_id(&tree, "fs-first").unwrap();
    assert_eq!(first["visible"], true);
    assert_eq!(first["percent"], 1.0);
}

/// Oracle: state scenario error_precedence (diff-fam-v3-error-precedence,
/// random-v3 seeds 91 and 94). `urgent` with the workspace focused has no
/// container (`sway/sway/commands/urgent.c:15-17`), and `scratchpad show`
/// refuses an empty scratchpad before looking at criteria matches
/// (`sway/sway/commands/scratchpad.c:105-107`).
#[test]
fn error_precedence_matches_sway() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    windows_on_workspaces(&mut f, &[("1", "first"), ("1", "second")]);

    let refusal = |error: &str, parse_error| swayward_ipc::CommandOutcome {
        success: false,
        error: Some(error.into()),
        parse_error: Some(parse_error),
    };
    assert_eq!(
        crate::command::execute(f.niri_state(), "[app_id=second] scratchpad show"),
        [refusal("Scratchpad is empty", true)]
    );

    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
    let outcomes = crate::command::execute(f.niri_state(), "focus parent; mark m");
    assert!(outcomes[0].success, "{outcomes:?}");
    assert_eq!(outcomes[1], refusal("Only containers can have marks", true));
    assert_eq!(
        crate::command::execute(f.niri_state(), "urgent enable"),
        [refusal("No current container", false)]
    );
}
