#[test]
fn xdg_activation_policy_matches_sway_for_tokens_and_visibility() {
    use swayward_config::FocusOnWindowActivation::{Focus, None, Smart, Urgent};

    for (policy, focused_token, visible, expect_focus, expect_urgent) in [
        (Smart, true, true, true, false),
        (Smart, true, false, false, true),
        (Urgent, true, true, false, true),
        (Focus, true, false, true, false),
        (None, true, true, false, false),
        (Smart, false, true, false, true),
        (Urgent, false, true, false, true),
        (Focus, false, true, false, true),
        (None, false, true, false, false),
    ] {
        let config = swayward_config::Config {
            focus_on_window_activation: policy,
            ..Default::default()
        };
        let mut f = Fixture::with_config(config);
        f.add_output(1, (1920, 1080));
        let client = f.add_client();
        map_test_window(&mut f, client, "target");
        let target = f.swayward().layout.focus().unwrap().id();
        let target_surface = f.client(client).state.windows[0].surface.clone();
        if visible {
            map_test_window(&mut f, client, "other");
        } else {
            assert!(crate::command::execute(f.niri_state(), "workspace 2")[0].success);
            map_test_window(&mut f, client, "other");
        }
        let requesting_surface = focused_token.then_some(target_surface.clone());
        let token = f
            .client(client)
            .request_activation_token_with_focus(requesting_surface.as_ref());
        f.double_roundtrip(client);
        let token = token.lock().unwrap().take().unwrap();
        f.client(client).activate(token, &target_surface);
        f.double_roundtrip(client);

        assert_eq!(
            f.swayward().layout.focus().map(|w| w.id()) == Some(target),
            expect_focus,
            "policy={policy:?} focused_token={focused_token} visible={visible}"
        );
        assert_eq!(
            test_window_is_urgent(&mut f, "target"),
            expect_urgent,
            "policy={policy:?} focused_token={focused_token} visible={visible}"
        );
    }

    // Focus mode reveals a hidden scratchpad target before focusing it.
    let config = swayward_config::Config {
        focus_on_window_activation: Focus,
        ..Default::default()
    };
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    map_test_window(&mut f, client, "target");
    let target = f.swayward().layout.focus().unwrap().id();
    let target_window = f.swayward().layout.focus().unwrap().window.clone();
    let surface = f.client(client).state.windows[0].surface.clone();
    assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);
    let token = f
        .client(client)
        .request_activation_token_with_focus(Some(&surface));
    f.double_roundtrip(client);
    let token = token.lock().unwrap().take().unwrap();
    f.client(client).activate(token, &surface);
    f.double_roundtrip(client);
    assert_eq!(f.swayward().layout.focus().map(|w| w.id()), Some(target));
    assert!(!f.swayward().layout.is_scratchpad_hidden(&target_window));
}

#[test]
fn focusing_an_urgent_window_clears_urgency_immediately() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    for app_id in ["target", "focused"] {
        map_test_window(&mut f, client, app_id);
    }
    set_test_window_urgent(&mut f, "target");

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let workspaces = serde_json::to_value(describe_workspaces(
        &swayward.layout,
        &swayward.global_space,
    ))
    .unwrap();
    assert_eq!(
        find_json_node_with_app_id(&tree, "target").unwrap()["urgent"],
        true
    );
    assert!(workspaces
        .as_array()
        .unwrap()
        .iter()
        .any(|ws| ws["urgent"] == true));

    assert!(crate::command::execute(f.niri_state(), "[app_id=target] focus")[0].success);
    f.double_roundtrip(client);
    assert!(!test_window_is_urgent(&mut f, "target"));
    set_test_window_urgent(&mut f, "target");
    assert!(!test_window_is_urgent(&mut f, "target"));
}

#[test]
fn urgent_criteria_selects_windows_by_urgency_timestamp() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    for app_id in ["oldest", "latest", "focused"] {
        map_test_window(&mut f, client, app_id);
    }
    set_test_window_urgent_at(&mut f, "oldest", Duration::from_millis(1));
    set_test_window_urgent_at(&mut f, "latest", Duration::from_millis(2));

    assert!(crate::command::execute(f.niri_state(), "[urgent=oldest] focus")[0].success);
    f.double_roundtrip(client);
    assert!(!test_window_is_urgent(&mut f, "oldest"));
    assert!(crate::command::execute(f.niri_state(), "[urgent=latest] focus")[0].success);
    f.double_roundtrip(client);
    assert!(!test_window_is_urgent(&mut f, "latest"));
}

#[test]
fn cross_workspace_focus_delays_urgency_clear_without_restarting_timer() {
    let config = swayward_config::Config {
        urgent_timeout_ms: swayward_config::UrgentTimeout(60_000),
        ..Default::default()
    };
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    map_test_window(&mut f, client, "target");
    assert!(crate::command::execute(f.niri_state(), "workspace 2")[0].success);
    map_test_window(&mut f, client, "other");
    set_test_window_urgent(&mut f, "target");

    assert!(crate::command::execute(f.niri_state(), "[app_id=target] focus")[0].success);
    f.double_roundtrip(client);
    assert!(test_window_is_urgent(&mut f, "target"));
    let target_id = f
        .swayward()
        .layout
        .windows()
        .find_map(|(_, window)| {
            crate::utils::with_toplevel_role(window.toplevel(), |role| {
                (role.app_id.as_deref() == Some("target")).then_some(window.id())
            })
        })
        .unwrap();
    let first_timer = *f.swayward().urgency_timers.get(&target_id).unwrap();
    assert!(crate::command::execute(f.niri_state(), "workspace 2")[0].success);
    assert!(crate::command::execute(f.niri_state(), "[app_id=target] focus")[0].success);
    f.double_roundtrip(client);
    assert_eq!(
        f.swayward().urgency_timers.get(&target_id),
        Some(&first_timer)
    );
    assert!(f.swayward().fire_urgency_timer_for_test(target_id));
    assert!(!test_window_is_urgent(&mut f, "target"));
}

#[test]
fn closing_a_window_cancels_its_pending_urgency_timer() {
    let config = swayward_config::Config {
        urgent_timeout_ms: swayward_config::UrgentTimeout(60_000),
        ..Default::default()
    };
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    map_test_window(&mut f, client, "target");
    let target_surface = f.client(client).state.windows[0].surface.clone();
    assert!(crate::command::execute(f.niri_state(), "workspace 2")[0].success);
    map_test_window(&mut f, client, "other");
    set_test_window_urgent(&mut f, "target");
    assert!(crate::command::execute(f.niri_state(), "[app_id=target] focus")[0].success);
    f.double_roundtrip(client);
    assert_eq!(f.swayward().urgency_timers.len(), 1);

    f.client(client).window(&target_surface).attach_null();
    f.client(client).window(&target_surface).commit();
    f.double_roundtrip(client);
    assert!(f.swayward().urgency_timers.is_empty());
}

#[test]
fn targeted_focus_selects_the_requested_unfocused_window() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();

    for app_id in ["first", "middle", "last"] {
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

    assert!(crate::command::execute(f.niri_state(), r#"[app_id="middle"] focus"#)[0].success);

    let focused_app_id = f.swayward().layout.focus().map(|window| {
        crate::utils::with_toplevel_role(window.toplevel(), |role| role.app_id.clone())
    });
    assert_eq!(focused_app_id, Some(Some("middle".into())));
}

#[test]
fn workspace_auto_back_and_forth_honors_global_and_command_settings() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    f.swayward()
        .config
        .borrow_mut()
        .input
        .workspace_auto_back_and_forth = true;

    for workspace in ["1", "2"] {
        assert!(
            crate::command::execute(f.niri_state(), &format!("workspace {workspace}"))[0].success
        );
    }

    // Enabled + active target + previous workspace: bounce.
    assert!(crate::command::execute(f.niri_state(), "workspace 2")[0].success);
    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().number(),
        Some(1)
    );

    // A different target switches normally instead of bouncing.
    assert!(crate::command::execute(f.niri_state(), "workspace 2")[0].success);
    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().number(),
        Some(2)
    );

    // The command-level override suppresses the enabled global option.
    assert!(
        crate::command::execute(f.niri_state(), "workspace --no-auto-back-and-forth 2",)[0].success
    );
    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().number(),
        Some(2)
    );

    // With the option disabled, selecting the active workspace remains there.
    f.swayward()
        .config
        .borrow_mut()
        .input
        .workspace_auto_back_and_forth = false;
    assert!(crate::command::execute(f.niri_state(), "workspace 1")[0].success);
    assert!(crate::command::execute(f.niri_state(), "workspace 1")[0].success);
    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().number(),
        Some(1)
    );
}

#[test]
fn move_no_auto_back_and_forth_changes_the_same_workspace_destination() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    f.swayward()
        .config
        .borrow_mut()
        .input
        .workspace_auto_back_and_forth = true;

    assert!(crate::command::execute(f.niri_state(), "workspace 1")[0].success);
    assert!(crate::command::execute(f.niri_state(), "workspace 2")[0].success);
    assert!(crate::command::execute(f.niri_state(), "workspace 1")[0].success);

    let client = f.add_client();
    for app_id in ["normal", "suppressed"] {
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

    assert!(
        crate::command::execute(f.niri_state(), r#"[app_id="normal"] move workspace 1"#)[0].success
    );
    assert!(
        crate::command::execute(
            f.niri_state(),
            r#"[app_id="suppressed"] move --no-auto-back-and-forth window to workspace 1"#,
        )[0]
        .success
    );

    let workspace_apps = f
        .swayward()
        .layout
        .workspaces()
        .filter_map(|(_, _, workspace)| {
            workspace.number().map(|number| {
                let apps = workspace
                    .windows()
                    .filter_map(|window| {
                        crate::utils::with_toplevel_role(window.toplevel(), |role| {
                            role.app_id.clone()
                        })
                    })
                    .collect::<Vec<_>>();
                (number, apps)
            })
        })
        .collect::<std::collections::HashMap<_, _>>();
    assert_eq!(workspace_apps[&2], ["normal"]);
    assert_eq!(workspace_apps[&1], ["suppressed"]);
}

/// Sway creates every workspace on the first resolving output its config
/// assigns, else on the focused output (workspace_get_initial_output,
/// sway/tree/workspace.c:153-175). Oracle scenarios
/// move_to_assigned_workspace_creates_on_its_output,
/// assign_rule_creates_workspace_on_its_output and
/// switch_to_assigned_workspace_focuses_its_output.
#[test]
fn workspace_output_assignment_names_are_case_sensitive() {
    let config = swayward_config::Config::parse_mem(
        r#"
workspace "5" { sway-output-assignment "headless-1"; }
workspace "Web" { sway-output-assignment "headless-1"; }
"#,
    )
    .unwrap();
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1280, 720));
    f.add_output(2, (1280, 720));
    assert!(crate::command::execute(f.niri_state(), "focus output headless-2")[0].success);
    assert!(crate::command::execute(f.niri_state(), "workspace web")[0].success);

    let swayward = f.swayward();
    let workspaces = describe_workspaces(&swayward.layout, &swayward.global_space);
    let web = workspaces
        .iter()
        .find(|workspace| workspace.name == "web")
        .unwrap();
    assert_eq!(web.output, "headless-2");
}

#[test]
fn new_workspaces_are_created_on_their_assigned_output() {
    for command in [
        "move container to workspace 7",
        r#"assign [app_id="^assigned$"] workspace 7"#,
        "workspace 7",
    ] {
        // headless-2 takes the first assigned name, 5, as its initial
        // workspace (workspace_next_name, sway/tree/workspace.c:436), so 7
        // does not exist until the command creates it.
        let config = swayward_config::Config::parse_mem(
            r#"
workspace "5" { sway-output-assignment "headless-2"; }
workspace "7" { sway-output-assignment "headless-2"; }
"#,
        )
        .unwrap();
        let mut f = Fixture::with_config(config);
        f.add_output(1, (1280, 720));
        f.add_output(2, (1280, 720));
        let client = f.add_client();
        let focus = crate::command::execute(f.niri_state(), "focus output headless-1");
        assert!(focus[0].success, "{focus:?}");
        assert!(
            !f.swayward()
                .layout
                .workspaces()
                .any(|(_, _, workspace)| workspace.sway_name().as_deref() == Some("7")),
            "workspace 7 must not exist before {command}"
        );
        let moved = if command.starts_with("assign") {
            let outcome = crate::command::execute(f.niri_state(), command);
            assert!(outcome[0].success, "{command}: {outcome:?}");
            map_test_window(&mut f, client, "assigned");
            "assigned"
        } else {
            map_test_window(&mut f, client, "first");
            let outcome = crate::command::execute(f.niri_state(), command);
            assert!(outcome[0].success, "{command}: {outcome:?}");
            if command == "workspace 7" {
                map_test_window(&mut f, client, "second");
                "second"
            } else {
                "first"
            }
        };

        let swayward = f.swayward();
        let workspaces = describe_workspaces(&swayward.layout, &swayward.global_space);
        let seven = workspaces
            .iter()
            .find(|workspace| workspace.name == "7")
            .unwrap_or_else(|| panic!("{command}: no workspace 7"));
        assert_eq!(seven.output, "headless-2", "{command}");
        let focused = workspaces
            .iter()
            .find(|workspace| workspace.focused)
            .unwrap();
        let expected_focus = if command == "workspace 7" {
            "headless-2"
        } else {
            "headless-1"
        };
        assert_eq!(focused.output, expected_focus, "{command}");
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &swayward.marks_by_window,
            &swayward.marks_by_container,
        ))
        .unwrap();
        let holder = tree["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|output| output["nodes"].as_array().unwrap())
            .find(|workspace| find_json_node_with_app_id(workspace, moved).is_some())
            .unwrap();
        assert_eq!(holder["name"], "7", "{command}");
    }
}

#[test]
fn a_split_holding_an_urgent_view_reports_urgent() {
    // differential seeds 1455 1847 2026 2398 2895: sway reports a split urgent
    // when a view below it is (`container_has_urgent_child`,
    // sway/sway/ipc-json.c:728-730).
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    map_test_window(&mut f, client, "target");
    assert!(crate::command::execute(f.niri_state(), "layout stacking")[0].success);
    map_test_window(&mut f, client, "focused");
    f.double_roundtrip(client);

    let split_urgency = |f: &mut Fixture| {
        let swayward = f.swayward();
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &swayward.marks_by_window,
            &swayward.marks_by_container,
        ))
        .unwrap();
        let mut found = vec![];
        fn walk(node: &serde_json::Value, found: &mut Vec<bool>) {
            if node["type"] == "con" && node["window"].is_null() && node["app_id"].is_null() {
                found.push(node["urgent"].as_bool().unwrap());
            }
            for child in node["nodes"].as_array().into_iter().flatten() {
                walk(child, found);
            }
        }
        walk(&tree, &mut found);
        found
    };
    assert_eq!(split_urgency(&mut f), [false]);

    let outcome = crate::command::execute(f.niri_state(), "[app_id=target] urgent enable");
    assert!(outcome[0].success, "{outcome:?}");
    assert_eq!(split_urgency(&mut f), [true]);

    let outcome = crate::command::execute(f.niri_state(), "[app_id=target] urgent disable");
    assert!(outcome[0].success, "{outcome:?}");
    assert_eq!(split_urgency(&mut f), [false]);
}

/// Sway records the previous workspace whenever the seat's focused workspace
/// changes (`set_workspace`, sway/sway/input/seat.c:1098-1113), so focusing
/// another output gives `back_and_forth` a target across outputs. Oracle
/// scenario workspace_back_and_forth_after_focus_output.
#[test]
fn back_and_forth_returns_across_outputs_after_focus_output() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    f.add_output(2, (1280, 720));
    let focused = |f: &mut Fixture| {
        let swayward = f.swayward();
        describe_workspaces(&swayward.layout, &swayward.global_space)
            .into_iter()
            .find(|workspace| workspace.focused)
            .map(|workspace| (workspace.name, workspace.output))
            .unwrap()
    };
    assert!(crate::command::execute(f.niri_state(), "focus output headless-1")[0].success);
    let first = focused(&mut f);
    assert!(crate::command::execute(f.niri_state(), "focus output headless-2")[0].success);
    let second = focused(&mut f);
    assert_ne!(first, second);

    assert!(crate::command::execute(f.niri_state(), "workspace back_and_forth")[0].success);
    assert_eq!(focused(&mut f), first);
    assert!(crate::command::execute(f.niri_state(), "workspace back_and_forth")[0].success);
    assert_eq!(focused(&mut f), second);
}

/// `workspace_auto_back_and_forth` bounces to the seat's previous workspace
/// even when it sits on another output (`workspace_auto_back_and_forth`,
/// sway/sway/tree/workspace.c:710-728, reading the seat-wide
/// `prev_workspace_name`). Oracle scenario
/// workspace_auto_back_and_forth_two_outputs; random-v3 seed 30154.
#[test]
fn auto_back_and_forth_returns_to_the_previous_workspace_on_another_output() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    f.add_output(2, (1280, 720));
    let focused = |f: &mut Fixture| {
        let swayward = f.swayward();
        describe_workspaces(&swayward.layout, &swayward.global_space)
            .into_iter()
            .find(|workspace| workspace.focused)
            .map(|workspace| (workspace.name, workspace.output))
            .unwrap()
    };
    assert!(crate::command::execute(f.niri_state(), "focus output headless-1")[0].success);
    let first = focused(&mut f);
    assert_eq!(first.0, "1");
    assert!(crate::command::execute(f.niri_state(), "workspace next")[0].success);
    let second = focused(&mut f);
    assert_ne!(first.1, second.1);
    assert!(crate::command::execute(f.niri_state(), "workspace 1")[0].success);
    assert_eq!(focused(&mut f), first);

    let outcome = crate::command::execute(
        f.niri_state(),
        "workspace_auto_back_and_forth yes; workspace 1",
    );
    assert!(outcome.iter().all(|outcome| outcome.success), "{outcome:?}");
    assert_eq!(focused(&mut f), second);
}

/// `move <direction>` across outputs never calls `seat_set_focus`, so the seat
/// keeps its workspace and `prev_workspace_name` (sway/sway/commands/move.c:
/// 277-298, 672-745; sway/sway/input/seat.c:1098-1113). `move container to
/// workspace back_and_forth` then targets the workspace the view just reached
/// and leaves it there. A command that changes no seat focus in between, such
/// as `mark`, does not catch the seat up. Oracle scenario
/// move_back_and_forth_after_cross_output_move; random-v3 seed 32806.
#[test]
fn move_back_and_forth_after_cross_output_move_keeps_the_seat_record() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    f.add_output(2, (1280, 720));
    let client = f.add_client();
    run_urgency_commands(
        &mut f,
        client,
        &[
            "@moved",
            "focus output right",
            "focus output left",
            "move right",
            "mark --add x",
            "move container to workspace back_and_forth",
        ],
    );
    let swayward = f.swayward();
    let workspaces = describe_workspaces(&swayward.layout, &swayward.global_space);
    let focused = workspaces
        .iter()
        .find(|workspace| workspace.focused)
        .unwrap();
    assert_eq!(focused.name, "2", "{workspaces:?}");
    let holding = swayward
        .layout
        .workspaces()
        .find(|(_, _, workspace)| workspace.windows().next().is_some())
        .and_then(|(_, _, workspace)| workspace.sway_name());
    assert_eq!(holding.as_deref(), Some("2"));
}

/// `move [absolute] position` re-homes a floater onto the workspace of the
/// output under it through `container_floating_move_to`, which never calls
/// `seat_set_focus` (sway/sway/tree/container.c:1113-1145; sway/sway/commands/
/// move.c:775, 831, 917). The seat keeps its workspace and
/// `prev_workspace_name`, so `move container to workspace back_and_forth`
/// targets the workspace the floater just reached and leaves it there.
/// Oracle scenario move_back_and_forth_after_cross_output_floating_move.
#[test]
fn move_back_and_forth_after_cross_output_floating_move_keeps_the_seat_record() {
    for position in [
        "move absolute position 1500 px 100 px",
        "move position 1500 px 100 px",
    ] {
        let mut f = Fixture::new();
        f.add_output(1, (1280, 720));
        f.add_output(2, (1280, 720));
        let client = f.add_client();
        run_urgency_commands(
            &mut f,
            client,
            &[
                "@moved",
                "focus output right",
                "focus output left",
                "floating enable",
                position,
                "move container to workspace back_and_forth",
            ],
        );
        let swayward = f.swayward();
        let holding = swayward
            .layout
            .workspaces()
            .find(|(_, _, workspace)| workspace.windows().next().is_some())
            .and_then(|(_, _, workspace)| workspace.sway_name());
        assert_eq!(holding.as_deref(), Some("2"), "{position}");
    }
}

fn run_urgency_commands(f: &mut Fixture, client: crate::tests::client::ClientId, steps: &[&str]) {
    for step in steps {
        if let Some(app_id) = step.strip_prefix('@') {
            map_test_window(f, client, app_id);
        } else {
            let outcome = crate::command::execute(f.niri_state(), step);
            assert!(outcome.iter().all(|o| o.success), "{step}: {outcome:?}");
            f.double_roundtrip(client);
        }
    }
}

/// Sway starts the urgency timer only when the focused view's output showed
/// another workspace before the focus change (`last_workspace`,
/// sway/sway/input/seat.c:1158-1161, 1223-1240), not when the previously
/// focused view sat elsewhere. Focusing across outputs onto a visible
/// workspace clears at once. Differential seed 31799 (random-v3).
#[test]
fn focus_onto_another_outputs_visible_workspace_clears_urgency_at_once() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    f.add_output(2, (1280, 720));
    let client = f.add_client();
    run_urgency_commands(
        &mut f,
        client,
        &[
            "@left",
            "focus output right",
            "@right",
            "focus output left",
            "[app_id=right] urgent enable",
        ],
    );
    assert!(test_window_is_urgent(&mut f, "right"));
    run_urgency_commands(&mut f, client, &["focus right"]);
    assert!(f.swayward().urgency_timers.is_empty());
    assert!(!test_window_is_urgent(&mut f, "right"));
}

/// Focusing a view from an empty workspace on the same output switches that
/// output's workspace, so sway starts the timer and the view stays urgent
/// for now, even with no previously focused view. Differential seed 32407
/// (random-v3).
#[test]
fn focus_from_an_empty_workspace_on_the_same_output_starts_the_urgency_timer() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    run_urgency_commands(
        &mut f,
        client,
        &[
            "@target",
            "workspace number 3",
            "[app_id=target] urgent enable",
            "[app_id=target] focus",
        ],
    );
    assert_eq!(f.swayward().urgency_timers.len(), 1);
    assert!(test_window_is_urgent(&mut f, "target"));
}

/// `split v` with the workspace focused moves seat focus to the new
/// container, not to a view (`workspace_split`,
/// sway/sway/tree/workspace.c:1071-1076), so an urgent view under it stays
/// urgent. Differential seed 32574 (random-v3).
#[test]
fn splitting_a_focused_workspace_with_a_floater_keeps_urgency() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    f.add_output(2, (1280, 720));
    let client = f.add_client();
    run_urgency_commands(
        &mut f,
        client,
        &[
            "focus output right",
            "@two",
            "@four",
            "focus output left",
            "[app_id=four] urgent enable",
            "@twelve",
            "focus next",
            "floating enable",
            "focus parent",
        ],
    );
    assert!(test_window_is_urgent(&mut f, "four"));
    run_urgency_commands(&mut f, client, &["split v"]);
    assert!(test_window_is_urgent(&mut f, "four"));
}

/// A sticky floater refuses `move container to workspace back_and_forth` when
/// the previous workspace was on its own output, even after that workspace
/// was destroyed: sway names the target from `seat->prev_workspace_name`, and
/// a workspace it would create there is refused before creation
/// (sway/commands/move.c:459-468, 498-511). Random-v3 seed 32488
/// (diff-fam-v3-move-ws-back-and-forth-after-ws-move); oracle scenario
/// sticky_move_back_and_forth_same_output_refused.
#[test]
fn sticky_move_to_back_and_forth_on_the_same_output_is_refused() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    f.add_output(2, (1280, 720));
    let client = f.add_client();
    map_test_window(&mut f, client, "sticky");
    for step in [
        "floating enable",
        "sticky enable",
        "workspace 3",
        "focus floating",
    ] {
        assert!(
            crate::command::execute(f.niri_state(), step)[0].success,
            "{step}"
        );
    }
    assert_eq!(
        crate::command::execute(f.niri_state(), "move container to workspace back_and_forth"),
        [swayward_ipc::CommandOutcome {
            success: false,
            error: Some(
                "Can't move sticky container to another workspace on the same output".into()
            ),
            parse_error: Some(false),
        }]
    );
    let swayward = f.swayward();
    let names: Vec<_> = describe_workspaces(&swayward.layout, &swayward.global_space)
        .into_iter()
        .map(|workspace| workspace.name)
        .collect();
    assert!(!names.contains(&"1".to_owned()), "{names:?}");
    swayward.layout.verify_invariants();
}

/// `move workspace to output` keeps the seat's focused workspace, so it
/// records no history: `seat_set_focus` lands on the same workspace and
/// `set_workspace` returns early (sway/commands/move.c:659-667,
/// sway/input/seat.c:1098-1102). The previous record survives the move.
/// Random-v3 seeds 32404, 32453, 32496
/// (diff-fam-v3-move-ws-back-and-forth-after-ws-move); oracle scenario
/// move_back_and_forth_after_move_workspace_to_output.
#[test]
fn move_workspace_to_output_keeps_the_back_and_forth_record() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    f.add_output(2, (1280, 720));
    let client = f.add_client();
    assert!(crate::command::execute(f.niri_state(), "focus output headless-1")[0].success);
    assert_eq!(
        crate::command::execute(f.niri_state(), "move workspace to output headless-2")[0].error,
        None
    );
    map_test_window(&mut f, client, "moved");
    assert_eq!(
        crate::command::execute(f.niri_state(), "move container to workspace back_and_forth"),
        [swayward_ipc::CommandOutcome {
            success: false,
            error: Some("No workspace was previously active.".into()),
            parse_error: Some(false),
        }]
    );

    for step in [
        "workspace 5",
        "workspace 1",
        "move workspace to output headless-1",
    ] {
        assert!(
            crate::command::execute(f.niri_state(), step)[0].success,
            "{step}"
        );
    }
    assert!(
        crate::command::execute(f.niri_state(), "move container to workspace back_and_forth")[0]
            .success
    );
    let swayward = f.swayward();
    let holding: Vec<_> = describe_workspaces(&swayward.layout, &swayward.global_space)
        .into_iter()
        .filter(|workspace| workspace.name == "5")
        .collect();
    assert_eq!(holding.len(), 1);
    swayward.layout.verify_invariants();
}

/// `move workspace to output` detaches the workspace before naming the
/// replacement for the emptied output (`workspace_move_to_output`,
/// sway/sway/tree/workspace.c:1131-1161), and `workspace back_and_forth` has
/// already destroyed the empty workspace it left. The destination's empty
/// workspace is destroyed only after the replacement is named, so the
/// replacement takes the name back_and_forth just released. Oracle
/// scenario move_workspace_after_back_and_forth; random-v3 seed 32264.
#[test]
fn move_workspace_after_back_and_forth_reuses_the_released_name() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    f.add_output(2, (1280, 720));
    let swayward = f.swayward();
    let initial = describe_workspaces(&swayward.layout, &swayward.global_space);
    let second = initial
        .iter()
        .find(|w| w.name == "2")
        .map(|w| w.output.clone())
        .unwrap();
    let other = initial
        .iter()
        .find(|w| w.name == "1")
        .map(|w| w.output.clone())
        .unwrap();
    for command in [
        format!("focus output {second}"),
        "workspace 4".into(),
        "workspace 2".into(),
        "workspace back_and_forth".into(),
    ] {
        let outcome = crate::command::execute(f.niri_state(), &command);
        assert!(outcome.iter().all(|o| o.success), "{command}: {outcome:?}");
    }
    let swayward = f.swayward();
    let before = describe_workspaces(&swayward.layout, &swayward.global_space);
    let mut names = before.iter().map(|w| w.name.clone()).collect::<Vec<_>>();
    names.sort();
    assert_eq!(names, ["1", "4"].map(String::from), "{before:?}");

    let outcome =
        crate::command::execute(f.niri_state(), &format!("move workspace to output {other}"));
    assert!(outcome.iter().all(|o| o.success), "{outcome:?}");
    let swayward = f.swayward();
    let after = describe_workspaces(&swayward.layout, &swayward.global_space);
    let left = after.iter().find(|w| w.output == second).unwrap();
    assert_eq!(left.name, "2", "{after:?}");
    let focused = after.iter().find(|w| w.focused).unwrap();
    assert_eq!(focused.name, "4", "{after:?}");
}
