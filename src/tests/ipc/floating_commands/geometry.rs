#[test]
fn floating_grow_edges_change_origin_and_size_like_sway() {
    let mut config = swayward_config::Config::default();
    config.animations.off = true;
    config.layout.floating_maximum_size = swayward_config::FloatingSize {
        width: 1280,
        height: 800,
    };
    let mut f = Fixture::with_config(config);
    f.add_output_at(1, (1280, 800), Some((100, 50)));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);
    assert!(crate::command::execute(f.niri_state(), "resize set 400 px 300 px")[0].success);
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

    let rect = |f: &mut Fixture| {
        f.niri_state().ipc_refresh_layout();
        let swayward = f.swayward();
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &Default::default(),
            &Default::default(),
        ))
        .unwrap();
        find_json_node(&tree, "floating_con", false).unwrap()["rect"].clone()
    };

    for (direction, delta) in [
        ("left", (-10, 0, 10, 0)),
        ("right", (0, 0, 10, 0)),
        ("up", (0, -10, 0, 10)),
        ("down", (0, 0, 0, 10)),
    ] {
        let before = rect(&mut f);
        let outcome = crate::command::execute(
            f.niri_state(),
            &format!("resize grow {direction} 10 px or 25 ppt"),
        );
        assert!(outcome[0].success, "{direction}: {outcome:?}");
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
        let after = rect(&mut f);
        assert_eq!(
            after["x"].as_i64(),
            before["x"].as_i64().map(|x| x + delta.0)
        );
        assert_eq!(
            after["y"].as_i64(),
            before["y"].as_i64().map(|y| y + delta.1)
        );
        assert_eq!(
            after["width"].as_i64(),
            before["width"].as_i64().map(|width| width + delta.2)
        );
        assert_eq!(
            after["height"].as_i64(),
            before["height"].as_i64().map(|height| height + delta.3)
        );
    }

    assert!(crate::command::execute(f.niri_state(), "resize set 1280 px 800 px")[0].success);
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
    let before = rect(&mut f);
    assert_eq!(before["width"], 1280);
    assert_eq!(before["height"], 800);
    for command in [
        "resize grow right 10 px or 25 ppt",
        "resize grow width 10 px or 25 ppt",
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
        assert_eq!(rect(&mut f), before, "{command}");
    }

    assert!(crate::command::execute(f.niri_state(), "resize set 1 px 1 px")[0].success);
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
    let before = rect(&mut f);
    let outcome = crate::command::execute(f.niri_state(), "resize shrink height 10 px");
    assert_eq!(
        outcome,
        [swayward_ipc::CommandOutcome {
            success: false,
            error: Some("Cannot resize any further".into()),
            parse_error: Some(true),
        }]
    );
    assert_eq!(rect(&mut f), before);
}

#[test]
fn move_command_rejects_fullscreen_floating_windows() {
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
    assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);
    assert!(crate::command::execute(f.niri_state(), "fullscreen enable")[0].success);

    let outcome = crate::command::execute(f.niri_state(), "move left");
    assert!(!outcome[0].success);
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("Cannot move fullscreen floating container")
    );
}

/// Oracle: fullscreen_floating_group_move_refused. A fullscreen floating
/// group root is a fullscreen floating container too
/// (`cmd_move_in_direction`, sway/commands/move.c:688-692). Random seed 314
/// step 18.
#[test]
fn directional_move_of_a_fullscreen_floating_group_is_refused() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
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
    for command in ["focus parent", "floating toggle", "fullscreen toggle"] {
        let reply = crate::command::execute(f.niri_state(), command);
        assert!(reply[0].success, "{command}: {reply:?}");
    }

    let outcome = crate::command::execute(f.niri_state(), "move left");
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("Cannot move fullscreen floating container")
    );
}

#[test]
fn move_command_uses_sway_floating_pixel_distances() {
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
    assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);

    let rect = |f: &mut Fixture| {
        let swayward = f.swayward();
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &Default::default(),
            &Default::default(),
        ))
        .unwrap();
        find_json_node(&tree, "floating_con", false).unwrap()["rect"].clone()
    };
    let before = rect(&mut f);

    assert!(crate::command::execute(f.niri_state(), "move left")[0].success);
    let moved = rect(&mut f);
    assert_eq!(moved["x"].as_i64(), before["x"].as_i64().map(|x| x - 10));

    assert!(crate::command::execute(f.niri_state(), "move down 20 px")[0].success);
    let moved = rect(&mut f);
    assert_eq!(moved["y"].as_i64(), before["y"].as_i64().map(|y| y + 20));
}

#[test]
fn move_position_uses_workspace_coordinates_and_rejects_absolute_ppt() {
    let mut f = Fixture::new();
    f.add_output(1, (1000, 800));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);

    let rect = |f: &mut Fixture| {
        let swayward = f.swayward();
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &Default::default(),
            &Default::default(),
        ))
        .unwrap();
        find_json_node(&tree, "floating_con", false).unwrap()["rect"].clone()
    };

    assert!(crate::command::execute(f.niri_state(), "move position 5 px 15")[0].success);
    assert_eq!(rect(&mut f)["x"], 5);
    assert_eq!(rect(&mut f)["y"], 15);

    assert!(crate::command::execute(f.niri_state(), "move position 20 ppt 25 ppt")[0].success);
    assert_eq!(rect(&mut f)["x"], 200);
    assert_eq!(rect(&mut f)["y"], 200);

    for command in [
        "move absolute position 20 ppt 5 px",
        "move absolute position 5 px 20 ppt",
    ] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(!outcome[0].success);
        assert_eq!(
            outcome[0].error.as_deref(),
            Some("Cannot move to absolute positions by ppt")
        );
    }
}

/// Oracle: floating_window_ppt_resize_rejected. A floating view resizes only
/// in px or unitless amounts; a ppt-only resize is refused and leaves the
/// window alone (sway/commands/resize.c:523-537). Differential seed 1136.
#[test]
fn ppt_only_resize_of_a_floating_window_is_refused() {
    let mut f = Fixture::new();
    f.add_output(1, (1000, 800));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);

    let rect = |f: &mut Fixture| {
        f.niri_state().ipc_refresh_layout();
        let swayward = f.swayward();
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &Default::default(),
            &Default::default(),
        ))
        .unwrap();
        find_json_node(&tree, "floating_con", false).unwrap()["rect"].clone()
    };
    let before = rect(&mut f);

    for command in [
        "resize shrink width 5 ppt",
        "resize grow height 10 ppt",
        "resize grow left 5 ppt",
    ] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(!outcome[0].success, "{command}: {outcome:?}");
        assert_eq!(
            outcome[0].error.as_deref(),
            Some("Floating containers cannot use ppt measurements"),
            "{command}"
        );
        assert_eq!(rect(&mut f), before, "{command}");
    }
}

#[test]
fn move_absolute_position_is_verbatim_under_a_bar_and_gaps() {
    // `move absolute position` must use the requested coordinate unchanged.
    // Sway applies the workspace origin only on the relative form
    // (`sway/sway/commands/move.c:913-916`) and then calls
    // container_floating_move_to, which performs no bounds check
    // (`sway/sway/tree/container.c:1113-1145`).
    // A full-workspace-height window leaves no slack for an accidental second
    // application of the bar or gap offset.
    let mut config = swayward_config::Config::default();
    config.animations.off = true;
    config.layout.gaps = 4.;
    let mut f = Fixture::with_config(config);
    f.add_output(1, (800, 600));
    let client = f.add_client();

    // A top bar, so the workspace origin is not the output origin.
    use smithay::reexports::wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::Layer;
    use smithay::reexports::wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::Anchor;

    let layer = f.client(client).create_layer(None, Layer::Top, "bar");
    layer.set_configure_props(crate::tests::client::LayerConfigureProps {
        anchor: Some(Anchor::Left | Anchor::Right | Anchor::Top),
        size: Some((0, 20)),
        exclusive_zone: Some(20),
        ..Default::default()
    });
    let layer_surface = layer.surface.clone();
    layer.commit();
    f.double_roundtrip(client);
    let layer = f.client(client).layer(&layer_surface);
    let size = layer.configures_received.last().unwrap().1.size;
    layer.attach_new_buffer();
    layer.set_size(size.0 as u16, size.1 as u16);
    layer.ack_last_and_commit();
    f.double_roundtrip(client);

    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);

    // find_json_node would return the hidden __i3 scratch workspace, whose
    // rect is all zeroes.
    fn visible<'a>(node: &'a Value, kind: &str) -> Option<&'a Value> {
        if node["type"] == kind && node["name"] != "__i3" && node["name"] != "__i3_scratch" {
            return Some(node);
        }
        ["nodes", "floating_nodes"]
            .into_iter()
            .find_map(|key| node[key].as_array()?.iter().find_map(|c| visible(c, kind)))
    }

    let node = |f: &mut Fixture, kind: &str| {
        let swayward = f.swayward();
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &Default::default(),
            &Default::default(),
        ))
        .unwrap();
        visible(&tree, kind).unwrap()["rect"].clone()
    };

    let ws = node(&mut f, "workspace");
    let (ws_x, ws_y) = (ws["x"].as_i64().unwrap(), ws["y"].as_i64().unwrap());
    let (ws_w, ws_h) = (
        ws["width"].as_i64().unwrap(),
        ws["height"].as_i64().unwrap(),
    );
    assert_eq!(
        ws_y, 24,
        "workspace rect starts below the bar and outer gap"
    );

    // The tallCenter preset from a real script: full workspace height at the
    // workspace origin, computed from the IPC workspace rect. Zero slack, so
    // any displacement overflows.
    let command =
        format!("resize set {ws_w} px {ws_h} px, move absolute position {ws_x} px {ws_y} px");
    assert!(crate::command::execute(f.niri_state(), &command)[0].success);
    f.double_roundtrip(client);

    let rect = node(&mut f, "floating_con");
    assert_eq!(
        rect["y"], ws_y,
        "absolute y must be used verbatim, not offset by the bar and gaps"
    );
    assert_eq!(rect["x"], ws_x, "absolute x must be used verbatim");

    let bottom = rect["y"].as_i64().unwrap() + rect["height"].as_i64().unwrap();
    assert!(
        bottom <= ws_y + ws_h,
        "a full-height window overflowed the workspace: bottom {bottom} > {}",
        ws_y + ws_h
    );
}

#[test]
fn move_position_centers_on_root_and_pointer() {
    let mut f = Fixture::new();
    f.add_output_at(1, (1000, 800), Some((100, 50)));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);

    let rect = |f: &mut Fixture| {
        let swayward = f.swayward();
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &Default::default(),
            &Default::default(),
        ))
        .unwrap();
        find_json_node(&tree, "floating_con", false).unwrap()["rect"].clone()
    };

    assert!(crate::command::execute(f.niri_state(), "move position 150 75")[0].success);
    let relative = rect(&mut f);
    assert_eq!(relative["x"], 250);
    assert_eq!(relative["y"], 125);

    assert!(crate::command::execute(f.niri_state(), "move absolute position 150 75")[0].success);
    let absolute = rect(&mut f);
    assert_eq!(absolute["x"], 150);
    assert_eq!(absolute["y"], 75);

    assert!(crate::command::execute(f.niri_state(), "move absolute position center")[0].success);
    let centered = rect(&mut f);
    assert_eq!(centered["x"], 600);
    assert_eq!(centered["y"], 450);

    f.niri_state().move_cursor((300., 250.).into());
    assert!(crate::command::execute(f.niri_state(), "move position pointer")[0].success);
    let pointer = rect(&mut f);
    assert_eq!(pointer["x"], 300);
    assert_eq!(pointer["y"], 250);
}

#[test]
fn move_position_targets_floating_windows_by_criteria() {
    let mut f = Fixture::new();
    f.add_output(1, (1000, 800));
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
        assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);
    }

    assert!(
        crate::command::execute(
            f.niri_state(),
            r#"[app_id="first"] move position 25 px 30 px"#,
        )[0]
        .success
    );

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    fn find_first(value: &Value) -> Option<&Value> {
        if value["app_id"] == "first" {
            return Some(value);
        }
        ["nodes", "floating_nodes"]
            .into_iter()
            .find_map(|key| value[key].as_array()?.iter().find_map(find_first))
    }
    let first = find_first(&tree).unwrap();
    assert_eq!(first["rect"]["x"], 25);
    assert_eq!(first["rect"]["y"], 30);
}

#[test]
fn floating_ipc_rect_uses_final_position_during_animation() {
    let (mut f, _) = ipc_fixture();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("animated".into());
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    f.swayward().layout.toggle_window_floating(None);
    f.swayward().layout.move_floating_window(
        None,
        swayward_ipc::PositionChange::SetFixed(100.),
        swayward_ipc::PositionChange::SetFixed(200.),
        true,
    );

    let tree = get_tree(&mut f);
    let node = find_json_node(&tree, "floating_con", false).unwrap();
    assert_eq!(node["rect"]["x"], 100);
    assert_eq!(node["rect"]["y"], 200);
}

#[test]
fn floating_input_region_holes_click_through_but_decorations_activate() {
    let mut config = swayward_config::Config::default();
    config.animations.off = true;
    config.layout.border.off = false;
    let mut f = Fixture::with_config(config);
    f.add_output(1, (800, 600));
    let client = f.add_client();
    let mut surfaces = Vec::new();
    for app_id in ["bottom", "top"] {
        let window = f.client(client).create_ssd_window();
        window.xdg_toplevel.set_app_id(app_id.into());
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);
        assert!(
            crate::command::execute(f.niri_state(), "resize set width 200 height 100")[0].success
        );
        let window = f.client(client).window(&surface);
        window.set_size(200, 100);
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        let id = f.swayward().layout.focus().unwrap().window.clone();
        f.swayward().layout.move_floating_window(
            Some(&id),
            swayward_ipc::PositionChange::SetFixed(100.),
            swayward_ipc::PositionChange::SetFixed(100.),
            false,
        );
        surfaces.push(surface);
    }

    let focused_app_id = |f: &mut Fixture| {
        f.swayward().layout.focus().and_then(|window| {
            crate::utils::with_toplevel_role(window.toplevel(), |role| role.app_id.clone())
        })
    };
    let (bottom, tile_pos, window_loc) = {
        let workspace = f.swayward().layout.active_workspace().unwrap();
        let bottom = workspace
            .windows()
            .find(|window| {
                crate::utils::with_toplevel_role(window.toplevel(), |role| {
                    role.app_id.as_deref() == Some("bottom")
                })
            })
            .unwrap()
            .window
            .clone();
        let (tile, tile_pos, _) = workspace
            .tiles_with_render_positions()
            .find(|(tile, _, _)| {
                crate::utils::with_toplevel_role(tile.window().toplevel(), |role| {
                    role.app_id.as_deref() == Some("top")
                })
            })
            .unwrap();
        (bottom, tile_pos, tile.window_loc())
    };
    let inside = tile_pos + window_loc + smithay::utils::Point::from((25., 25.));
    let outside = tile_pos + window_loc + smithay::utils::Point::from((175., 25.));
    let border_in_tile = smithay::utils::Point::from((window_loc.x / 2., window_loc.y + 10.));

    f.client(client)
        .set_input_region(&surfaces[1], Some((0, 0, 100, 100)));
    f.double_roundtrip(client);
    f.niri_state().move_cursor(inside);
    pointer_button(&mut f, 0x110, true);
    pointer_button(&mut f, 0x110, false);
    assert_eq!(focused_app_id(&mut f).as_deref(), Some("top"));

    f.swayward().layout.activate_window_without_raising(&bottom);
    let output = f.niri_output(1);
    assert_eq!(
        f.swayward()
            .layout
            .window_under(&output, outside)
            .and_then(|(window, _)| {
                crate::utils::with_toplevel_role(window.toplevel(), |role| role.app_id.clone())
            })
            .as_deref(),
        Some("bottom")
    );
    f.niri_state().move_cursor(outside);
    pointer_button(&mut f, 0x110, true);
    pointer_button(&mut f, 0x110, false);
    assert_eq!(focused_app_id(&mut f).as_deref(), Some("bottom"));

    f.client(client).set_input_region(&surfaces[1], None);
    f.double_roundtrip(client);
    f.niri_state().move_cursor(inside);
    pointer_button(&mut f, 0x110, true);
    pointer_button(&mut f, 0x110, false);
    assert_eq!(focused_app_id(&mut f).as_deref(), Some("bottom"));

    f.client(client).reset_input_region(&surfaces[1]);
    f.double_roundtrip(client);
    assert!(crate::command::execute(f.niri_state(), r#"[app_id="top"] focus"#)[0].success);
    f.swayward().layout.activate_window_without_raising(&bottom);
    f.niri_state().move_cursor(outside);
    pointer_button(&mut f, 0x110, true);
    pointer_button(&mut f, 0x110, false);
    assert_eq!(focused_app_id(&mut f).as_deref(), Some("top"));

    assert!(crate::command::execute(f.niri_state(), r#"[app_id="top"] focus"#)[0].success);
    let workspace = f.swayward().layout.active_workspace().unwrap();
    let (tile, _, _) = workspace
        .tiles_with_render_positions()
        .find(|(tile, _, _)| {
            crate::utils::with_toplevel_role(tile.window().toplevel(), |role| {
                role.app_id.as_deref() == Some("top")
            })
        })
        .unwrap();
    assert_eq!(
        tile.hit(border_in_tile),
        Some(crate::layout::HitType::Activate {
            is_tab_indicator: false
        })
    );
}

#[test]
fn floating_stacking_and_focus_match_sway_before_and_after_raise() {
    let two: Value = serde_json::from_str(&sway_fixture!("two_floating.tree.json")).unwrap();
    assert_eq!(
        floating_order(&two),
        (
            vec!["fixture-1", "fixture-2"],
            vec!["fixture-2", "fixture-1"]
        )
    );

    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    for title in ["fixture-tiled", "fixture-1", "fixture-2", "fixture-3"] {
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(title.into());
        window.set_title(title);
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        if title != "fixture-tiled" {
            assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);
        }
    }

    let describe = |f: &mut Fixture| {
        let swayward = f.swayward();
        serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &Default::default(),
            &Default::default(),
        ))
        .unwrap()
    };
    let before: Value =
        serde_json::from_str(&sway_fixture!("three_floating_before_raise.tree.json")).unwrap();
    assert_eq!(floating_order(&describe(&mut f)), floating_order(&before));

    assert!(crate::command::execute(f.niri_state(), r#"[app_id="^fixture-1$"] focus"#)[0].success);
    let after: Value =
        serde_json::from_str(&sway_fixture!("three_floating_after_raise.tree.json")).unwrap();
    assert_eq!(floating_order(&describe(&mut f)), floating_order(&after));
}

/// Oracle rows sway-1.12-random seeds 39, 127, 290 and 307: `floating toggle` on a tiled
/// view floats it at its natural size with the content box centered on the workspace
/// (`container_floating_resize_and_center`, sway/tree/container.c:850-894), clears the
/// tiled edges (sway/tree/container.c:955-956), and a split keeps that box.
#[test]
fn floating_toggle_centers_the_natural_size_and_split_keeps_it() {
    // The oracle's swayward harness config (sway-ipc-run `SwaywardAdapter`).
    let mut config = swayward_config::Config::parse_mem(
        r#"layout { default-border "normal" width=2; default-floating-border "normal" width=2; border { on; width 2; }; }"#,
    )
    .unwrap();
    config.animations.off = true;
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    let surface = f.client(client).create_window().surface.clone();
    f.client(client).decorate_last_window(
        smithay::reexports::wayland_protocols::xdg::decoration::zv1::client::zxdg_toplevel_decoration_v1::Mode::ServerSide,
    );
    f.client(client).window(&surface).commit();
    f.roundtrip(client);
    let initial = f.client(client).window(&surface).format_recent_configures();
    assert!(
        initial.starts_with("size: 0 × 0,") && initial.ends_with("states: []"),
        "the initial configure carries no size and no tiled edges: {initial}"
    );
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.set_size(696, 491);
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    let window = f.client(client).window(&surface);
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    let _ = f.client(client).window(&surface).recent_configures();

    assert!(crate::command::execute(f.niri_state(), "floating toggle")[0].success);
    f.double_roundtrip(client);
    let configure = f.client(client).window(&surface).format_recent_configures();
    let last = configure.lines().last().unwrap_or_default();
    assert!(
        last.starts_with("size: 696 × 491,") && last.ends_with("states: [Activated]"),
        "floating restores the natural size without tiled edges: {configure}"
    );
    let window = f.client(client).window(&surface);
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    let tree = |f: &mut Fixture| {
        f.niri_state().ipc_refresh_layout();
        let swayward = f.swayward();
        serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &Default::default(),
            &Default::default(),
        ))
        .unwrap()
    };
    // The content box (696x491) is centered on the 1280x720 workspace: x 292, y 114.5,
    // which sway reports truncated. The container adds the 2 px border on three sides and
    // the titlebar on top.
    let floating = tree(&mut f);
    let floating = find_json_node(&floating, "floating_con", false).unwrap();
    let titlebar = floating["deco_rect"]["height"].as_i64().unwrap();
    assert!(titlebar > 0, "{floating}");
    let content_top = 114 - titlebar;
    let expected = serde_json::json!({
        "x": 290, "y": content_top, "width": 700, "height": 493 + titlebar,
    });
    assert_eq!(floating["rect"], expected, "{floating}");
    assert_eq!(
        floating["geometry"],
        serde_json::json!({"x": 0, "y": 0, "width": 696, "height": 491}),
        "{floating}"
    );

    assert!(crate::command::execute(f.niri_state(), "splith")[0].success);
    f.double_roundtrip(client);
    let split = tree(&mut f);
    let split = find_json_node(&split, "floating_con", false).unwrap();
    assert_eq!(split["layout"], "splith", "{split}");
    assert_eq!(split["rect"], expected, "{split}");
    let child = &split["nodes"][0];
    assert_eq!(
        child["rect"],
        serde_json::json!({"x": 290, "y": content_top + titlebar, "width": 700, "height": 493}),
        "{child}"
    );
}

/// Differential family floating-split-rect (random-v2 seeds 1263, 2212, 2247, 5572; oracle row
/// `floating_split_keeps_rect`): a split that arrives before the client commits its floating
/// size still wraps the box `container_floating_resize_and_center` gave the view, because
/// `container_split` copies the container's pending geometry (sway/tree/container.c:1543-1548).
#[test]
fn split_before_the_floating_commit_keeps_the_floating_box() {
    let mut config = swayward_config::Config::parse_mem(
        r#"layout { default-border "normal" width=2; default-floating-border "normal" width=2; border { on; width 2; }; }"#,
    )
    .unwrap();
    config.animations.off = true;
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1270, 1408));
    let client = f.add_client();
    let surface = f.client(client).create_window().surface.clone();
    f.client(client).decorate_last_window(
        smithay::reexports::wayland_protocols::xdg::decoration::zv1::client::zxdg_toplevel_decoration_v1::Mode::ServerSide,
    );
    f.client(client).window(&surface).commit();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.set_size(696, 491);
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    // Tiled at the full workspace, the client commits that size.
    let window = f.client(client).window(&surface);
    window.set_size(1266, 1379);
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);
    f.double_roundtrip(client);
    // The client has not committed the floating size yet.
    assert!(crate::command::execute(f.niri_state(), "splith")[0].success);

    f.niri_state().ipc_refresh_layout();
    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    let split = find_json_node(&tree, "floating_con", false).unwrap();
    let child = &split["nodes"][0];
    let titlebar = child["deco_rect"]["height"].as_i64().unwrap();
    assert_eq!(split["layout"], "splith", "{split}");
    assert_eq!(split["rect"]["width"], 700, "{split}");
    assert_eq!(split["rect"]["height"], 493 + titlebar, "{split}");
    assert_eq!(child["rect"]["width"], 700, "{child}");
    assert_eq!(child["rect"]["height"], 493, "{child}");
}

/// Differential family floating-split-toggle-border (random-v2 seeds 10229, 10238, 10622;
/// oracle row `floating_split_toggle_wraps`): `split toggle` and `splitt` on a floating view
/// wrap it in a new split like `splith`/`splitv`. The floater has no parent, so
/// `container_parent_layout` reads the workspace layout and the split is V unless that is V
/// (sway/commands/split.c:64-71,104-115, sway/tree/container.c:1353-1361).
#[test]
fn split_toggle_wraps_a_floating_view_against_the_workspace_layout() {
    // A portrait output gives the workspace a splitv default layout
    // (`output_get_default_layout`, sway/tree/output.c).
    for (output, command, expected) in [
        ((1280, 720), "split toggle", "splitv"),
        ((1280, 720), "splitt", "splitv"),
        ((720, 1280), "split toggle", "splith"),
        ((720, 1280), "splitt", "splith"),
    ] {
        let mut f = Fixture::new();
        f.add_output(1, output);
        let client = f.add_client();
        let window = f.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);
        f.niri_state().ipc_refresh_layout();
        assert!(crate::command::execute(f.niri_state(), command)[0].success);

        f.niri_state().ipc_refresh_layout();
        let swayward = f.swayward();
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &Default::default(),
            &Default::default(),
        ))
        .unwrap();
        let split = find_json_node(&tree, "floating_con", false).unwrap();
        assert_eq!(
            split["layout"], expected,
            "{command} on {output:?}: {split}"
        );
        assert_eq!(split["nodes"].as_array().map(Vec::len), Some(1), "{split}");
        assert_eq!(split["nodes"][0]["focused"], true, "{split}");
    }
}

/// Differential family floating-split-rect, random-v2 seed 2247: `border pixel 3` on a
/// floating view keeps its content box and moves the container around it
/// (`container_set_geometry_from_content`, sway/commands/border.c:94-96,
/// sway/tree/container.c:1018-1039), so a later split wraps the moved box.
#[test]
fn floating_border_change_keeps_the_content_box() {
    let mut config = swayward_config::Config::parse_mem(
        r#"layout { default-border "normal" width=2; default-floating-border "normal" width=2; border { on; width 2; }; }"#,
    )
    .unwrap();
    config.animations.off = true;
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1270, 1408));
    let client = f.add_client();
    let surface = f.client(client).create_window().surface.clone();
    f.client(client).decorate_last_window(
        smithay::reexports::wayland_protocols::xdg::decoration::zv1::client::zxdg_toplevel_decoration_v1::Mode::ServerSide,
    );
    f.client(client).window(&surface).commit();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.set_size(696, 491);
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    assert!(crate::command::execute(f.niri_state(), "floating toggle")[0].success);
    f.double_roundtrip(client);
    let window = f.client(client).window(&surface);
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    let floating = |f: &mut Fixture| {
        f.niri_state().ipc_refresh_layout();
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
    };
    let before = floating(&mut f);
    let content = |node: &Value| {
        (
            node["rect"]["x"].as_i64().unwrap() + node["window_rect"]["x"].as_i64().unwrap(),
            node["rect"]["y"].as_i64().unwrap()
                + node["deco_rect"]["height"].as_i64().unwrap()
                + node["window_rect"]["y"].as_i64().unwrap(),
        )
    };
    let (content_x, content_y) = content(&before);

    assert!(crate::command::execute(f.niri_state(), "border pixel 3")[0].success);
    let after = floating(&mut f);
    assert_eq!(content(&after), (content_x, content_y), "{before}\n{after}");
    assert_eq!(
        after["rect"],
        serde_json::json!({"x": content_x - 3, "y": content_y - 3, "width": 702, "height": 497}),
        "{after}"
    );

    assert!(crate::command::execute(f.niri_state(), "split v")[0].success);
    let split = floating(&mut f);
    assert_eq!(split["rect"], after["rect"], "{split}");
    assert_eq!(split["nodes"][0]["rect"], after["rect"], "{split}");
}

/// Differential family csd-floating-split-rect, random-v2 seed 13795 (oracle row
/// `floating_csd_split_keeps_content`): `border csd` on a floating view shrinks the container
/// to its content box (sway/commands/border.c:94-96), and a later `split v` wraps that box.
/// The wrapped child gets the default `normal` border back, so its titlebar sits inside the
/// wrapper and the child rect starts below it.
#[test]
fn floating_csd_split_wraps_the_content_box() {
    let mut config = swayward_config::Config::parse_mem(
        r#"layout { default-border "normal" width=2; default-floating-border "normal" width=2; border { on; width 2; }; }"#,
    )
    .unwrap();
    config.animations.off = true;
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1270, 1408));
    let client = f.add_client();
    let surface = f.client(client).create_window().surface.clone();
    f.client(client).decorate_last_window(
        smithay::reexports::wayland_protocols::xdg::decoration::zv1::client::zxdg_toplevel_decoration_v1::Mode::ServerSide,
    );
    f.client(client).window(&surface).commit();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.set_size(696, 491);
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);
    f.double_roundtrip(client);
    let window = f.client(client).window(&surface);
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    let floating = |f: &mut Fixture| {
        f.niri_state().ipc_refresh_layout();
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
    };
    let before = floating(&mut f);
    let content_x =
        before["rect"]["x"].as_i64().unwrap() + before["window_rect"]["x"].as_i64().unwrap();
    let content_y = before["rect"]["y"].as_i64().unwrap()
        + before["deco_rect"]["height"].as_i64().unwrap()
        + before["window_rect"]["y"].as_i64().unwrap();

    assert!(crate::command::execute(f.niri_state(), "border csd")[0].success);
    f.double_roundtrip(client);
    let csd = floating(&mut f);
    let content = serde_json::json!({"x": content_x, "y": content_y, "width": 696, "height": 491});
    assert_eq!(csd["border"], "csd", "{csd}");
    assert_eq!(csd["rect"], content, "{before}\n{csd}");

    assert!(crate::command::execute(f.niri_state(), "split v")[0].success);
    f.double_roundtrip(client);
    let split = floating(&mut f);
    assert_eq!(split["layout"], "splitv", "{split}");
    assert_eq!(split["rect"], content, "{split}");
    let child = &split["nodes"][0];
    let titlebar = child["deco_rect"]["height"].as_i64().unwrap();
    assert!(titlebar > 0, "{child}");
    assert_eq!(child["border"], "normal", "{child}");
    assert_eq!(
        child["rect"],
        serde_json::json!({"x": content_x, "y": content_y + titlebar, "width": 696, "height": 491 - titlebar}),
        "{child}"
    );
}

/// An output rescale leaves a floater where it was and lets the client pick
/// its own new size. sway's arrange_workspace moves floaters only when the
/// workspace origin moves (sway/sway/tree/arrange.c:277-304), sends no
/// configure bounds (sway/sway/desktop/xdg_shell.c:305), and a floating
/// client's own size change resizes its container (xdg_shell.c:319-331).
/// Oracle row: state `floating_output_scale_keeps_position`; differential
/// seed 10900.
#[test]
fn output_rescale_keeps_floating_position_and_client_size() {
    let (mut f, _) = ipc_fixture();
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
    for command in [
        "floating enable",
        "resize set 700 px 500 px",
        "move absolute position 290 px 87 px",
    ] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success);
    }
    f.double_roundtrip(client);
    f.client(client).window(&surface).ack_last_and_commit();
    f.double_roundtrip(client);
    let window = f.client(client).window(&surface);
    let (w, h) = window.configures_received.last().unwrap().1.size;
    window.set_size(w as u16, h as u16);
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    let rect = |f: &mut Fixture| {
        let tree = get_tree(f);
        let rect = &find_json_node(&tree, "floating_con", false).unwrap()["rect"];
        (
            rect["x"].as_i64().unwrap(),
            rect["y"].as_i64().unwrap(),
            rect["width"].as_i64().unwrap(),
        )
    };
    let (x, y, width) = rect(&mut f);
    assert_eq!((x, y), (290, 87));
    let _ = f.client(client).window(&surface).format_recent_configures();

    assert!(crate::command::execute(f.niri_state(), "output * scale 2")[0].success);
    f.double_roundtrip(client);
    assert_eq!(
        rect(&mut f),
        (290, 87, width),
        "a rescale must not move the floater"
    );

    // The client narrows itself for the new scale, as foot does.
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.set_size((w - 34) as u16, h as u16);
    window.commit();
    f.double_roundtrip(client);
    f.double_roundtrip(client);
    let window = f.client(client).window(&surface);
    let stale: Vec<_> = window
        .recent_configures()
        .filter(|configure| configure.size == (w, h))
        .cloned()
        .collect();
    assert!(
        stale.is_empty(),
        "configures since the rescale repeat the old size: {stale:?}"
    );
    assert_eq!(rect(&mut f), (290, 87, width - 34));
}

fn run_split_wrap_commands(f: &mut Fixture, client: super::client::ClientId, commands: &[&str]) {
    for command in commands {
        assert!(
            crate::command::execute(f.niri_state(), command)[0].success,
            "{command}"
        );
        f.double_roundtrip(client);
    }
}

/// Differential family floating-fullscreen-split-wrap (random-v2 seed 16849): `splith` on a
/// fullscreen floating view wraps it in a new floating split that takes over the fullscreen
/// mode (`container_split` and `container_replace`, sway/tree/container.c:1471-1501).
#[test]
fn split_wraps_a_fullscreen_floating_view_and_moves_the_mode() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    map_app(&mut f, client, "fixture-1");
    run_split_wrap_commands(
        &mut f,
        client,
        &["fullscreen toggle", "floating enable", "splith"],
    );

    let tree = tree_json(&mut f);
    let split = find_json_node(&tree, "floating_con", false).unwrap();
    assert_eq!(split["layout"], "splith", "{split}");
    assert_eq!(split["fullscreen_mode"], 1, "{split}");
    assert_eq!(split["border"], "none", "{split}");
    let view = &split["nodes"][0];
    assert_eq!(view["fullscreen_mode"], 0, "{view}");
    assert_eq!(view["focused"], true, "{view}");
}

/// The same split with a view mapped under the fullscreen one. Sway only re-arranges the
/// fullscreen container (sway/tree/arrange.c:310-316), so the tiled view keeps calloc's
/// `border none`, empty box and zero percent. Oracle row:
/// floating_fullscreen_split_wraps_over_tiled.
#[test]
fn split_wrap_of_a_fullscreen_floating_view_leaves_the_tiled_view_unarranged() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    map_app(&mut f, client, "fixture-1");
    run_split_wrap_commands(&mut f, client, &["fullscreen toggle"]);
    map_app(&mut f, client, "fixture-2");
    run_split_wrap_commands(&mut f, client, &["floating enable", "splith"]);

    let tree = tree_json(&mut f);
    let split = find_json_node(&tree, "floating_con", false).unwrap();
    assert_eq!(split["layout"], "splith", "{split}");
    assert_eq!(split["fullscreen_mode"], 1, "{split}");
    let tiled = find_json_node_with_app_id(&tree, "fixture-2").unwrap();
    assert_eq!(tiled["border"], "none", "{tiled}");
    assert_eq!(tiled["current_border_width"], 0, "{tiled}");
    assert_eq!(tiled["percent"], 0.0, "{tiled}");
    assert_eq!(tiled["rect"]["width"], 0, "{tiled}");
    assert_eq!(tiled["rect"]["height"], 0, "{tiled}");

    // Ending the fullscreen arranges the whole workspace again.
    run_split_wrap_commands(&mut f, client, &["focus parent", "fullscreen disable"]);
    let tree = tree_json(&mut f);
    let tiled = find_json_node_with_app_id(&tree, "fixture-2").unwrap();
    assert_eq!(tiled["percent"], 1.0, "{tiled}");
    assert_eq!(tiled["rect"]["width"], 1280, "{tiled}");
}

/// Random-v2 seed 16849: the tiled view mapped beside the view before it went fullscreen
/// keeps the half box it had. The split arranges only the fullscreen container
/// (sway/tree/arrange.c:310-316), so its percent stays 0.5.
#[test]
fn split_wrap_of_a_fullscreen_floating_view_keeps_the_tiled_sibling_box() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    map_app(&mut f, client, "fixture-1");
    map_app(&mut f, client, "fixture-2");
    run_split_wrap_commands(&mut f, client, &["fullscreen toggle", "floating enable"]);
    let tree = tree_json(&mut f);
    let before = find_json_node_with_app_id(&tree, "fixture-1")
        .unwrap()
        .clone();
    run_split_wrap_commands(&mut f, client, &["splith"]);

    let tree = tree_json(&mut f);
    let tiled = find_json_node_with_app_id(&tree, "fixture-1").unwrap();
    assert_eq!(before["percent"], 0.5, "{before}");
    assert_eq!(tiled["percent"], 0.5, "{tiled}");
    assert_eq!(tiled["rect"], before["rect"], "{tree}");

    run_split_wrap_commands(&mut f, client, &["focus parent", "fullscreen disable"]);
    let tree = tree_json(&mut f);
    let tiled = find_json_node_with_app_id(&tree, "fixture-1").unwrap();
    assert_eq!(tiled["percent"], 1.0, "{tiled}");
}

/// The two hidden kinds together, as random-v3 seeds 31204 and 31633 reach them: a view
/// mapped under the fullscreen keeps its empty box, and a tiled view keeps the box it had
/// when the fullscreen began, whether that was half the workspace or all of it.
#[test]
fn split_wrap_of_a_fullscreen_floating_view_keeps_mapped_and_tiled_boxes() {
    for (setup, split, percent) in [
        (&["fullscreen toggle"][..], "splith", 0.5),
        (&["floating enable", "fullscreen enable"][..], "splitt", 1.0),
    ] {
        let mut f = Fixture::new();
        f.add_output(1, (1270, 1408));
        let client = f.add_client();
        map_app(&mut f, client, "fixture-1");
        map_app(&mut f, client, "fixture-2");
        run_split_wrap_commands(&mut f, client, setup);
        map_app(&mut f, client, "fixture-3");
        if percent == 0.5 {
            run_split_wrap_commands(&mut f, client, &["floating enable"]);
        }
        run_split_wrap_commands(&mut f, client, &[split]);

        let tree = tree_json(&mut f);
        let tiled = find_json_node_with_app_id(&tree, "fixture-1").unwrap();
        assert_eq!(tiled["percent"], percent, "{setup:?} {tiled}");
        assert_eq!(tiled["border"], "normal", "{setup:?} {tiled}");
        let mapped = find_json_node_with_app_id(&tree, "fixture-3").unwrap();
        assert_eq!(mapped["percent"], 0.0, "{setup:?} {mapped}");
        assert_eq!(mapped["border"], "none", "{setup:?} {mapped}");
    }
}

/// Ending the floating fullscreen arranges the whole root (sway/commands/fullscreen.c:55), so
/// the view mapped under it gets its box, border and percent at once, and keeps them through
/// the next command. Oracle rows: rv2_unfs, rv3_unfs_focus.
#[test]
fn fullscreen_disable_of_a_floating_fullscreen_split_arranges_the_mapped_view() {
    let mut f = Fixture::new();
    f.add_output(1, (1270, 1408));
    let client = f.add_client();
    map_app(&mut f, client, "fixture-1");
    run_split_wrap_commands(&mut f, client, &["fullscreen toggle"]);
    map_app(&mut f, client, "fixture-2");
    run_split_wrap_commands(
        &mut f,
        client,
        &[
            "floating enable",
            "splith",
            "focus parent",
            "fullscreen disable",
        ],
    );

    let assert_arranged = |tree: &serde_json::Value| {
        let mapped = find_json_node_with_app_id(tree, "fixture-2").unwrap();
        assert_eq!(mapped["border"], "normal", "{mapped}");
        assert_ne!(mapped["current_border_width"], 0, "{mapped}");
        assert_eq!(mapped["percent"], 1.0, "{mapped}");
        assert_eq!(mapped["rect"]["width"], 1270, "{mapped}");
    };
    assert_arranged(&tree_json(&mut f));
    run_split_wrap_commands(&mut f, client, &["[app_id=fixture-2] focus"]);
    assert_arranged(&tree_json(&mut f));
}

/// `floating disable` moves the fullscreen split back into the tiling tree, still fullscreen,
/// and `arrange_workspace` reaches only it (sway/commands/floating.c:55,
/// sway/tree/arrange.c:310-316). The view mapped under it stays unarranged and the tiled
/// sibling keeps its half box. Oracle row: rv2_floatdis.
#[test]
fn floating_disable_of_a_fullscreen_split_leaves_the_mapped_view_unarranged() {
    let mut f = Fixture::new();
    f.add_output(1, (1270, 1408));
    let client = f.add_client();
    map_app(&mut f, client, "fixture-1");
    map_app(&mut f, client, "fixture-2");
    run_split_wrap_commands(&mut f, client, &["fullscreen toggle"]);
    map_app(&mut f, client, "fixture-3");
    run_split_wrap_commands(
        &mut f,
        client,
        &[
            "floating enable",
            "splith",
            "focus parent",
            "floating disable",
        ],
    );

    let tree = tree_json(&mut f);
    let mapped = find_json_node_with_app_id(&tree, "fixture-3").unwrap();
    assert_eq!(mapped["border"], "none", "{tree}");
    assert_eq!(mapped["current_border_width"], 0, "{mapped}");
    assert_eq!(mapped["percent"], 0.0, "{mapped}");
    assert_eq!(mapped["rect"]["width"], 0, "{mapped}");
    let tiled = find_json_node_with_app_id(&tree, "fixture-1").unwrap();
    assert_eq!(tiled["percent"], 0.5, "{tiled}");
    assert_eq!(tiled["border"], "normal", "{tiled}");

    // The tiled fullscreen split then ends like any other, arranging the whole workspace.
    // Oracle row: rv3_floatdis_unfs.
    run_split_wrap_commands(&mut f, client, &["fullscreen disable"]);
    let tree = tree_json(&mut f);
    let mapped = find_json_node_with_app_id(&tree, "fixture-3").unwrap();
    assert_eq!(mapped["border"], "normal", "{tree}");
    assert_ne!(mapped["current_border_width"], 0, "{mapped}");
    assert_ne!(mapped["percent"], 0.0, "{mapped}");
}

/// After the split, a command that ends or moves the floating fullscreen leaves the tiled
/// views arranged with their own border (sway/commands/move.c, sway/commands/fullscreen.c:55).
/// Oracle rows: rv2_movews, rv2_splitv_twice, rv3_scratch; rv2_kill covers `kill`, which the
/// harness client does not act on.
#[test]
fn ending_a_floating_fullscreen_split_arranges_the_mapped_view() {
    for exit in [
        &["focus parent", "move container to workspace 2"][..],
        &["focus parent", "fullscreen toggle"][..],
        &["focus parent", "move scratchpad"][..],
    ] {
        let mut f = Fixture::new();
        f.add_output(1, (1270, 1408));
        let client = f.add_client();
        map_app(&mut f, client, "fixture-1");
        map_app(&mut f, client, "fixture-2");
        run_split_wrap_commands(&mut f, client, &["fullscreen toggle"]);
        map_app(&mut f, client, "fixture-3");
        run_split_wrap_commands(&mut f, client, &["floating enable", "splith"]);
        run_split_wrap_commands(&mut f, client, exit);

        let tree = tree_json(&mut f);
        for app_id in ["fixture-1", "fixture-3"] {
            let tiled = find_json_node_with_app_id(&tree, app_id).unwrap();
            assert_eq!(tiled["border"], "normal", "{exit:?} {tiled}");
            assert_ne!(tiled["current_border_width"], 0, "{exit:?} {tiled}");
            assert_eq!(tiled["percent"], 0.5, "{exit:?} {tiled}");
        }
    }
}

/// Differential family v3-floating-split-rect-after-resize (random-v3 seeds 30850, 31123,
/// 31297, 32283; oracle row `floating_resize_ppt_split_keeps_rect`): `resize set` on a
/// floating view moves the container by half the growth (`con->pending.x -= grow_width / 2`,
/// sway/commands/resize.c:360-362, :381-383), so the box keeps its centre and a later split
/// wraps the moved box.
#[test]
fn floating_resize_set_keeps_the_centre_and_split_wraps_it() {
    let mut config = swayward_config::Config::parse_mem(
        r#"layout { default-border "normal" width=2; default-floating-border "normal" width=2; border { on; width 2; }; }"#,
    )
    .unwrap();
    config.animations.off = true;
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1270, 1408));
    let client = f.add_client();
    let surface = f.client(client).create_window().surface.clone();
    f.client(client).decorate_last_window(
        smithay::reexports::wayland_protocols::xdg::decoration::zv1::client::zxdg_toplevel_decoration_v1::Mode::ServerSide,
    );
    f.client(client).window(&surface).commit();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.set_size(696, 491);
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);
    f.double_roundtrip(client);
    let window = f.client(client).window(&surface);
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    let floating = |f: &mut Fixture| {
        f.niri_state().ipc_refresh_layout();
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
    };
    let before = floating(&mut f);
    let int = |node: &Value, key: &str| node["rect"][key].as_i64().unwrap();
    let (x, y, w, h) = (
        int(&before, "x"),
        int(&before, "y"),
        int(&before, "width"),
        int(&before, "height"),
    );
    assert_eq!(w, 700, "{before}");

    // 30 ppt of 1270 is 381, 40 ppt of 1408 is 563; the client has not committed yet.
    assert!(
        crate::command::execute(f.niri_state(), "resize set width 30 ppt height 40 ppt")[0].success
    );
    assert!(crate::command::execute(f.niri_state(), "splitv")[0].success);
    let expected = serde_json::json!({
        "x": x - (381 - w) / 2,
        "y": y - (563 - h) / 2,
        "width": 381,
        "height": 563,
    });
    let split = floating(&mut f);
    assert_eq!(split["layout"], "splitv", "{split}");
    assert_eq!(split["rect"], expected, "{before}\n{split}");
    let child = &split["nodes"][0];
    let titlebar = child["deco_rect"]["height"].as_i64().unwrap();
    assert_eq!(int(child, "x"), expected["x"].as_i64().unwrap(), "{child}");
    assert_eq!(
        int(child, "y"),
        expected["y"].as_i64().unwrap() + titlebar,
        "{child}"
    );
}

#[test]
fn floating_move_to_another_output_rehomes_like_sway() {
    let mut config = swayward_config::Config::default();
    config.animations.off = true;
    let mut f = Fixture::with_config(config);
    f.add_named_output_at("left-head".into(), (1280, 720), Some((0, 0)));
    f.add_named_output_at("right-head".into(), (1280, 720), Some((1280, 0)));
    f.niri_focus_output(2);
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);

    let home = |f: &mut Fixture| {
        f.niri_state().ipc_refresh_layout();
        let swayward = f.swayward();
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &Default::default(),
            &Default::default(),
        ))
        .unwrap();
        let outputs = tree["nodes"].as_array().unwrap();
        let (output, rect) = outputs
            .iter()
            .find_map(|output| {
                let workspace = output["nodes"].as_array()?.iter().find(|workspace| {
                    !workspace["floating_nodes"].as_array().unwrap().is_empty()
                })?;
                Some((
                    output["name"].as_str().unwrap().to_owned(),
                    workspace["floating_nodes"][0]["rect"].clone(),
                ))
            })
            .unwrap();
        (
            output,
            rect["x"].as_i64().unwrap(),
            rect["y"].as_i64().unwrap(),
        )
    };

    assert_eq!(home(&mut f).0, "right-head");
    assert!(
        crate::command::execute(f.niri_state(), "move absolute position 10 px 20 px")[0].success
    );
    let (output, x, _) = home(&mut f);
    assert_eq!((output.as_str(), x), ("left-head", 10));
    assert_eq!(
        f.swayward().layout.active_output().unwrap().name(),
        "left-head"
    );

    assert!(crate::command::execute(f.niri_state(), "move right 1500 px")[0].success);
    let (output, x, _) = home(&mut f);
    assert_eq!((output.as_str(), x), ("right-head", 1510));
    assert_eq!(
        f.swayward().layout.active_output().unwrap().name(),
        "right-head"
    );
}

#[test]
fn fullscreen_floater_move_absolute_position_crosses_output_like_sway() {
    let mut f = Fixture::new();
    f.add_named_output_at("left-head".into(), (1280, 720), Some((0, 0)));
    f.add_named_output_at("right-head".into(), (1280, 720), Some((1280, 0)));
    f.niri_focus_output(2);
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    assert!(crate::command::execute(f.niri_state(), "fullscreen toggle")[0].success);
    assert!(crate::command::execute(f.niri_state(), "floating toggle")[0].success);
    assert!(crate::command::execute(f.niri_state(), "move position 10 px 20 px")[0].success);
    assert_eq!(
        f.swayward().layout.active_output().unwrap().name(),
        "right-head"
    );
    assert!(
        crate::command::execute(f.niri_state(), "move absolute position 10 px 20 px")[0].success
    );
    assert_eq!(
        f.swayward().layout.active_output().unwrap().name(),
        "left-head"
    );
    let focused = f.swayward().layout.focus().unwrap().window.clone();
    assert!(f.swayward().layout.fullscreen_mode(&focused).is_some());
    assert!(
        !crate::command::execute(f.niri_state(), "move absolute position 10 ppt 20 px")[0].success
    );
}
