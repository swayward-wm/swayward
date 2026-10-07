#[test]
fn translated_map_time_sticky_command_executes_for_the_mapped_window() {
    let config = swayward_config::Config::parse_mem(
        r#"window-rule {
            match app-id="^sticky-map$"
            open-floating true
            sway-for-window-command "sticky enable"
        }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (800, 600));
    let client = fixture.add_client();
    let window = fixture.client(client).create_window();
    window.xdg_toplevel.set_app_id("sticky-map".into());
    let surface = window.surface.clone();
    window.commit();
    fixture.roundtrip(client);
    let window = fixture.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    fixture.double_roundtrip(client);

    let swayward = fixture.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let floating = &tree["nodes"][1]["nodes"][0]["floating_nodes"][0];
    assert_eq!(floating["app_id"], "sticky-map");
    assert_eq!(floating["sticky"], true);
}

#[test]
fn runtime_assign_applies_only_to_windows_mapped_after_registration() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();

    map_test_window(&mut f, client, "assigned-existing");
    let outcome = crate::command::execute(
        f.niri_state(),
        r#"assign [app_id="^assigned-"] workspace 7: target"#,
    );
    assert!(outcome[0].success, "{outcome:?}");
    map_test_window(&mut f, client, "assigned-future");

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let workspace_for = |app_id: &str| {
        tree["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|output| output["nodes"].as_array().unwrap())
            .find(|workspace| find_json_node_with_app_id(workspace, app_id).is_some())
            .unwrap()["name"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    assert_eq!(workspace_for("assigned-existing"), "1");
    assert_eq!(workspace_for("assigned-future"), "7: target");
}

#[test]
fn runtime_assign_uses_the_first_matching_rule() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();

    for command in [
        r#"assign [app_id="^assigned$"] workspace first"#,
        r#"assign [app_id="^assigned$"] workspace second"#,
    ] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success);
    }
    map_test_window(&mut f, client, "assigned");

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let workspace = tree["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|output| output["nodes"].as_array().unwrap())
        .find(|workspace| find_json_node_with_app_id(workspace, "assigned").is_some())
        .unwrap();
    assert_eq!(workspace["name"], "first");
}

#[test]
fn file_config_assignment_precedes_a_runtime_assignment() {
    let config = swayward_config::Config::parse_mem(
        r#"window-rule {
            match app-id="^assigned$"
            open-on-workspace "configured"
        }"#,
    )
    .unwrap();
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1920, 1080));
    let client = f.add_client();

    assert!(
        crate::command::execute(
            f.niri_state(),
            r#"assign [app_id="^assigned$"] workspace runtime"#,
        )[0]
        .success
    );
    map_test_window(&mut f, client, "assigned");

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let workspace = tree["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|output| output["nodes"].as_array().unwrap())
        .find(|workspace| find_json_node_with_app_id(workspace, "assigned").is_some())
        .unwrap();
    assert_eq!(workspace["name"], "configured");
}

#[test]
fn runtime_assign_supports_workspace_numbers() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();

    assert!(
        crate::command::execute(
            f.niri_state(),
            r#"assign [app_id="^numbered$"] workspace number 7: target"#,
        )[0]
        .success
    );
    map_test_window(&mut f, client, "numbered");

    assert!(f
        .swayward()
        .layout
        .workspaces()
        .any(|(_, _, workspace)| workspace.sway_name().as_deref() == Some("7: target")));
}

/// Oracle: assign_number_matches_by_digit_prefix. Sway resolves an
/// `assign ... number` target by its leading digits only
/// (_workspace_by_number, sway/tree/workspace.c:493-502), so with "3: web"
/// present, `number 3: mail` lands on "3: web" and creates nothing.
#[test]
fn runtime_assign_number_matches_an_existing_workspace_by_its_digits() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    assert!(crate::command::execute(f.niri_state(), "workspace 3: web")[0].success);
    map_test_window(&mut f, client, "fixture-1");
    assert!(crate::command::execute(f.niri_state(), "workspace 1")[0].success);
    let outcome = crate::command::execute(
        f.niri_state(),
        r#"assign [app_id="^assigned$"] number 3: mail"#,
    );
    assert!(outcome[0].success, "{outcome:?}");
    map_test_window(&mut f, client, "assigned");

    let swayward = f.swayward();
    let names = swayward
        .layout
        .workspaces()
        .filter_map(|(_, _, workspace)| workspace.sway_name())
        .collect::<Vec<_>>();
    assert!(!names.iter().any(|name| name == "3: mail"), "{names:?}");
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
        .find(|workspace| find_json_node_with_app_id(workspace, "assigned").is_some())
        .unwrap();
    assert_eq!(holder["name"], "3: web");
}

#[test]
fn runtime_assign_skips_a_missing_output_and_uses_the_next_match() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();

    for command in [
        r#"assign [app_id="^assigned$"] output missing"#,
        r#"assign [app_id="^assigned$"] workspace fallback"#,
    ] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success);
    }
    map_test_window(&mut f, client, "assigned");

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let workspace = tree["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|output| output["nodes"].as_array().unwrap())
        .find(|workspace| find_json_node_with_app_id(workspace, "assigned").is_some())
        .unwrap();
    assert_eq!(workspace["name"], "fallback");
}

#[test]
fn runtime_assign_supports_outputs() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    f.add_output(2, (1920, 1080));
    f.niri_focus_output(1);
    let client = f.add_client();

    assert!(
        crate::command::execute(
            f.niri_state(),
            r#"assign [app_id="^output$"] output headless-2"#,
        )[0]
        .success
    );
    map_test_window(&mut f, client, "output");

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let output = tree["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|output| find_json_node_with_app_id(output, "output").is_some())
        .unwrap();
    assert_eq!(output["name"], "headless-2");
}

#[test]
fn successful_reload_clears_runtime_assign_and_no_focus_rules() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    for command in [
        r#"assign [app_id="^future$"] workspace target"#,
        r#"no_focus [app_id="^future$"]"#,
    ] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success);
    }
    assert_eq!(f.swayward().runtime_window_rules.len(), 2);

    f.niri_state()
        .reload_config(Err::<swayward_config::Config, _>(()));
    assert_eq!(f.swayward().runtime_window_rules.len(), 2);

    f.niri_state()
        .reload_config(Ok(swayward_config::Config::default()));
    assert!(f.swayward().runtime_window_rules.is_empty());
}

#[test]
fn runtime_no_focus_does_not_leave_the_first_window_unfocused() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();

    assert!(crate::command::execute(f.niri_state(), r#"no_focus [app_id="^first$"]"#)[0].success);
    map_test_window(&mut f, client, "first");

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    assert_eq!(
        find_json_node(&tree, "con", true).unwrap()["app_id"],
        "first"
    );
}

#[test]
fn runtime_no_focus_applies_only_to_windows_mapped_after_registration() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();

    for app_id in ["existing", "already-mapped"] {
        map_test_window(&mut f, client, app_id);
    }
    let focused_app_id = |f: &mut Fixture| {
        let swayward = f.swayward();
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &swayward.marks_by_window,
            &swayward.marks_by_container,
        ))
        .unwrap();
        find_json_node(&tree, "con", true).unwrap()["app_id"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    assert_eq!(focused_app_id(&mut f), "already-mapped");
    assert!(
        crate::command::execute(
            f.niri_state(),
            r#"no_focus [app_id="^(already-mapped|future)$"]"#,
        )[0]
        .success
    );
    assert_eq!(
        focused_app_id(&mut f),
        "already-mapped",
        "registering no_focus must not change an already mapped view"
    );

    map_test_window(&mut f, client, "future");
    assert_eq!(
        focused_app_id(&mut f),
        "already-mapped",
        "a future no_focus match must not steal focus"
    );
}

fn workspace_focus(f: &mut Fixture, name: &str) -> Vec<Value> {
    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let workspace = tree["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|output| output["nodes"].as_array().unwrap())
        .find(|workspace| workspace["name"] == name)
        .unwrap();
    workspace["focus"].as_array().unwrap().clone()
}

/// Sway appends a new node to the bottom of the seat focus stack (`seat_node_from_node`,
/// sway/input/seat.c:327-354) and raises it only when `should_focus` holds
/// (sway/tree/view.c:697-731, 945-957). A `no_focus` view mapped while the workspace
/// itself is focused therefore ranks behind the existing view, not ahead of it.
/// Oracle row: no_focus_view_ranks_last (random-v3 seed 40262).
#[test]
fn no_focus_view_mapped_beside_a_focused_workspace_ranks_last() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();

    map_test_window(&mut f, client, "first");
    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
    assert!(crate::command::execute(f.niri_state(), r#"no_focus [app_id="^second$"]"#)[0].success);
    map_test_window(&mut f, client, "second");

    let expected = vec![
        rule_window_node(&mut f, "first")["id"].clone(),
        rule_window_node(&mut f, "second")["id"].clone(),
    ];
    assert_eq!(workspace_focus(&mut f, "1"), expected);
}

/// Sway runs a criteria command once per match with live seat focus
/// (sway/commands.c:305-323). Moving the focused view away refocuses the never-focused
/// `no_focus` view left behind (sway/commands/move.c:598-608), so when the next match moves
/// it to the same workspace it is the most recently focused view there. Oracle row:
/// no_focus_view_ranks_last (random-v3 seed 40246).
#[test]
fn multi_match_move_ranks_each_refocused_view_on_the_seat_stack() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();

    map_test_window(&mut f, client, "first");
    f.niri_state().update_keyboard_focus();
    assert!(crate::command::execute(f.niri_state(), r#"no_focus [app_id="^second$"]"#)[0].success);
    map_test_window(&mut f, client, "second");
    f.niri_state().update_keyboard_focus();
    assert!(
        crate::command::execute(
            f.niri_state(),
            "[workspace=__focused__] move container to workspace 2"
        )[0]
        .success
    );

    let expected = vec![
        rule_window_node(&mut f, "second")["id"].clone(),
        rule_window_node(&mut f, "first")["id"].clone(),
    ];
    assert_eq!(workspace_focus(&mut f, "2"), expected);
}

#[test]
fn for_window_opacity_applies_when_window_maps() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (1920, 1080));
    let mut stream = UnixStream::connect(socket).unwrap();
    stream
        .write_all(&swayward_ipc::wire::encode(
            MessageType::RunCommand,
            r#"for_window [app_id="^transparent$"] opacity 0.6"#,
        ))
        .unwrap();
    let (_, reply) = read_ipc_reply(&mut fixture, &mut stream);
    assert_eq!(
        serde_json::from_str::<Value>(&reply).unwrap(),
        serde_json::json!([{"success": true}])
    );

    let id = fixture.add_client();
    let window = fixture.client(id).create_window();
    window.xdg_toplevel.set_app_id("transparent".into());
    let surface = window.surface.clone();
    window.commit();
    fixture.roundtrip(id);
    let window = fixture.client(id).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    fixture.double_roundtrip(id);

    assert_eq!(
        fixture.swayward().layout.focus().unwrap().command_opacity(),
        0.6
    );
}

#[test]
fn for_window_applies_matching_command_when_window_maps() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (1920, 1080));
    let mut stream = UnixStream::connect(socket).unwrap();
    stream
        .write_all(&swayward_ipc::wire::encode(
            MessageType::RunCommand,
            r#"for_window [app_id="^dialog$"] floating enable"#,
        ))
        .unwrap();
    let (_, reply) = read_ipc_reply(&mut fixture, &mut stream);
    assert_eq!(
        serde_json::from_str::<Value>(&reply).unwrap(),
        serde_json::json!([{"success": true}])
    );

    let id = fixture.add_client();
    let window = fixture.client(id).create_window();
    window.xdg_toplevel.set_app_id("dialog".into());
    window.set_title("Dialog");
    let surface = window.surface.clone();
    window.commit();
    fixture.roundtrip(id);
    let window = fixture.client(id).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    fixture.double_roundtrip(id);

    let swayward = fixture.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    assert_eq!(
        find_json_node(&tree, "floating_con", false).unwrap()["app_id"],
        "dialog"
    );
}

#[test]
fn marking_a_mapped_window_runs_each_newly_matching_for_window_rule_once() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    assert!(
        crate::command::execute(
            f.niri_state(),
            "for_window [con_mark=trigger] sticky toggle",
        )[0]
        .success
    );

    let client = f.add_client();
    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("first".into());
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);
    let mapped = f.swayward().layout.focus().unwrap().id();
    let sticky = |f: &mut Fixture, app_id: &str| {
        let swayward = f.swayward();
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &swayward.marks_by_window,
            &swayward.marks_by_container,
        ))
        .unwrap();
        find_json_node_with_app_id(&tree, app_id).unwrap()["sticky"] == true
    };
    assert!(!sticky(&mut f, "first"));

    assert!(crate::command::execute(f.niri_state(), "mark trigger")[0].success);
    assert!(
        sticky(&mut f, "first"),
        "the mark-dependent action must run"
    );

    let second = f.client(client).create_window();
    second.xdg_toplevel.set_app_id("second".into());
    second.commit();
    let surface = second.surface.clone();
    f.roundtrip(client);
    let second = f.client(client).window(&surface);
    second.attach_new_buffer();
    second.ack_last_and_commit();
    f.double_roundtrip(client);
    assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);
    assert!(crate::command::execute(f.niri_state(), "mark trigger")[0].success);
    assert!(sticky(&mut f, "second"));

    let con_id = crate::ipc::tree::window_id(mapped);
    assert!(
        crate::command::execute(f.niri_state(), &format!("[con_id={con_id}] mark trigger"),)[0]
            .success
    );
    assert!(
        sticky(&mut f, "first"),
        "moving a global mark away and back must not rerun the first view's rule"
    );
}

fn map_rule_window(fixture: &mut Fixture, client: super::client::ClientId, app_id: &str) {
    let window = fixture.client(client).create_window();
    window.xdg_toplevel.set_app_id(app_id.into());
    let surface = window.surface.clone();
    window.commit();
    fixture.roundtrip(client);
    let window = fixture.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    fixture.double_roundtrip(client);
}

fn rule_window_node(fixture: &mut Fixture, app_id: &str) -> Value {
    let swayward = fixture.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    find_json_node_with_app_id(&tree, app_id).unwrap().clone()
}

/// A map-time floating rule runs once. Sway executes a view's criteria on map
/// and skips any criterion it already ran for that view
/// (`sway/sway/tree/view.c:569-593`); a floating change does not rerun them.
/// Re-running every rule after the rule's own `floating` command recursed until
/// the stack overflowed and aborted the compositor.
#[test]
fn map_time_floating_rule_does_not_recurse() {
    for (command, floating) in [
        ("floating enable", true),
        ("floating toggle", true),
        ("floating disable", false),
    ] {
        let config = swayward_config::Config::parse_mem(&format!(
            r#"window-rule {{
                match app-id="^flip$"
                sway-for-window-command "{command}"
            }}"#,
        ))
        .unwrap();
        let mut fixture = Fixture::with_config(config);
        fixture.add_output(1, (800, 600));
        let client = fixture.add_client();
        map_rule_window(&mut fixture, client, "flip");

        let node = rule_window_node(&mut fixture, "flip");
        assert_eq!(
            node["type"] == "floating_con",
            floating,
            "{command}: {node}"
        );
    }
}

/// A floating change does not re-execute a window's map-time rules. Sway only
/// runs `for_window` criteria on map, on mark, and on title or app_id change,
/// and never twice for one view (`sway/sway/tree/view.c:569-593`).
#[test]
fn targeted_floating_toggle_does_not_rerun_map_time_rules() {
    let config = swayward_config::Config::parse_mem(
        r#"window-rule {
            match app-id="^rerun$"
            sway-for-window-command "mark --add --toggle seen"
        }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (800, 600));
    let client = fixture.add_client();
    map_rule_window(&mut fixture, client, "rerun");
    let con_id = rule_window_node(&mut fixture, "rerun")["id"].clone();
    assert_eq!(
        rule_window_node(&mut fixture, "rerun")["marks"],
        serde_json::json!(["seen"])
    );

    for _ in 0..2 {
        let outcome = crate::command::execute(
            fixture.niri_state(),
            &format!("[con_id={con_id}] floating toggle"),
        );
        assert!(outcome[0].success, "{outcome:?}");
        assert_eq!(
            rule_window_node(&mut fixture, "rerun")["marks"],
            serde_json::json!(["seen"]),
            "a floating change must not rerun the map-time mark rule"
        );
    }
}

/// Oracle: differential_seed_1030 (also 1157, 1187). Sway focuses a new view
/// only when it maps into the focused workspace (should_focus,
/// sway/sway/tree/view.c:712-715), so a window assigned to another workspace
/// leaves the empty focused workspace current even with a launch token.
#[test]
fn runtime_assign_to_another_workspace_keeps_the_empty_current_workspace() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();

    let outcome = crate::command::execute(
        f.niri_state(),
        r#"assign [app_id="fixture-diff-4"] workspace 2"#,
    );
    assert!(outcome[0].success, "{outcome:?}");
    // `exec` hands the client a launch token, which it activates before mapping.
    let (token, _) = f.swayward().activation_state.create_external_token(None);
    let token = token.to_string();
    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("fixture-diff-4".into());
    let surface = window.surface.clone();
    window.commit();
    f.roundtrip(client);
    f.client(client).activate(token, &surface);
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    let swayward = f.swayward();
    let active = swayward.layout.active_workspace().unwrap();
    assert_eq!(active.sway_name().as_deref(), Some("1"));
    assert!(swayward.layout.focus().is_none());
    let (_, target) = swayward.layout.find_workspace_by_name("2").unwrap();
    assert_eq!(target.windows().count(), 1);
}

#[test]
fn for_window_layout_wraps_the_workspace_children_instead_of_changing_the_workspace() {
    // `for_window [...] layout tabbed` runs with the new window as the
    // handler container, so sway keeps the workspace layout and wraps its
    // children in a tabbed container (sway/commands/layout.c:178-183).
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    assert!(
        crate::command::execute(f.niri_state(), r#"for_window [app_id="tab"] layout tabbed"#)[0]
            .success
    );
    let client = f.add_client();
    map_test_window(&mut f, client, "tab");

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let workspace = &tree["nodes"][1]["nodes"][0];
    assert_eq!(workspace["layout"], "splith");
    assert_eq!(workspace["representation"], "H[T[tab]]");
    let wrapper = &workspace["nodes"][0];
    assert_eq!(wrapper["type"], "con");
    assert_eq!(wrapper["layout"], "tabbed");
    assert_eq!(wrapper["nodes"][0]["app_id"], "tab");
    assert_eq!(wrapper["nodes"][0]["focused"], true);
}

#[test]
fn for_window_layout_wrapper_is_raised_with_the_mapped_view() {
    // Sway runs a mapped view's criteria before focusing it (`view_map`,
    // sway/tree/view.c:943-956), so the wrapper `layout tabbed` creates is
    // raised with the view and leads the workspace focus list ahead of an
    // older floating window.
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    map_test_window(&mut f, client, "float");
    assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);
    map_test_window(&mut f, client, "tile");
    assert!(
        crate::command::execute(f.niri_state(), r#"for_window [app_id="tab"] layout tabbed"#)[0]
            .success
    );
    map_test_window(&mut f, client, "tab");

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let workspace = &tree["nodes"][1]["nodes"][0];
    let wrapper = &workspace["nodes"][0];
    assert_eq!(wrapper["layout"], "tabbed");
    assert_eq!(
        workspace["focus"],
        serde_json::json!([wrapper["id"], workspace["floating_nodes"][0]["id"]])
    );
}
