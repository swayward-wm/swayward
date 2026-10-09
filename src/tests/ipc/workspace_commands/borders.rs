#[test]
fn criteria_directional_move_uses_the_materialized_target_without_changing_focus() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    for app_id in ["target", "middle", "target", "focused"] {
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
    let focused = f.swayward().layout.focus().unwrap().id();

    let outcome = crate::command::execute(f.niri_state(), r#"[app_id="target"] move right"#);

    assert!(outcome[0].success, "{outcome:?}");
    assert_eq!(f.swayward().layout.focus().unwrap().id(), focused);
    let apps = f
        .swayward()
        .layout
        .active_workspace()
        .unwrap()
        .tiles()
        .map(|tile| {
            crate::utils::with_toplevel_role(tile.window().toplevel(), |role| {
                role.app_id.clone().unwrap()
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(apps, ["middle", "target", "focused", "target"]);

    let outcome = crate::command::execute(f.niri_state(), r#"[app_id="target"] move left"#);
    assert!(outcome[0].success, "{outcome:?}");
    assert_eq!(f.swayward().layout.focus().unwrap().id(), focused);
    let apps = f
        .swayward()
        .layout
        .active_workspace()
        .unwrap()
        .tiles()
        .map(|tile| {
            crate::utils::with_toplevel_role(tile.window().toplevel(), |role| {
                role.app_id.clone().unwrap()
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(apps, ["target", "middle", "target", "focused"]);
}

#[test]
fn criteria_commands_do_not_change_focus() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let mut surfaces = Vec::new();
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
        surfaces.push(surface);
    }
    let focused = f.swayward().layout.focus().unwrap().id();

    assert!(
        crate::command::execute(f.niri_state(), r#"[app_id="target"] mark selected"#)[0].success
    );

    assert_eq!(f.swayward().layout.focus().unwrap().id(), focused);
    assert_eq!(surfaces.len(), 2);
}

#[test]
fn multi_target_mark_moves_to_last_match_and_unmark_clears_every_match() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let mut ids = Vec::new();

    for title in ["first", "second"] {
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id("shared-app".into());
        window.set_title(title);
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        ids.push(f.swayward().layout.focus().unwrap().id());
    }

    let outcome = crate::command::execute(f.niri_state(), r#"[app_id="shared-app"] mark shared"#);
    assert_eq!(outcome.len(), 1);
    assert!(outcome[0].success);
    assert!(f
        .swayward()
        .marks_by_window
        .get(&ids[0])
        .is_none_or(Vec::is_empty));
    assert_eq!(
        f.swayward().marks_by_window.get(&ids[1]).map(Vec::as_slice),
        Some(["shared".to_owned()].as_slice())
    );

    for (id, mark) in ids.iter().zip(["first", "second"]) {
        let outcome = crate::command::execute(
            f.niri_state(),
            &format!(
                r#"[con_id="{}"] mark {mark}"#,
                crate::ipc::tree::window_id(*id)
            ),
        );
        assert!(outcome[0].success);
    }
    assert!(crate::command::execute(f.niri_state(), r#"[app_id="shared-app"] unmark"#)[0].success);
    assert!(ids.iter().all(|id| f
        .swayward()
        .marks_by_window
        .get(id)
        .is_none_or(Vec::is_empty)));
}

#[test]
fn semicolon_starts_a_new_criteria_scope() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let mut ids = Vec::new();
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
        ids.push(f.swayward().layout.focus().unwrap().id());
    }

    let outcomes = crate::command::execute(
        f.niri_state(),
        r#"[app_id="first"] mark first; [app_id="second"] mark second"#,
    );

    assert!(outcomes.iter().all(|outcome| outcome.success));
    assert_eq!(
        f.swayward().marks_by_window.get(&ids[0]).map(Vec::as_slice),
        Some(["first".to_owned()].as_slice())
    );
    assert_eq!(
        f.swayward().marks_by_window.get(&ids[1]).map(Vec::as_slice),
        Some(["second".to_owned()].as_slice())
    );
}

#[test]
fn comma_chain_keeps_the_original_criteria_targets() {
    let mut f = Fixture::new();
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

    assert!(crate::command::execute(f.niri_state(), "mark original")[0].success);
    let outcomes = crate::command::execute(
        f.niri_state(),
        "[con_mark=original] unmark original, mark retained",
    );
    assert!(outcomes.iter().all(|outcome| outcome.success));
    assert_eq!(
        f.swayward().marks_by_window.values().next().unwrap(),
        &["retained"]
    );
}

#[test]
fn output_workspaces_and_move_replacements_use_next_free_numbers() {
    let mut f = Fixture::new();
    f.add_named_output_at("fake-0".into(), (100, 100), Some((0, 0)));
    f.add_named_output_at("fake-1".into(), (100, 100), Some((100, 0)));

    assert!(crate::command::execute(f.niri_state(), "focus output fake-1")[0].success);
    let swayward = f.swayward();
    let workspaces = describe_workspaces(&swayward.layout, &swayward.global_space);
    assert_eq!(
        workspaces
            .iter()
            .map(|workspace| (workspace.output.as_str(), workspace.name.as_str()))
            .collect::<Vec<_>>(),
        [("fake-0", "1"), ("fake-1", "2")]
    );

    assert!(crate::command::execute(f.niri_state(), "focus output fake-0")[0].success);
    assert!(crate::command::execute(f.niri_state(), "move workspace to output fake-1")[0].success);
    let swayward = f.swayward();
    let workspaces = describe_workspaces(&swayward.layout, &swayward.global_space);
    assert_eq!(
        workspaces
            .iter()
            .filter(|workspace| workspace.output == "fake-0")
            .map(|workspace| workspace.name.as_str())
            .collect::<Vec<_>>(),
        ["3"]
    );
}

#[test]
fn rename_ignores_an_empty_inactive_source_sway_would_have_destroyed() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();

    let outcome = crate::command::execute(f.niri_state(), "workspace 5");
    assert!(outcome[0].success, "{outcome:?}");
    let surface = windows::map_window(&mut f, client, windows::WindowSpec::default());
    let window = f.client(client).window(&surface);
    window.attach_null();
    window.commit();
    f.double_roundtrip(client);

    let outcome = crate::command::execute(f.niri_state(), "workspace 6");
    assert!(outcome[0].success, "{outcome:?}");
    windows::map_window(&mut f, client, windows::WindowSpec::default());

    let outcome = crate::command::execute(f.niri_state(), "rename workspace 5 to 5: foo");
    assert!(!outcome[0].success, "{outcome:?}");
    assert_eq!(outcome[0].parse_error, Some(true));
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("There is no workspace with that name")
    );
}

#[test]
fn rename_workspace_updates_name_number_and_rejects_collisions() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));

    for command in [
        "workspace 5",
        "rename workspace to 7: web",
        "workspace mail",
        "rename workspace mail to inbox",
        "rename workspace inbox to mail",
    ] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(outcome[0].success, "{command}: {outcome:?}");
    }
    let collision = crate::command::execute(f.niri_state(), "rename workspace mail to 7: web");
    assert!(!collision[0].success);
    assert_eq!(collision[0].parse_error, Some(true));
    for command in [
        "rename workspace mail to chat",
        "rename workspace chat to CHAT",
        "rename workspace chat to 9 web",
    ] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(outcome[0].success, "{command}: {outcome:?}");
    }
    assert!(!crate::command::execute(f.niri_state(), "rename workspace to next")[0].success);

    let swayward = f.swayward();
    assert_eq!(
        describe_workspaces(&swayward.layout, &swayward.global_space)
            .iter()
            .map(|workspace| (workspace.num, workspace.name.as_str()))
            .collect::<Vec<_>>(),
        [(9, "9 web")]
    );
}

#[test]
fn tiled_and_floating_default_borders_remain_independent_in_get_tree() {
    let config = swayward_config::Config::parse_mem(
        r#"window-rule {
            sway-border "pixel"
            sway-border-width 5
            sway-floating-border "normal"
            sway-floating-border-width 2
        }
        window-rule {
            match app-id="floating"
            open-floating true
        }"#,
    )
    .unwrap();
    let (mut f, _) = ipc_fixture_with_config(config);
    f.add_output(1, (800, 600));
    let client = f.add_client();
    for app_id in ["tiled", "floating"] {
        let window = f.client(client).create_ssd_window();
        window.xdg_toplevel.set_app_id(app_id.into());
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    }

    let tree = get_tree(&mut f);
    let workspace = &tree["nodes"][1]["nodes"][0];
    let tiled = &workspace["nodes"][0];
    let floating = &workspace["floating_nodes"][0];
    assert_eq!(tiled["border"], "pixel");
    assert_eq!(tiled["current_border_width"], 5);
    assert_eq!(floating["border"], "normal");
    assert_eq!(floating["current_border_width"], 2);
}

#[test]
fn default_border_changes_only_windows_mapped_after_the_command() {
    let mut f = Fixture::new();
    f.add_output(1, (800, 600));
    let client = f.add_client();

    for app_id in ["existing", "explicit"] {
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
        crate::command::execute(f.niri_state(), r#"[app_id="explicit"] border pixel 7"#,)[0]
            .success
    );
    assert!(crate::command::execute(f.niri_state(), "default_border pixel 3")[0].success);

    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("new".into());
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let existing = find_json_node_with_app_id(&tree, "existing").unwrap();
    assert_eq!(existing["border"], "normal");
    assert_eq!(existing["current_border_width"], 4);
    let explicit = find_json_node_with_app_id(&tree, "explicit").unwrap();
    assert_eq!(explicit["border"], "pixel");
    assert_eq!(explicit["current_border_width"], 7);
    let new = find_json_node_with_app_id(&tree, "new").unwrap();
    assert_eq!(new["border"], "pixel");
    assert_eq!(new["current_border_width"], 3);
}

#[test]
fn default_floating_border_changes_only_windows_mapped_after_the_command() {
    let config = swayward_config::Config::parse_mem("window-rule { open-floating true; }").unwrap();
    let mut f = Fixture::with_config(config);
    f.add_output(1, (800, 600));
    let client = f.add_client();

    for app_id in ["existing-float", "new-float"] {
        if app_id == "new-float" {
            assert!(
                crate::command::execute(f.niri_state(), "default_floating_border pixel 3",)[0]
                    .success
            );
        }
        let window = f.client(client).create_ssd_window();
        window.xdg_toplevel.set_app_id(app_id.into());
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    }

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let existing = find_json_node_with_app_id(&tree, "existing-float").unwrap();
    assert_eq!(existing["border"], "normal");
    assert_eq!(existing["current_border_width"], 4);
    let new = find_json_node_with_app_id(&tree, "new-float").unwrap();
    assert_eq!(new["border"], "pixel");
    assert_eq!(new["current_border_width"], 3);
}

#[test]
fn edge_border_modes_apply_to_workspace_edges_and_visible_view_count() {
    fn window_nodes(config: &str, windows: usize) -> Vec<Value> {
        let config = swayward_config::Config::parse_mem(config).unwrap();
        let mut f = Fixture::with_config(config);
        f.add_output(1, (800, 600));
        let client = f.add_client();
        for _ in 0..windows {
            let window = f.client(client).create_ssd_window();
            window.commit();
            let surface = window.surface.clone();
            f.roundtrip(client);
            let window = f.client(client).window(&surface);
            window.attach_new_buffer();
            window.ack_last_and_commit();
            f.double_roundtrip(client);
        }
        let swayward = f.swayward();
        swayward.layout.update_render_elements(None);
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &swayward.marks_by_window,
            &swayward.marks_by_container,
        ))
        .unwrap();
        let workspace = &tree["nodes"][1]["nodes"][0];
        workspace["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .chain(workspace["floating_nodes"].as_array().unwrap())
            .cloned()
            .collect()
    }

    let config = |mode: &str, smart: &str, gaps: u8| {
        format!(
            r#"layout {{
                gaps {gaps}
                hide-edge-borders "{mode}"
                smart-borders "{smart}"
            }}
            window-rule {{ sway-border "pixel"; sway-border-width 7; }}"#
        )
    };

    let vertical = window_nodes(&config("vertical", "off", 0), 2);
    assert_eq!(vertical[0]["window_rect"]["x"], 0);
    assert_eq!(vertical[0]["window_rect"]["y"], 7);
    assert_eq!(vertical[0]["window_rect"]["width"], 393);
    assert_eq!(vertical[0]["window_rect"]["height"], 586);
    assert_eq!(vertical[1]["window_rect"]["x"], 7);
    assert_eq!(vertical[1]["window_rect"]["y"], 7);
    assert_eq!(vertical[1]["window_rect"]["width"], 393);
    assert_eq!(vertical[1]["window_rect"]["height"], 586);

    let horizontal = window_nodes(&config("horizontal", "off", 0), 2);
    assert_eq!(horizontal[0]["window_rect"]["x"], 7);
    assert_eq!(horizontal[0]["window_rect"]["y"], 0);
    assert_eq!(horizontal[0]["window_rect"]["width"], 386);
    assert_eq!(horizontal[0]["window_rect"]["height"], 600);
    assert_eq!(horizontal[1]["window_rect"]["x"], 7);
    assert_eq!(horizontal[1]["window_rect"]["y"], 0);
    assert_eq!(horizontal[1]["window_rect"]["width"], 386);
    assert_eq!(horizontal[1]["window_rect"]["height"], 600);

    let smart_single = window_nodes(&config("none", "on", 0), 1);
    assert_eq!(
        smart_single[0]["window_rect"],
        serde_json::json!({ "x": 0, "y": 0, "width": 800, "height": 600 })
    );
    let smart_two = window_nodes(&config("none", "on", 0), 2);
    assert!(smart_two.iter().all(|node| {
        node["window_rect"] == serde_json::json!({ "x": 7, "y": 7, "width": 386, "height": 586 })
    }));
    let smart_and_edges = window_nodes(&config("both", "on", 0), 2);
    assert_eq!(
        smart_and_edges[0]["window_rect"],
        serde_json::json!({ "x": 0, "y": 0, "width": 393, "height": 600 })
    );
    assert_eq!(
        smart_and_edges[1]["window_rect"],
        serde_json::json!({ "x": 7, "y": 0, "width": 393, "height": 600 })
    );

    let no_gaps = window_nodes(&config("none", "no-gaps", 0), 1);
    assert_eq!(
        no_gaps[0]["window_rect"],
        serde_json::json!({ "x": 0, "y": 0, "width": 800, "height": 600 })
    );
    let with_gaps = window_nodes(&config("none", "no-gaps", 16), 1);
    assert_eq!(with_gaps[0]["window_rect"]["x"], 7);
    assert_eq!(with_gaps[0]["window_rect"]["y"], 7);

    let floating = window_nodes(
        &format!(
            "{}\nwindow-rule {{ open-floating true; }}",
            config("both", "on", 0)
        ),
        1,
    );
    assert_eq!(floating[0]["window_rect"]["x"], 7);
    assert_eq!(floating[0]["window_rect"]["y"], 7);
    assert_eq!(
        floating[0]["window_rect"]["width"].as_i64().unwrap(),
        floating[0]["rect"]["width"].as_i64().unwrap() - 14
    );
    assert_eq!(
        floating[0]["window_rect"]["height"].as_i64().unwrap(),
        floating[0]["rect"]["height"].as_i64().unwrap() - 14
    );
    assert_eq!(floating[0]["current_border_width"], 7);

    let initial = swayward_config::Config::parse_mem(&config("none", "off", 0)).unwrap();
    let mut f = Fixture::with_config(initial);
    f.add_output(1, (800, 600));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    f.niri_state()
        .reload_config(Ok(swayward_config::Config::parse_mem(&config(
            "both", "on", 0,
        ))
        .unwrap()));
    let swayward = f.swayward();
    swayward.layout.update_render_elements(None);
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let node = find_json_node(&tree, "con", false).unwrap();
    assert_eq!(
        node["window_rect"],
        serde_json::json!({ "x": 0, "y": 0, "width": 800, "height": 600 })
    );
}

#[test]
fn configured_border_width_matches_rendering_and_tree_for_tiled_and_floating_windows() {
    let config =
        swayward_config::Config::parse_mem(r#"layout { border { on; width 7; }; }"#).unwrap();
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let window = f.client(client).create_ssd_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    for (node_type, floating) in [("con", false), ("floating_con", true)] {
        if floating {
            assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);
        }
        let swayward = f.swayward();
        let tile = swayward
            .layout
            .active_workspace()
            .unwrap()
            .tiles()
            .next()
            .unwrap();
        assert_eq!(tile.effective_border_width(), Some(7.));
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &swayward.marks_by_window,
            &swayward.marks_by_container,
        ))
        .unwrap();
        let node = find_json_node(&tree, node_type, false).unwrap();
        assert_eq!(node["border"], "normal");
        assert_eq!(node["current_border_width"], 7);
    }

    assert!(crate::command::execute(f.niri_state(), "border none")[0].success);
    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let node = find_json_node(&tree, "floating_con", false).unwrap();
    assert_eq!(node["border"], "none");
    // Sway reports the retained thickness whatever the style
    // (sway/ipc-json.c:760-761).
    assert_eq!(node["current_border_width"], 7);
}

#[test]
fn border_command_updates_rendering_and_tree_metadata() {
    let mut f = Fixture::new();
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

    // The fixture's default thickness, which `border none` keeps.
    let default_width = {
        let swayward = f.swayward();
        let mapped = swayward.layout.focus().unwrap();
        let tile = swayward
            .layout
            .active_workspace()
            .unwrap()
            .tiles()
            .next()
            .unwrap();
        assert_eq!(tile.window().id(), mapped.id());
        i32::from(tile.sway_border_thickness().1)
    };
    for (command, style, stored_width, ipc_width, has_titlebar, rendered_width) in [
        ("border none", "none", 0, default_width, false, None),
        ("border pixel 3", "pixel", 3, 3, false, Some(3.)),
        ("border normal 5", "normal", 5, 5, true, Some(5.)),
        // Sway keeps the thickness across style changes without a width
        // (cmd_border, sway/commands/border.c:90-92; oracle
        // border_thickness_survives_style_changes). This test window has no
        // xdg-decoration, so the first toggle goes straight to none.
        ("border toggle", "none", 0, 5, false, None),
        ("border toggle", "pixel", 5, 5, false, Some(5.)),
        ("border toggle", "normal", 5, 5, true, Some(5.)),
        ("border pixel 4", "pixel", 4, 4, false, Some(4.)),
        ("border none", "none", 0, 4, false, None),
        ("border pixel", "pixel", 4, 4, false, Some(4.)),
        ("border normal", "normal", 4, 4, true, Some(4.)),
    ] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(outcome[0].success, "{command}: {outcome:?}");
        let swayward = f.swayward();
        let mapped = swayward.layout.focus().unwrap();
        assert_eq!(
            swayward.layout.window_border(&mapped.window),
            Some((
                match style {
                    "none" => swayward_ipc::command::BorderStyle::None,
                    "pixel" => swayward_ipc::command::BorderStyle::Pixel,
                    "normal" => swayward_ipc::command::BorderStyle::Normal,
                    _ => unreachable!(),
                },
                stored_width
            ))
        );
        let tile = swayward
            .layout
            .active_workspace()
            .unwrap()
            .tiles()
            .next()
            .unwrap();
        assert_eq!(tile.effective_border_width(), rendered_width);
        assert_eq!(tile.has_sway_titlebar(), has_titlebar);
        let tree = describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &swayward.marks_by_window,
            &swayward.marks_by_container,
        );
        let node = tree
            .nodes
            .iter()
            .flat_map(|output| &output.nodes)
            .flat_map(|workspace| workspace.nodes.iter().chain(&workspace.floating_nodes))
            .next()
            .unwrap();
        assert_eq!(format!("{:?}", node.border).to_ascii_lowercase(), style);
        assert_eq!(node.current_border_width, ipc_width);
    }

    assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);
    // `border none` keeps the thickness the loop above left behind (4).
    for (command, style, width) in [("border none", "none", 4), ("border pixel 7", "pixel", 7)] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success);
        let swayward = f.swayward();
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &swayward.marks_by_window,
            &swayward.marks_by_container,
        ))
        .unwrap();
        let node = find_json_node(&tree, "floating_con", false).unwrap();
        assert_eq!(node["border"], style, "{command}");
        assert_eq!(node["current_border_width"], width, "{command}");
    }
}

/// A hidden scratchpad window keeps its border thickness under `border none`,
/// as sway reports `c->current.border_thickness` (sway/ipc-json.c:760-761).
/// Oracle row scratchpad_border_none_keeps_thickness; differential seed 1199.
#[test]
fn scratchpad_window_under_border_none_reports_its_thickness() {
    let mut f = Fixture::new();
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

    for command in ["border pixel 3", "border none", "move scratchpad"] {
        assert!(
            crate::command::execute(f.niri_state(), command)[0].success,
            "{command}"
        );
    }
    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let node = find_json_node(&tree, "floating_con", false).unwrap();
    assert_eq!(node["scratchpad_state"], "fresh");
    assert_eq!(node["border"], "none");
    assert_eq!(node["current_border_width"], 3);
}

#[test]
fn border_csd_fails_without_client_decoration_support() {
    let mut f = Fixture::new();
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

    let outcome = crate::command::execute(f.niri_state(), "border csd");
    assert!(!outcome[0].success);
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("This window doesn't support client side decorations")
    );
}

#[test]
fn border_csd_on_a_tiled_window_keeps_sways_titlebar() {
    use smithay::reexports::wayland_protocols::xdg::decoration::zv1::client::zxdg_toplevel_decoration_v1::Mode;

    // foot creates an xdg-decoration object and asks for server-side
    // decorations. Sway accepts `border csd` because the object exists
    // (sway/commands/border.c:77-80), and on a tiled view it keeps the
    // stored border and goes on drawing it (border.c:10-14, 25-27).
    // Differential family diff-fam-border-csd, seeds 1035 1059 1102 1109.
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.client(client).decorate_last_window(Mode::ServerSide);
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    let tree_node = |f: &mut Fixture| {
        let swayward = f.swayward();
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &swayward.marks_by_window,
            &swayward.marks_by_container,
        ))
        .unwrap();
        find_json_node(&tree, "con", true).unwrap().clone()
    };
    assert!(crate::command::execute(f.niri_state(), "border normal")[0].success);
    let before = tree_node(&mut f);
    assert_eq!(before["border"], "normal");

    let outcome = crate::command::execute(f.niri_state(), "border csd");
    assert!(outcome[0].success, "{:?}", outcome[0].error);
    f.double_roundtrip(client);
    assert_eq!(
        f.client(client).window(&surface).decoration_modes.last(),
        Some(&Mode::ClientSide)
    );

    let after = tree_node(&mut f);
    assert_eq!(after["border"], "normal");
    for field in ["rect", "window_rect", "deco_rect"] {
        assert_eq!(after[field], before[field], "{field}");
    }
    assert!(after["deco_rect"]["height"].as_i64().unwrap() > 0);
}

#[test]
fn border_csd_fails_after_the_decoration_object_is_destroyed() {
    use smithay::reexports::wayland_protocols::xdg::decoration::zv1::client::zxdg_toplevel_decoration_v1::Mode;

    // Sway clears view->xdg_decoration when the object is destroyed
    // (sway/xdg_decoration.c:9-20), so `border csd` is refused again
    // (sway/commands/border.c:77-80).
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.client(client).decorate_last_window(Mode::ServerSide);
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    f.client(client)
        .window(&surface)
        .xdg_decoration
        .take()
        .unwrap()
        .destroy();
    f.double_roundtrip(client);

    let outcome = crate::command::execute(f.niri_state(), "border csd");
    assert!(!outcome[0].success);
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("This window doesn't support client side decorations")
    );
}

#[test]
fn border_toggle_on_a_tiled_decorated_window_enters_csd_and_keeps_normal() {
    use smithay::reexports::wayland_protocols::xdg::decoration::zv1::client::zxdg_toplevel_decoration_v1::Mode;

    // foot has an xdg-decoration object, so sway's toggle cycles
    // normal -> csd (sway/commands/border.c:45-50). A tiled view does not
    // store B_CSD (border.c:25-27): GET_TREE still says "normal". The next
    // toggle leaves CSD for none (border.c:34-37).
    // Differential family diff-fam-border-toggle-cycle, seeds 1046 1070 1082
    // 1108 1148 1179 1193 1194.
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("toggled".into());
    window.commit();
    let surface = window.surface.clone();
    f.client(client).decorate_last_window(Mode::ServerSide);
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    let border = |f: &mut Fixture| {
        let swayward = f.swayward();
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &swayward.marks_by_window,
            &swayward.marks_by_container,
        ))
        .unwrap();
        find_json_node_with_app_id(&tree, "toggled").unwrap()["border"].clone()
    };
    assert_eq!(border(&mut f), "normal");

    assert!(crate::command::execute(f.niri_state(), "border toggle")[0].success);
    f.double_roundtrip(client);
    assert_eq!(border(&mut f), "normal");
    assert_eq!(
        f.client(client).window(&surface).decoration_modes.last(),
        Some(&Mode::ClientSide)
    );

    // Floating a CSD view saves its border and stores csd; tiling it again
    // restores the saved one (container_set_floating,
    // sway/tree/container.c:955-964, 995-1003). Seed 1179.
    assert!(crate::command::execute(f.niri_state(), "floating toggle")[0].success);
    f.double_roundtrip(client);
    assert_eq!(border(&mut f), "csd");
    assert!(crate::command::execute(f.niri_state(), "floating toggle")[0].success);
    f.double_roundtrip(client);
    assert_eq!(border(&mut f), "normal");

    assert!(crate::command::execute(f.niri_state(), "border toggle")[0].success);
    f.double_roundtrip(client);
    assert_eq!(border(&mut f), "none");
    assert_eq!(
        f.client(client).window(&surface).decoration_modes.last(),
        Some(&Mode::ServerSide)
    );
}

#[test]
fn view_mapped_under_global_fullscreen_keeps_its_border_and_share() {
    // A global fullscreen container does not set workspace->fullscreen
    // (container_fullscreen_global, sway/tree/container.c), so a view mapped
    // beside it is arranged with its siblings: border normal at the
    // configured width, percent 0.5.
    // Differential family diff-fam-new-view-border, seed 1044.
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    map_app(&mut f, client, "fs");
    assert!(crate::command::execute(f.niri_state(), "fullscreen toggle global")[0].success);
    f.double_roundtrip(client);
    map_app(&mut f, client, "new");

    let tree = tree_json(&mut f);
    let fs = find_json_node_with_app_id(&tree, "fs").unwrap();
    let new = find_json_node_with_app_id(&tree, "new").unwrap();
    assert_eq!(fs["percent"], 0.5);
    assert_eq!(new["percent"], 0.5);
    assert_eq!(new["border"], "normal");
    assert_ne!(new["current_border_width"], 0);
    assert_eq!(new["current_border_width"], fs["current_border_width"]);
}

#[test]
fn view_mapped_with_the_workspace_focused_goes_beside_its_focus_inactive_child() {
    // With the workspace focused, view_map maps beside
    // seat_get_focus_inactive(ws), the last focused view
    // (sway/tree/view.c:849-882), not at the end of the workspace.
    // Differential family diff-fam-new-view-border, seed 1527.
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    map_app(&mut f, client, "one");
    map_app(&mut f, client, "two");
    for command in ["move up", "focus parent"] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success);
    }
    map_app(&mut f, client, "three");

    let tree = tree_json(&mut f);
    let workspace = &tree["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|output| output["name"] != "__i3")
        .unwrap()["nodes"][0];
    let names = workspace["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| node["name"].as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    assert_eq!(names.len(), 3, "{workspace:#}");
    let three = find_json_node_with_app_id(&tree, "three").unwrap();
    assert_eq!(workspace["nodes"][1]["id"], three["id"], "{workspace:#}");
    assert_eq!(three["border"], "normal");
    assert_eq!(three["focused"], true);
}

fn workspace_tiling_names(tree: &serde_json::Value) -> Vec<String> {
    let workspace = &tree["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|output| output["name"] != "__i3")
        .unwrap()["nodes"][0];
    fn names(node: &serde_json::Value, out: &mut Vec<String>) {
        for child in node["nodes"].as_array().unwrap() {
            match child["app_id"].as_str() {
                Some(app_id) => out.push(app_id.to_owned()),
                None => {
                    out.push("[".to_owned());
                    names(child, out);
                    out.push("]".to_owned());
                }
            }
        }
    }
    let mut out = Vec::new();
    names(workspace, &mut out);
    out
}

#[test]
fn view_mapped_beside_a_floating_fullscreen_view_maps_beside_the_tiling_focus() {
    // A floating fullscreen view stays in the workspace's floating list
    // (container_set_floating, sway/tree/container.c:951-975), so view_map
    // maps a new view beside the focus-inactive tiling view instead
    // (seat_get_focus_inactive_tiling, sway/tree/view.c:851-866), not beside
    // the fullscreen view. With no tiling view it goes onto the workspace.
    // Differential family diff-fam-map-beside-floating-fullscreen,
    // seeds 13399 15058 16121.
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    map_app(&mut f, client, "fs");
    for command in ["fullscreen toggle", "floating toggle"] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success);
    }
    f.double_roundtrip(client);
    map_app(&mut f, client, "two");
    map_app(&mut f, client, "three");
    let tree = tree_json(&mut f);
    assert_eq!(workspace_tiling_names(&tree), ["two", "three"], "{tree:#}");
    let two = find_json_node_with_app_id(&tree, "two").unwrap();
    let three = find_json_node_with_app_id(&tree, "three").unwrap();
    assert_eq!(two["border"], "normal");
    assert_eq!(three["border"], "none");

    // Seed 16121: floating the view reaps its split (container_reap_empty,
    // sway/tree/container.c:969-975), and the new view maps beside the
    // tiling focus instead of into a split beside the fullscreen view.
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    map_app(&mut f, client, "one");
    map_app(&mut f, client, "two");
    for command in ["split h", "fullscreen enable", "floating enable"] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success);
    }
    f.double_roundtrip(client);
    map_app(&mut f, client, "three");
    let tree = tree_json(&mut f);
    assert_eq!(workspace_tiling_names(&tree), ["one", "three"], "{tree:#}");
}

#[test]
fn a_floating_fullscreen_view_takes_no_share_of_its_tiled_parent() {
    // Seed 16770: a view floated by criteria and then fullscreened is in the
    // floating list, so the tiled sibling keeps the whole workspace.
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    assert!(
        crate::command::execute(
            f.niri_state(),
            r#"for_window [app_id="float"] floating enable"#
        )[0]
        .success
    );
    map_app(&mut f, client, "tiled");
    map_app(&mut f, client, "float");
    assert!(crate::command::execute(f.niri_state(), "fullscreen enable")[0].success);
    f.double_roundtrip(client);
    let tree = tree_json(&mut f);
    let tiled = find_json_node_with_app_id(&tree, "tiled").unwrap();
    assert_eq!(tiled["percent"], 1.0, "{tree:#}");
}

#[test]
fn a_tiled_csd_window_moved_to_the_scratchpad_reports_csd() {
    use smithay::reexports::wayland_protocols::xdg::decoration::zv1::client::zxdg_toplevel_decoration_v1::Mode;

    // `move scratchpad` floats a tiled view first (root_scratchpad_add_container,
    // sway/tree/root.c:114-118), and floating a CSD view stores B_CSD
    // (container_set_floating, sway/tree/container.c:955-959), so the hidden
    // window reports "csd". Differential family diff-fam-csd-scratchpad,
    // seed 11055; oracle row scratchpad_tiled_csd_reports_csd.
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("hidden".into());
    window.commit();
    let surface = window.surface.clone();
    f.client(client).decorate_last_window(Mode::ServerSide);
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    let node = |f: &mut Fixture| {
        let swayward = f.swayward();
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &swayward.marks_by_window,
            &swayward.marks_by_container,
        ))
        .unwrap();
        find_json_node_with_app_id(&tree, "hidden").unwrap().clone()
    };

    assert!(crate::command::execute(f.niri_state(), "border toggle")[0].success);
    f.double_roundtrip(client);
    assert_eq!(node(&mut f)["border"], "normal");

    assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);
    f.double_roundtrip(client);
    let hidden = node(&mut f);
    assert_eq!(hidden["scratchpad_state"], "fresh");
    assert_eq!(hidden["border"], "csd");
    assert_eq!(
        f.client(client).window(&surface).decoration_modes.last(),
        Some(&Mode::ClientSide)
    );

    // Showing keeps csd; tiling it again restores the saved normal border
    // (sway/tree/container.c:998-1001).
    assert!(crate::command::execute(f.niri_state(), "scratchpad show")[0].success);
    f.double_roundtrip(client);
    assert_eq!(node(&mut f)["border"], "csd");
    assert!(crate::command::execute(f.niri_state(), "floating toggle")[0].success);
    f.double_roundtrip(client);
    assert_eq!(node(&mut f)["border"], "normal");
}

/// Differential seeds 16254 and 14873: after `split toggle` moves fullscreen onto a new
/// parent, the view keeps its borders under `smart_borders on`, because sway's
/// view_is_only_visible counts tree siblings, not what fullscreen leaves on screen
/// (sway/tree/view.c:327-342), and the fullscreen pass does not touch the child.
#[test]
fn smart_borders_count_siblings_hidden_by_fullscreen() {
    let config = swayward_config::Config::parse_mem(
        r#"layout {
            gaps 0
            smart-borders "on"
        }
        window-rule { sway-border "pixel"; sway-border-width 7; }"#,
    )
    .unwrap();
    let mut f = Fixture::with_config(config);
    f.add_output(1, (800, 600));
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
    let tree = |f: &mut Fixture, command: &str| {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(outcome[0].success, "{command}: {outcome:?}");
        f.double_roundtrip(client);
        let swayward = f.swayward();
        swayward.layout.update_render_elements(None);
        serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &swayward.marks_by_window,
            &swayward.marks_by_container,
        ))
        .unwrap()
    };
    // The fullscreen view itself fills the output with no border, as in sway's
    // view_autoconfigure fullscreen branch (sway/tree/view.c:356-368).
    let fullscreen = tree(&mut f, "fullscreen toggle");
    let leaf = &fullscreen["nodes"][1]["nodes"][0]["nodes"][1];
    assert_eq!(leaf["fullscreen_mode"], 1, "{leaf}");
    assert_eq!(
        leaf["window_rect"],
        serde_json::json!({ "x": 0, "y": 0, "width": 800, "height": 600 }),
        "{leaf}"
    );
    let tree = tree(&mut f, "split toggle");
    let split = &tree["nodes"][1]["nodes"][0]["nodes"][1];
    assert_eq!(split["fullscreen_mode"], 1, "{split}");
    let view = &split["nodes"][0];
    assert_eq!(view["fullscreen_mode"], 0, "{view}");
    assert_eq!(
        view["window_rect"],
        serde_json::json!({ "x": 7, "y": 7, "width": 786, "height": 586 }),
        "{view}"
    );
}

#[test]
fn a_view_without_server_decorations_maps_using_csd() {
    use smithay::reexports::wayland_protocols::xdg::decoration::zv1::client::zxdg_toplevel_decoration_v1::Mode;

    // Sway maps an xdg view as using CSD unless it negotiated server-side
    // decorations (handle_map, sway/desktop/xdg_shell.c:484-500). A view that
    // floats at map stores `csd` (container_set_floating,
    // sway/tree/container.c:955-965); a tiled one keeps `normal` and stores
    // `csd` once floated. Mapping into the tiling layer first also gives the
    // workspace a representation (sway/tree/view.c:895-902).
    // Differential family diff-fam-v3-hinted-csd-border, seeds 3 and 21;
    // oracle row hinted_view_without_decoration_maps_csd.
    let config = swayward_config::Config::parse_mem(
        r#"layout { default-border "normal" width=2; default-floating-border "normal" width=2; }"#,
    )
    .unwrap();
    let (mut f, _) = ipc_fixture_with_config(config);
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    let map = |f: &mut Fixture, app_id: &str, hints: Option<(i32, i32)>, mode: Option<Mode>| {
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(app_id.into());
        if let Some((width, height)) = hints {
            window.set_min_size(width, height);
            window.set_max_size(width, height);
        }
        if let Some(mode) = mode {
            f.client(client).decorate_last_window(mode);
        }
        let window = f.client(client).state.windows.last_mut().unwrap();
        let surface = window.surface.clone();
        window.commit();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    };

    map(&mut f, "fixed", Some((400, 300)), None);
    let tree = get_tree(&mut f);
    let workspace = &tree["nodes"][1]["nodes"][0];
    assert_eq!(workspace["floating_nodes"][0]["border"], "csd");
    assert_eq!(workspace["representation"], "H[]", "{workspace:#}");
    // A fullscreen floating view stays floating in sway and keeps `csd`
    // (seed 3).
    for command in ["fullscreen toggle global", "fullscreen toggle"] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success);
        let tree = get_tree(&mut f);
        let fixed = find_json_node_with_app_id(&tree, "fixed").unwrap();
        assert_eq!(fixed["border"], "csd", "{command}");
    }

    map(
        &mut f,
        "fixed-ssd",
        Some((300, 200)),
        Some(Mode::ServerSide),
    );
    map(
        &mut f,
        "fixed-csd",
        Some((300, 200)),
        Some(Mode::ClientSide),
    );
    map(&mut f, "tiled", None, None);
    let tree = get_tree(&mut f);
    let border = |app_id| find_json_node_with_app_id(&tree, app_id).unwrap()["border"].clone();
    assert_eq!(border("fixed-ssd"), "normal");
    assert_eq!(border("fixed-csd"), "csd");
    assert_eq!(border("tiled"), "normal");

    assert!(
        crate::command::execute(f.niri_state(), r#"[app_id="tiled"] floating enable"#)[0].success
    );
    let tree = get_tree(&mut f);
    assert_eq!(
        find_json_node_with_app_id(&tree, "tiled").unwrap()["border"],
        "csd"
    );
    assert!(
        crate::command::execute(f.niri_state(), r#"[app_id="tiled"] floating disable"#)[0].success
    );
    let tree = get_tree(&mut f);
    assert_eq!(
        find_json_node_with_app_id(&tree, "tiled").unwrap()["border"],
        "normal"
    );
}

#[test]
fn a_recreated_decoration_object_forgets_the_old_requested_mode() {
    use smithay::reexports::wayland_protocols::xdg::decoration::zv1::client::zxdg_toplevel_decoration_v1::Mode;

    // Sway reads the requested mode from the view's current decoration
    // object (handle_map, sway/desktop/xdg_shell.c:484-490), and wlroots
    // starts a fresh object at requested_mode NONE. A client-side request on
    // a destroyed object therefore no longer marks the view as using CSD.
    let config = swayward_config::Config::parse_mem(
        r#"layout { default-border "normal" width=2; default-floating-border "normal" width=2; }"#,
    )
    .unwrap();
    let (mut f, _) = ipc_fixture_with_config(config);
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("fixed".into());
    window.set_min_size(400, 300);
    window.set_max_size(400, 300);
    f.client(client).decorate_last_window(Mode::ClientSide);
    f.roundtrip(client);

    let state = &mut f.client(client).state;
    let window = state.windows.last_mut().unwrap();
    window.xdg_decoration.take().unwrap().destroy();
    let manager = state.xdg_decoration_manager.as_ref().unwrap();
    window.xdg_decoration =
        Some(manager.get_toplevel_decoration(&window.xdg_toplevel, &state.qh, ()));
    let surface = window.surface.clone();
    window.commit();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    let tree = get_tree(&mut f);
    let workspace = &tree["nodes"][1]["nodes"][0];
    assert_eq!(
        workspace["floating_nodes"][0]["border"], "normal",
        "{workspace:#}"
    );
}

#[test]
fn a_csd_view_floated_then_fullscreened_keeps_csd() {
    use smithay::reexports::wayland_protocols::xdg::decoration::zv1::client::zxdg_toplevel_decoration_v1::Mode;

    // A fullscreen floating view stays in `ws->floating` in sway, so a view
    // using CSD keeps the B_CSD that container_set_floating stored
    // (sway/tree/container.c:955-959; set_border, sway/commands/border.c:15-31).
    // Differential family diff-fam-csd-floating-fullscreen, seeds 10251,
    // 14299 and 14654.
    let sequences: [&[&str]; 3] = [
        &["border csd", "floating enable", "fullscreen enable"],
        &["fullscreen enable", "border toggle", "floating enable"],
        &["fullscreen toggle", "floating toggle", "border csd"],
    ];
    for commands in sequences {
        let (mut f, _) = ipc_fixture();
        f.add_output(1, (1920, 1080));
        let client = f.add_client();
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id("view".into());
        window.commit();
        let surface = window.surface.clone();
        f.client(client).decorate_last_window(Mode::ServerSide);
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);

        for command in commands {
            let outcome = crate::command::execute(f.niri_state(), command);
            assert!(outcome[0].success, "{command}: {outcome:?}");
            f.double_roundtrip(client);
        }
        let tree = get_tree(&mut f);
        let view = find_json_node_with_app_id(&tree, "view").unwrap();
        assert_eq!(view["border"], "csd", "{commands:?}: {view:#}");
        assert_eq!(view["type"], "floating_con", "{commands:?}");
    }
}

#[test]
fn a_hinted_dialog_split_wraps_the_centred_content_box() {
    use smithay::reexports::wayland_protocols::xdg::shell::client::xdg_toplevel::XdgToplevel;

    // A dialog that binds no decoration protocol maps floating with csd
    // (handle_map, sway/desktop/xdg_shell.c:484-500). Sway centres the natural
    // size clamped to floating_minimum_size, 75x50, on the workspace rather
    // than over the parent (floating_natural_resize and
    // container_floating_resize_and_center, sway/tree/container.c:833-894),
    // then shrinks the box to the 1x1 the client commits around the same
    // content origin. `split h` wraps that box and restores the normal border;
    // the 1 px child is below sway's 10 px lower bound, so it gets an empty box
    // and GET_TREE reports it less the titlebar (sway/tree/arrange.c:91-94,
    // sway/ipc-json.c:816-825). Family diff-fam-v3-hinted-floating-split-content-rect,
    // seed 31752; oracle row hinted_floating_dialog_split_keeps_content.
    let config = swayward_config::Config::parse_mem(
        r#"layout { default-border "normal" width=2; default-floating-border "normal" width=2; }"#,
    )
    .unwrap();
    let (mut f, _) = ipc_fixture_with_config(config);
    f.add_output(1, (1270, 1408));
    let client = f.add_client();
    let map = |f: &mut Fixture, app_id: &str, parent: Option<&XdgToplevel>| {
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(app_id.into());
        window.set_parent(parent);
        let toplevel = window.xdg_toplevel.clone();
        let surface = window.surface.clone();
        window.commit();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        toplevel
    };
    let parent = map(&mut f, "parent", None);
    map(&mut f, "dialog", Some(&parent));

    let tree = get_tree(&mut f);
    let dialog = &tree["nodes"][1]["nodes"][0]["floating_nodes"][0];
    assert_eq!(dialog["border"], "csd", "{dialog:#}");
    assert_eq!(
        dialog["rect"],
        serde_json::json!({"x": 597, "y": 679, "width": 1, "height": 1}),
        "{dialog:#}"
    );

    assert!(crate::command::execute(f.niri_state(), "split h")[0].success);
    f.double_roundtrip(client);
    let tree = get_tree(&mut f);
    let split = &tree["nodes"][1]["nodes"][0]["floating_nodes"][0];
    assert_eq!(split["layout"], "splith", "{split:#}");
    assert_eq!(
        split["rect"],
        serde_json::json!({"x": 597, "y": 679, "width": 1, "height": 1}),
        "{split:#}"
    );
    let child = &split["nodes"][0];
    assert_eq!(child["border"], "normal", "{child:#}");
    let titlebar = child["deco_rect"]["height"].as_i64().unwrap();
    assert!(titlebar > 0, "{child:#}");
    assert_eq!(
        child["rect"],
        serde_json::json!({"x": 597, "y": 679 + titlebar, "width": 0, "height": -titlebar}),
        "{child:#}"
    );

    // A stacked view takes the whole 1x1 box less the titlebar row
    // (apply_stacked_layout, sway/tree/arrange.c:199-211); seed 31816.
    assert!(crate::command::execute(f.niri_state(), "layout stacking")[0].success);
    f.double_roundtrip(client);
    let tree = get_tree(&mut f);
    let child = &tree["nodes"][1]["nodes"][0]["floating_nodes"][0]["nodes"][0];
    assert_eq!(
        child["rect"],
        serde_json::json!({"x": 597, "y": 679 + titlebar, "width": 1, "height": 1 - titlebar}),
        "{child:#}"
    );
}

#[test]
fn a_min_hinted_view_floated_from_tiling_centres_the_minimum_content_box() {
    // Floating a tiled view runs container_floating_set_default_size and then
    // container_floating_resize_and_center (sway/tree/container.c:966-968). The
    // natural size, the 1x1 the client committed at map, is clamped by
    // floating_minimum_size alone, not by the client's 200x150 min hint
    // (floating_natural_resize, sway/tree/container.c:833-847), so the view
    // floats as a 75x50 content box centred on the workspace. The client's 1x1
    // geometry does not change, so no commit shrinks it (handle_commit,
    // sway/desktop/xdg_shell.c:313-335). `split h` then wraps that box.
    // Family diff-fam-v3-hinted-floating-split-content-rect-residual-1, seed
    // 32047; oracle row hinted_min_size_float_enable_split_keeps_content.
    let config = swayward_config::Config::parse_mem(
        r#"layout { default-border "normal" width=2; default-floating-border "normal" width=2; }"#,
    )
    .unwrap();
    let (mut f, _) = ipc_fixture_with_config(config);
    f.add_output(1, (1270, 1408));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("hinted".into());
    window.set_min_size(200, 150);
    let surface = window.surface.clone();
    window.commit();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    for command in ["floating enable", "split h"] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success);
        f.double_roundtrip(client);
        // The client acks without resizing: its buffer stays 1x1.
        let window = f.client(client).window(&surface);
        let last = window.configures_received.last().map(|(serial, _)| *serial);
        if last != window.last_acked_configure {
            window.ack_last_and_commit();
            f.double_roundtrip(client);
        }
    }
    let tree = get_tree(&mut f);
    let split = &tree["nodes"][1]["nodes"][0]["floating_nodes"][0];
    assert_eq!(split["layout"], "splith", "{split:#}");
    assert_eq!(
        split["rect"],
        serde_json::json!({"x": 597, "y": 679, "width": 75, "height": 50}),
        "{split:#}"
    );
    let child = &split["nodes"][0];
    let titlebar = child["deco_rect"]["height"].as_i64().unwrap();
    assert_eq!(
        child["rect"],
        serde_json::json!({"x": 597, "y": 679 + titlebar, "width": 75, "height": 50 - titlebar}),
        "{child:#}"
    );
}

#[test]
fn a_held_floating_content_box_takes_a_resize_on_the_named_axis_only() {
    // `resize set width` changes the content width around the centre, clamped by the
    // floating limits only, and leaves the height alone (resize_set_floating,
    // sway/commands/resize.c:341-403). The client's 200x150 min hint does not reach the
    // held 75x50 box. Seed 32047 after the floating box fix.
    let (mut f, _) = ipc_fixture();
    f.add_output(1, (1270, 1408));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("hinted".into());
    window.set_min_size(200, 150);
    let surface = window.surface.clone();
    window.commit();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    for command in ["floating enable", "resize set width 50 ppt"] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success);
        f.double_roundtrip(client);
        let window = f.client(client).window(&surface);
        let last = window.configures_received.last().map(|(serial, _)| *serial);
        if last != window.last_acked_configure {
            window.ack_last_and_commit();
            f.double_roundtrip(client);
        }
    }
    let tree = get_tree(&mut f);
    let view = &tree["nodes"][1]["nodes"][0]["floating_nodes"][0];
    assert_eq!(view["border"], "csd", "{view:#}");
    assert_eq!(
        view["rect"],
        serde_json::json!({"x": 317, "y": 679, "width": 635, "height": 50}),
        "{view:#}"
    );
}
