#[test]
fn live_ipc_rectangle_roles_match_sway_relationships() {
    let expected = nested_fixture_tree();
    let actual = nested_live_tree();
    assert_rectangle_roles_match_fixture(&expected, &actual, "$tree");
}

#[test]
fn nested_tiling_rectangles_match_sway_roles() {
    let tree = nested_live_tree();
    let workspace = &tree["nodes"][1]["nodes"][0];
    let top = &workspace["nodes"][0];
    let nested = &workspace["nodes"][1];
    let bottom_left = &nested["nodes"][0];
    let bottom_right = &nested["nodes"][1];

    assert_eq!(
        nested["rect"]["x"],
        bottom_left["rect"]["x"].as_i64().unwrap()
            - bottom_left["deco_rect"]["x"].as_i64().unwrap()
    );
    assert!(nested["rect"]["x"].as_i64().unwrap() > top["rect"]["x"].as_i64().unwrap());
    assert_eq!(
        nested["rect"]["y"],
        bottom_left["rect"]["y"].as_i64().unwrap()
            - bottom_left["deco_rect"]["height"].as_i64().unwrap()
    );
    assert_eq!(nested["rect"]["width"], bottom_left["rect"]["width"]);
    assert_eq!(
        nested["rect"]["height"],
        bottom_right["rect"]["y"].as_i64().unwrap()
            + bottom_right["rect"]["height"].as_i64().unwrap()
            - nested["rect"]["y"].as_i64().unwrap()
    );
    for window in [top, bottom_left, bottom_right] {
        assert!(window["deco_rect"]["height"].as_i64().unwrap() > 0);
        assert_eq!(window["current_border_width"], 4);
        assert_eq!(window["window_rect"]["x"], 4);
        assert_eq!(window["window_rect"]["y"], 0);
        assert_eq!(
            window["window_rect"]["width"].as_i64().unwrap(),
            window["rect"]["width"].as_i64().unwrap() - 8
        );
        assert_eq!(
            window["window_rect"]["height"].as_i64().unwrap(),
            window["rect"]["height"].as_i64().unwrap() - 4
        );
    }
}

#[test]
fn border_none_zeroes_deco_and_uses_the_whole_rect_for_window() {
    let mut f = Fixture::new();
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
    assert!(crate::command::execute(f.niri_state(), "border none")[0].success);

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    let window = &tree["nodes"][1]["nodes"][0]["nodes"][0];
    assert_eq!(
        window["deco_rect"],
        serde_json::json!({"x": 0, "y": 0, "width": 0, "height": 0})
    );
    assert_eq!(window["window_rect"]["x"], 0);
    assert_eq!(window["window_rect"]["y"], 0);
    assert_eq!(window["window_rect"]["width"], window["rect"]["width"]);
    assert_eq!(window["window_rect"]["height"], window["rect"]["height"]);
}

#[test]
fn live_ipc_percent_matches_sway_parent_shares() {
    let expected = nested_fixture_tree();
    let representation = nested_representation_live_tree();
    assert_eq!(
        expected["nodes"][1]["nodes"][0]["representation"],
        representation["nodes"][1]["nodes"][0]["representation"],
        "workspace representation at $tree.nodes[1].nodes[0]"
    );
    let actual = nested_live_tree();
    assert_percent_value_matches_fixture(
        &expected["nodes"][1]["nodes"][0]["nodes"][1],
        &actual["nodes"][1]["nodes"][0]["nodes"][1],
        "$tree.nodes[1].nodes[0].nodes[1]",
    );
    assert_percent_matches_fixture(&expected, &actual, "$tree");
}

#[test]
fn root_focus_lists_outputs_once_in_global_mru_order() {
    let mut f = Fixture::new();
    for output in 1..=3 {
        f.add_output(output, (1280, 720));
    }
    let client = f.add_client();
    for output in [1, 2, 3] {
        f.niri_focus_output(output);
        let window = f.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    }

    let root_focus = |f: &mut Fixture| {
        let swayward = f.swayward();
        describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &Default::default(),
            &Default::default(),
        )
        .focus
    };
    let output_ids = |f: &mut Fixture| {
        let swayward = f.swayward();
        describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &Default::default(),
            &Default::default(),
        )
        .nodes
        .into_iter()
        .filter(|node| node.name.as_deref() != Some("__i3"))
        .map(|node| node.id)
        .collect::<Vec<_>>()
    };

    let ids = output_ids(&mut f);
    assert_eq!(root_focus(&mut f), [ids[2], ids[1], ids[0]]);
    f.niri_focus_output(1);
    assert_eq!(root_focus(&mut f), [ids[0], ids[2], ids[1]]);
    let focus = root_focus(&mut f);
    assert_eq!(focus.len(), ids.len());
    assert_eq!(
        focus.iter().collect::<std::collections::HashSet<_>>().len(),
        ids.len()
    );
}

#[test]
fn criteria_focus_output_ignores_hidden_scratchpad_match_and_uses_seat_output() {
    let mut f = Fixture::new();
    f.add_named_output_at("left-head".into(), (800, 600), Some((0, 0)));
    f.add_named_output_at("right-head".into(), (800, 600), Some((800, 0)));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    let hidden = crate::ipc::tree::window_id(f.swayward().layout.focus().unwrap().id());
    assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);

    let command = format!(r#"[con_id="{hidden}"] focus output right-head"#);
    assert!(crate::command::execute(f.niri_state(), &command)[0].success);
    assert_eq!(
        f.swayward().layout.active_output().unwrap().name(),
        "right-head"
    );

    let command = format!(r#"[con_id="{hidden}"] focus output left"#);
    assert!(crate::command::execute(f.niri_state(), &command)[0].success);
    assert_eq!(
        f.swayward().layout.active_output().unwrap().name(),
        "left-head"
    );
    assert!(crate::command::execute(f.niri_state(), &command)[0].success);
    assert_eq!(
        f.swayward().layout.active_output().unwrap().name(),
        "right-head"
    );
    assert_eq!(f.swayward().layout.scratchpad_windows().count(), 1);
    assert!(crate::command::execute(f.niri_state(), "nop")[0].success);
}

#[test]
fn criteria_focus_output_succeeds_without_an_output_and_keeps_scratchpad_hidden() {
    let mut f = Fixture::new();
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
    let hidden = crate::ipc::tree::window_id(f.swayward().layout.focus().unwrap().id());
    assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);
    let output = f.swayward().layout.active_output().unwrap().clone();

    let command = format!(r#"[con_id="{hidden}"] focus output left"#);
    assert!(crate::command::execute(f.niri_state(), &command)[0].success);
    assert_eq!(f.swayward().layout.active_output(), Some(&output));
    assert_eq!(f.swayward().layout.scratchpad_windows().count(), 1);
    assert!(crate::command::execute(f.niri_state(), "nop")[0].success);
}

#[test]
fn focus_output_prefers_a_name_over_a_direction_and_resolves_directions() {
    let mut f = Fixture::new();
    f.add_named_output_at("origin".into(), (1280, 720), Some((0, 0)));
    f.add_named_output_at("left".into(), (1280, 720), Some((1280, 0)));

    assert!(crate::command::execute(f.niri_state(), "focus output left")[0].success);
    assert_eq!(f.swayward().layout.active_output().unwrap().name(), "left");
    assert!(crate::command::execute(f.niri_state(), "focus output origin")[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus output right")[0].success);
    assert_eq!(f.swayward().layout.active_output().unwrap().name(), "left");
}

#[test]
fn focus_output_uses_nearest_geometry_then_wraps_to_farthest_opposite() {
    let mut f = Fixture::new();
    f.add_named_output_at("west".into(), (800, 600), Some((0, 0)));
    f.add_named_output_at("center".into(), (800, 600), Some((800, 0)));
    f.add_named_output_at("east".into(), (800, 600), Some((1600, 0)));
    f.add_named_output_at("far-east".into(), (800, 600), Some((2400, 0)));
    let client = f.add_client();

    let mut ids = std::collections::HashMap::new();
    for output in ["west", "east", "far-east", "center"] {
        assert!(
            crate::command::execute(f.niri_state(), &format!("focus output {output}"))[0].success
        );
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(format!("{output}-window"));
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        f.niri_state().update_keyboard_focus();
        ids.insert(output, f.swayward().layout.focus().unwrap().id());
    }

    assert!(crate::command::execute(f.niri_state(), "focus output east")[0].success);
    assert_eq!(f.swayward().layout.focus().unwrap().id(), ids["east"]);
    let latest = f.client(client).create_window();
    latest.xdg_toplevel.set_app_id("east-latest".into());
    latest.commit();
    let latest_surface = latest.surface.clone();
    f.roundtrip(client);
    let latest = f.client(client).window(&latest_surface);
    latest.attach_new_buffer();
    latest.ack_last_and_commit();
    f.double_roundtrip(client);
    f.niri_state().update_keyboard_focus();
    ids.insert("east", f.swayward().layout.focus().unwrap().id());
    assert!(crate::command::execute(f.niri_state(), "focus output center")[0].success);

    assert!(crate::command::execute(f.niri_state(), "focus output right")[0].success);
    assert_eq!(f.swayward().layout.active_output().unwrap().name(), "east");
    assert_eq!(f.swayward().layout.focus().unwrap().id(), ids["east"]);

    assert!(crate::command::execute(f.niri_state(), "focus output far-east")[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus output right")[0].success);
    assert_eq!(f.swayward().layout.active_output().unwrap().name(), "west");
    assert_eq!(f.swayward().layout.focus().unwrap().id(), ids["west"]);
}

#[test]
fn directional_focus_prefers_an_adjacent_output_over_local_wrapping() {
    for (target_position, layout, command, wrapping, expected_output) in [
        (
            (800, 0),
            "layout splith",
            "focus right",
            swayward_config::FocusWrapping::Yes,
            "target",
        ),
        (
            (0, 600),
            "layout stacking",
            "focus down",
            swayward_config::FocusWrapping::Yes,
            "target",
        ),
        (
            (800, 0),
            "layout splith",
            "focus right",
            swayward_config::FocusWrapping::Force,
            "source",
        ),
        (
            (800, 0),
            "layout splith",
            "focus right",
            swayward_config::FocusWrapping::No,
            "target",
        ),
        (
            (800, 0),
            "layout splith",
            "focus right",
            swayward_config::FocusWrapping::Workspace,
            "source",
        ),
    ] {
        let mut config = swayward_config::Config::default();
        config.layout.focus_wrapping = wrapping;
        let mut f = Fixture::with_config(config);
        f.add_named_output_at("source".into(), (800, 600), Some((0, 0)));
        f.add_named_output_at("target".into(), (800, 600), Some(target_position));
        let client = f.add_client();

        for output in ["target", "source", "source"] {
            assert!(
                crate::command::execute(f.niri_state(), &format!("focus output {output}"))[0]
                    .success
            );
            let window = f.client(client).create_window();
            window.commit();
            let surface = window.surface.clone();
            f.roundtrip(client);
            let window = f.client(client).window(&surface);
            window.attach_new_buffer();
            window.ack_last_and_commit();
            f.double_roundtrip(client);
        }
        assert!(crate::command::execute(f.niri_state(), layout)[0].success);

        assert!(crate::command::execute(f.niri_state(), command)[0].success);
        assert_eq!(
            f.swayward().layout.active_output().unwrap().name(),
            expected_output
        );
    }
}

#[test]
fn no_wrapping_crosses_an_adjacent_output_but_does_not_wrap_outputs() {
    let mut config = swayward_config::Config::default();
    config.layout.focus_wrapping = swayward_config::FocusWrapping::No;
    let mut f = Fixture::with_config(config);
    f.add_named_output_at("left".into(), (800, 600), Some((0, 0)));
    f.add_named_output_at("right".into(), (800, 600), Some((800, 0)));

    assert!(crate::command::execute(f.niri_state(), "focus output left")[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus right")[0].success);
    assert_eq!(f.swayward().layout.active_output().unwrap().name(), "right");
    assert!(crate::command::execute(f.niri_state(), "focus right")[0].success);
    assert_eq!(f.swayward().layout.active_output().unwrap().name(), "right");
}

#[test]
fn workspace_wrapping_uses_local_wrap_instead_of_an_adjacent_output() {
    let mut config = swayward_config::Config::default();
    config.layout.focus_wrapping = swayward_config::FocusWrapping::Workspace;
    let mut f = Fixture::with_config(config);
    f.add_named_output_at("source".into(), (800, 600), Some((0, 0)));
    f.add_named_output_at("target".into(), (800, 600), Some((800, 0)));
    let client = f.add_client();
    let mut ids = Vec::new();

    assert!(crate::command::execute(f.niri_state(), "focus output source")[0].success);
    for _ in 0..2 {
        let window = f.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        ids.push(f.swayward().layout.focus().unwrap().id());
    }

    assert!(crate::command::execute(f.niri_state(), "focus right")[0].success);
    assert_eq!(
        f.swayward().layout.active_output().unwrap().name(),
        "source"
    );
    assert_eq!(f.swayward().layout.focus().unwrap().id(), ids[0]);
}

#[test]
fn workspace_wrapping_allows_output_focus_from_a_focused_workspace_node() {
    let mut config = swayward_config::Config::default();
    config.layout.focus_wrapping = swayward_config::FocusWrapping::Workspace;
    let mut f = Fixture::with_config(config);
    f.add_named_output_at("source".into(), (800, 600), Some((0, 0)));
    f.add_named_output_at("target".into(), (800, 600), Some((800, 0)));
    let client = f.add_client();

    assert!(crate::command::execute(f.niri_state(), "focus output source")[0].success);
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
    assert!(f
        .swayward()
        .layout
        .active_workspace()
        .unwrap()
        .is_workspace_focused());

    assert!(crate::command::execute(f.niri_state(), "focus right")[0].success);
    assert_eq!(
        f.swayward().layout.active_output().unwrap().name(),
        "target"
    );
}

#[test]
fn global_fullscreen_blocks_directional_output_focus_but_workspace_fullscreen_does_not() {
    for (global, direction, source, expected) in [
        (false, "right", "left", "right"),
        (false, "left", "right", "left"),
        (true, "right", "left", "left"),
        (true, "left", "right", "right"),
    ] {
        let mut config = swayward_config::Config::default();
        config.layout.focus_wrapping = swayward_config::FocusWrapping::Workspace;
        let mut f = Fixture::with_config(config);
        f.add_named_output_at("left".into(), (800, 600), Some((0, 0)));
        f.add_named_output_at("right".into(), (800, 600), Some((800, 0)));
        assert!(
            crate::command::execute(f.niri_state(), &format!("focus output {source}"))[0].success
        );
        let client = f.add_client();
        let window = f.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        let mode = if global { " global" } else { "" };
        assert!(
            crate::command::execute(f.niri_state(), &format!("fullscreen enable{mode}"))[0].success
        );

        assert!(crate::command::execute(f.niri_state(), &format!("focus {direction}"))[0].success);
        assert_eq!(
            f.swayward().layout.active_output().unwrap().name(),
            expected,
            "global={global} direction={direction}"
        );
    }
}

#[test]
fn focus_output_reports_sway_errors() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let output = f.swayward().layout.active_output().unwrap().clone();
    assert_eq!(
        crate::command::execute(f.niri_state(), "focus output missing"),
        [swayward_ipc::CommandOutcome {
            success: false,
            error: Some("There is no output with that name.".into()),
            parse_error: Some(true),
        }]
    );
    assert_eq!(f.swayward().layout.active_output(), Some(&output));

    let mut f = Fixture::new();
    assert_eq!(
        crate::command::execute(f.niri_state(), "focus output right"),
        [swayward_ipc::CommandOutcome {
            success: false,
            error: Some("No focused workspace to base directions off of.".into()),
            parse_error: Some(false),
        }]
    );
}

#[test]
fn move_output_reports_the_missing_target() {
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

    assert_eq!(
        crate::command::execute(f.niri_state(), "move output missing")[0]
            .error
            .as_deref(),
        Some("Can't find output with name/direction 'missing'")
    );
}

#[test]
fn move_output_accepts_direction_name_and_workspace_forms() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    f.add_output(2, (1280, 720));
    let outputs = [f.niri_output(1).name(), f.niri_output(2).name()];
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    let focused = f.swayward().layout.focus().unwrap().id();

    assert!(crate::command::execute(f.niri_state(), "move output right")[0].success);
    assert_eq!(
        f.swayward()
            .layout
            .windows()
            .find(|(_, mapped)| mapped.id() == focused)
            .unwrap()
            .0
            .unwrap()
            .output_name(),
        &outputs[1]
    );
    // Sway refocuses the emptied source workspace (sway/commands/move.c:598-607), where
    // a container move fails "Can't move an empty workspace"; follow the window first.
    assert!(crate::command::execute(f.niri_state(), "focus output right")[0].success);
    assert!(
        crate::command::execute(
            f.niri_state(),
            &format!("move container to output {}", outputs[0])
        )[0]
        .success
    );
    assert!(crate::command::execute(f.niri_state(), "move workspace output right")[0].success);
}

// sway/tree/container.c:977-980: returning a container to tiling removes it
// from the scratchpad, so a later `scratchpad show` finds nothing to toggle.
#[test]
fn unfloating_a_shown_scratchpad_window_removes_it_from_the_scratchpad() {
    for unfloat in ["floating disable", "floating toggle"] {
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
        let id = f.swayward().layout.focus().unwrap().window.clone();

        assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);
        assert!(crate::command::execute(f.niri_state(), "scratchpad show")[0].success);
        assert!(f.swayward().layout.is_scratchpad_window(&id));

        let outcome = crate::command::execute(f.niri_state(), unfloat);
        assert!(outcome[0].success, "{unfloat}: {outcome:?}");
        assert!(!f.swayward().layout.is_scratchpad_window(&id), "{unfloat}");
        assert!(
            !f.swayward().layout.focus().unwrap().is_floating(),
            "{unfloat}"
        );
        assert_eq!(
            crate::command::execute(f.niri_state(), "scratchpad show")[0]
                .error
                .as_deref(),
            Some("Scratchpad is empty"),
            "{unfloat}"
        );
        assert_eq!(
            f.swayward().layout.focus().map(|w| w.window.clone()),
            Some(id)
        );
    }
}

#[test]
fn an_unfocused_floating_group_does_not_report_itself_focused() {
    // Oracle random seed 133 step 19: a tabbed group is floated and focused, then a tiled
    // view maps and takes focus. Sway reports only the seat focus as focused, so the
    // floating group must clear its flag; its own tree still remembers its focused child.
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    crate::tests::windows::map_window(
        &mut f,
        client,
        crate::tests::windows::WindowSpec {
            app_id: Some("group-tab"),
            ..Default::default()
        },
    );
    for command in ["layout tabbed", "focus parent", "floating toggle"] {
        assert!(
            crate::command::execute(f.niri_state(), command)[0].success,
            "{command}"
        );
    }
    crate::tests::windows::map_window(
        &mut f,
        client,
        crate::tests::windows::WindowSpec {
            app_id: Some("tiled-new"),
            ..Default::default()
        },
    );

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let mut focused = Vec::new();
    fn collect(node: &serde_json::Value, focused: &mut Vec<String>) {
        if node["focused"] == true {
            focused.push(format!(
                "{}:{}",
                node["type"].as_str().unwrap_or_default(),
                node["app_id"].as_str().unwrap_or("-")
            ));
        }
        for child in node["nodes"]
            .as_array()
            .into_iter()
            .flatten()
            .chain(node["floating_nodes"].as_array().into_iter().flatten())
        {
            collect(child, focused);
        }
    }
    collect(&tree, &mut focused);
    assert_eq!(focused, ["con:tiled-new"]);
}

/// Differential seeds 1117, 1830, 4372, 4674 (diff-fam-fullscreen-mode-transfer).
/// `focus floating|tiling` never leaves fullscreen: `seat_set_focus` refuses a
/// view a fullscreen container hides (sway/input/seat.c:1148-1151), and a
/// fullscreen floating view is the floating layer's target
/// (sway/commands/focus.c:262-307). A fullscreen floating view keeps its slot
/// in `floating_nodes` (sway/tree/container.c:1186-1218).
#[test]
fn focus_layer_commands_keep_fullscreen() {
    /// Type, app_id, fullscreen_mode, focused, visible.
    type Row = (String, String, i64, bool, bool);
    fn run(seq: &[&str]) -> (Vec<swayward_ipc::CommandOutcome>, Vec<Row>) {
        let (mut f, _) = ipc_fixture();
        f.add_output(1, (1280, 720));
        let client = f.add_client();
        let mut last = Vec::new();
        for step in seq {
            if let Some(app_id) = step.strip_prefix("map ") {
                crate::tests::windows::map_window(
                    &mut f,
                    client,
                    crate::tests::windows::WindowSpec {
                        app_id: Some(app_id),
                        ..Default::default()
                    },
                );
            } else {
                last = crate::command::execute(f.niri_state(), step);
                f.double_roundtrip(client);
            }
        }
        let tree = get_tree(&mut f);
        let workspace = &tree["nodes"][1]["nodes"][0];
        let nodes = workspace["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .chain(workspace["floating_nodes"].as_array().unwrap())
            .map(|node| {
                (
                    node["type"].as_str().unwrap().to_owned(),
                    node["app_id"].as_str().unwrap().to_owned(),
                    node["fullscreen_mode"].as_i64().unwrap(),
                    node["focused"].as_bool().unwrap(),
                    node["visible"].as_bool().unwrap(),
                )
            })
            .collect();
        (last, nodes)
    }
    let node = |kind: &str, app: &str, fs, focused, visible| {
        (kind.to_owned(), app.to_owned(), fs, focused, visible)
    };
    let rule = r#"for_window [app_id="float"] floating enable"#;

    // 1117: no tiling view outside the fullscreen one; it stays fullscreen.
    let (reply, nodes) = run(&["map tiled", "fullscreen enable", "focus tiling"]);
    assert!(reply[0].success, "{reply:?}");
    assert_eq!(nodes, [node("con", "tiled", 1, true, true)]);

    // 1830: the floater is hidden by the fullscreen view, so focus stays.
    let (reply, nodes) = run(&[
        "map tiled",
        "fullscreen toggle",
        rule,
        "map float",
        "focus floating",
    ]);
    assert!(reply[0].success, "{reply:?}");
    assert_eq!(
        nodes,
        [
            node("con", "tiled", 1, true, true),
            node("floating_con", "float", 0, false, false),
        ]
    );

    // 4674: a fullscreen floating view is the floating layer's target.
    let (reply, nodes) = run(&[
        "map float",
        rule,
        "fullscreen enable",
        "mark oracle",
        "focus floating",
    ]);
    assert_eq!(
        reply,
        [swayward_ipc::CommandOutcome {
            success: true,
            error: None,
            parse_error: None
        }]
    );
    assert_eq!(nodes, [node("floating_con", "float", 1, true, true)]);

    // 4372: a floater mapped above a fullscreen floating view is listed
    // after it, and stays hidden and unfocused.
    let (_, nodes) = run(&[
        "map first",
        "floating toggle",
        rule,
        "fullscreen enable",
        "map float",
    ]);
    assert_eq!(
        nodes,
        [
            node("floating_con", "first", 1, true, true),
            node("floating_con", "float", 0, false, false),
        ]
    );

    // 1830 and 4372, step 5: views mapped under a fullscreen view, floating
    // or tiled, were never focused, so they join the workspace focus list in
    // creation order (sway/input/seat.c:327-349).
    for prefix in [
        &["map tiled", "fullscreen toggle"][..],
        &["map tiled", "floating toggle", "fullscreen enable"],
    ] {
        let (mut f, _) = ipc_fixture();
        f.add_output(1, (1280, 720));
        let client = f.add_client();
        let mut steps = prefix.to_vec();
        steps.extend([
            rule,
            r#"for_window [app_id="later"] floating enable"#,
            "map float",
            "map later",
            "map tail",
        ]);
        for step in steps {
            if let Some(app_id) = step.strip_prefix("map ") {
                crate::tests::windows::map_window(
                    &mut f,
                    client,
                    crate::tests::windows::WindowSpec {
                        app_id: Some(app_id),
                        ..Default::default()
                    },
                );
            } else {
                assert!(crate::command::execute(f.niri_state(), step)[0].success);
                f.double_roundtrip(client);
            }
        }
        let tree = get_tree(&mut f);
        let workspace = &tree["nodes"][1]["nodes"][0];
        let id = |app_id| find_json_node_with_app_id(workspace, app_id).unwrap()["id"].clone();
        assert_eq!(
            workspace["focus"],
            serde_json::json!([id("tiled"), id("float"), id("later"), id("tail")]),
            "{prefix:?}"
        );
    }
}

/// diff-fam-v3-focus-after-kill-in-toggled-split, the family's filed minimal
/// (oracle state row floated_by_rule_keeps_wrapper_unfocused). `layout toggle all`
/// on a lone view wraps it in a new split. A view a `for_window` rule floats
/// while it maps was never the seat focus (criteria run before `view_map`
/// focuses it, sway/tree/view.c:943-956), so `container_set_floating` does not
/// raise the old parent (sway/tree/container.c:946-949,969-973). Hiding the
/// floater then refocuses `seat_get_focus_inactive(ws)`
/// (sway/tree/root.c:227-229): the tiled view, not the wrapper.
#[test]
fn hiding_a_floater_refocuses_the_view_inside_a_fresh_wrapper() {
    let (mut f, _) = ipc_fixture();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    let map = |f: &mut Fixture, app_id| {
        crate::tests::windows::map_window(
            f,
            client,
            crate::tests::windows::WindowSpec {
                app_id: Some(app_id),
                ..Default::default()
            },
        );
    };
    map(&mut f, "tiled");
    for command in [
        "layout toggle all",
        r#"for_window [app_id="float"] floating enable"#,
    ] {
        assert!(
            crate::command::execute(f.niri_state(), command)[0].success,
            "{command}"
        );
    }
    map(&mut f, "float");
    assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);
    f.double_roundtrip(client);

    let tree = get_tree(&mut f);
    let wrapper = &tree["nodes"][1]["nodes"][0]["nodes"][0];
    assert_eq!(wrapper["focused"], false, "{wrapper:#}");
    assert_eq!(wrapper["nodes"][0]["app_id"], "tiled");
    assert_eq!(wrapper["nodes"][0]["focused"], true, "{wrapper:#}");
}

/// Seat focus after a view leaves, in the first-divergence shapes of
/// diff-fam-v3-focus-after-kill-in-toggled-split (seeds 30969 31054 31514
/// 31859 32235). Each case lists the commands and the tree path of the
/// node sway 1.12 reports focused afterwards.
/// - 30969: closing the focused view focuses the most recent view under its parent before any
///   elsewhere (`handle_seat_node_destroy`, sway/input/seat.c:273-286).
/// - 31054: floating a focused container raises its old parent (`container_set_floating`,
///   sway/tree/container.c:969-973), so hiding the floater refocuses that split
///   (sway/tree/root.c:227-229).
/// - 31514, 31859: closing the focused floater focuses the workspace's most recent view, never a
///   container (sway/input/seat.c:273-286).
/// - 31054 later: a view mapped while a floating view has focus is tiled beside the most recent
///   view under the focus-inactive split (sway/tree/view.c:851-866).
/// - 32235 later: the new view mapped beside the focused split, so hiding it focuses
///   `seat_get_focus_inactive(ws)`, which is that split (sway/tree/view.c:849-882,
///   sway/tree/root.c:128-140).
/// - 32235: hiding a tiled view reaps its emptied parent first, which raises the view below it, so
///   the view is refocused, not the grandparent (sway/tree/root.c:114-140,
///   sway/input/seat.c:273-323).
#[test]
fn focus_after_a_view_leaves_matches_sway() {
    let cases: &[(&str, &[&str], &[usize])] = &[
        (
            "30969",
            &[
                "map 6",
                r#"for_window [app_id="7"] split v"#,
                "map 7",
                r#"no_focus [app_id="8"]"#,
                "map 8",
                "kill",
            ],
            &[1, 0],
        ),
        (
            "31054",
            &[
                "map 1",
                "map 2",
                "layout toggle split",
                "split toggle",
                "focus parent; floating toggle",
                "move scratchpad",
            ],
            &[0],
        ),
        (
            "31514",
            &[
                "map 2",
                "layout toggle splitv tabbed",
                "map 7",
                "floating enable",
                "kill",
            ],
            &[0, 0],
        ),
        (
            "31859",
            &[
                "map 1",
                "map 2",
                "splitv",
                "map 5",
                "floating enable",
                "kill",
            ],
            &[1, 0],
        ),
        (
            "32235",
            &[
                "map 4",
                "focus parent; split v",
                "map 5",
                "layout toggle all",
                "focus prev",
                "move scratchpad",
            ],
            &[0, 0],
        ),
        (
            "31054 later",
            &[
                "map 1",
                "map 2",
                "layout toggle split",
                "split toggle",
                "focus parent; floating toggle",
                "map 7",
            ],
            &[0, 1],
        ),
        (
            "32235 later",
            &["map 4", "focus parent; split v", "map 5", "move scratchpad"],
            &[0],
        ),
    ];
    for (seed, steps, focused_path) in cases {
        let (mut f, _) = ipc_fixture();
        f.add_output(1, (1280, 720));
        let client = f.add_client();
        for step in *steps {
            if let Some(app_id) = step.strip_prefix("map ") {
                crate::tests::windows::map_window(
                    &mut f,
                    client,
                    crate::tests::windows::WindowSpec {
                        app_id: Some(app_id),
                        ..Default::default()
                    },
                );
                continue;
            }
            for outcome in crate::command::execute(f.niri_state(), step) {
                assert!(outcome.success, "{seed} {step}: {outcome:?}");
            }
            f.double_roundtrip(client);
            let closed = f
                .client(client)
                .state
                .windows
                .iter()
                .filter(|window| window.close_requested)
                .map(|window| window.surface.clone())
                .collect::<Vec<_>>();
            for surface in closed {
                let window = f.client(client).window(&surface);
                window.close_requested = false;
                window.attach_null();
                window.commit();
            }
            f.double_roundtrip(client);
        }
        let tree = get_tree(&mut f);
        let workspace = &tree["nodes"][1]["nodes"][0];
        let mut node = workspace;
        for index in *focused_path {
            node = &node["nodes"][*index];
        }
        assert_eq!(node["focused"], true, "{seed}: {workspace:#}");
    }
}
