#[test]
fn initial_workspace_keeps_pre_mode_orientation_and_later_workspace_uses_configured_mode() {
    let config = swayward_config::Config::parse_mem(
        r#"output "headless-1" { mode custom=true "1270x1408@60"; }"#,
    )
    .unwrap();
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1280, 720));
    let client = f.add_client();

    assert!(crate::command::execute(f.niri_state(), "workspace 1")[0].success);
    for (workspace, app_id) in [(None, "initial"), (Some("2"), "later")] {
        if let Some(workspace) = workspace {
            assert!(
                crate::command::execute(f.niri_state(), &format!("workspace {workspace}"))[0]
                    .success
            );
        }
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(app_id.into());
        let surface = window.surface.clone();
        window.commit();
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
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    assert_eq!(
        find_json_parent_of_app_id(&tree, "initial").unwrap()["layout"],
        "splith"
    );
    assert_eq!(
        find_json_parent_of_app_id(&tree, "later").unwrap()["layout"],
        "splitv"
    );
}

#[test]
fn emptied_workspace_is_recreated_with_default_layout() {
    let mut config = swayward_config::Config::default();
    config.animations.off = true;
    let (mut f, socket) = ipc_fixture_with_config(config);
    f.add_output(1, (1270, 1408));
    let client = f.add_client();

    assert!(crate::command::execute(f.niri_state(), "layout stacking")[0].success);
    let mut surfaces = Vec::new();
    for app_id in ["fixture-1", "fixture-2"] {
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(app_id.into());
        let surface = window.surface.clone();
        window.commit();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        surfaces.push(surface);
    }
    for surface in &surfaces {
        let window = f.client(client).window(surface);
        window.attach_null();
        window.commit();
        f.double_roundtrip(client);
    }

    assert!(crate::command::execute(f.niri_state(), "workspace __fixture_reset")[0].success);
    assert!(crate::command::execute(f.niri_state(), "workspace 1")[0].success);
    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("recreated".into());
    let surface = window.surface.clone();
    window.commit();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    let mut stream = UnixStream::connect(socket).unwrap();
    let tree = query_ipc(&mut f, &mut stream, MessageType::GetTree);
    let workspace = find_json_parent_of_app_id(&tree, "recreated").unwrap();
    assert_eq!(workspace["type"], "workspace");
    assert_eq!(workspace["name"], "1");
    assert_eq!(workspace["layout"], "splitv");
}

#[test]
fn configured_workspace_layout_uses_sways_layout_field_without_an_i3_alias() {
    let mut config = swayward_config::Config::default();
    config.layout.workspace_layout = swayward_config::WorkspaceLayout::Tabbed;
    let mut f = Fixture::with_config(config);
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

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    let workspace = &tree["nodes"][1]["nodes"][0];
    assert_eq!(workspace["layout"], "splith");
    assert_eq!(workspace["nodes"][0]["layout"], "tabbed");
    assert!(workspace.get("workspace_layout").is_none());
}

#[test]
fn layout_on_a_focused_nested_split_does_not_promote_to_the_workspace_root() {
    for (outer, expected_outer, inner, expected_inner) in [
        ("stacking", "stacked", "tabbed", "tabbed"),
        ("tabbed", "tabbed", "stacking", "stacked"),
    ] {
        let (mut f, socket) = ipc_fixture();
        f.add_output(1, (1920, 1080));
        let client = f.add_client();

        assert!(crate::command::execute(f.niri_state(), "splith")[0].success);
        for app_id in ["fixture-1", "fixture-2"] {
            let window = f.client(client).create_window();
            window.xdg_toplevel.set_app_id(app_id.into());
            let surface = window.surface.clone();
            window.commit();
            f.roundtrip(client);
            let window = f.client(client).window(&surface);
            window.attach_new_buffer();
            window.ack_last_and_commit();
            f.double_roundtrip(client);
        }
        assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
        assert!(crate::command::execute(f.niri_state(), &format!("layout {outer}"))[0].success);
        assert!(
            crate::command::execute(f.niri_state(), r#"[app_id="^fixture-1$"] focus"#)[0].success
        );
        assert!(crate::command::execute(f.niri_state(), "splith")[0].success);
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id("fixture-3".into());
        let surface = window.surface.clone();
        window.commit();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
        assert!(crate::command::execute(f.niri_state(), &format!("layout {inner}"))[0].success);
        f.niri_state().ipc_refresh_layout();

        let mut stream = UnixStream::connect(socket).unwrap();
        let tree = query_ipc(&mut f, &mut stream, MessageType::GetTree);
        let workspace = &tree["nodes"][1]["nodes"][0];
        assert_eq!(workspace["layout"], expected_outer);
        assert_eq!(workspace["nodes"].as_array().unwrap().len(), 1);
        assert_eq!(workspace["nodes"][0]["layout"], expected_inner);
        assert_eq!(workspace["nodes"][0]["nodes"].as_array().unwrap().len(), 2);
        assert_eq!(workspace["nodes"][0]["nodes"][0]["layout"], "splith");
        assert_eq!(
            workspace["nodes"][0]["nodes"][0]["nodes"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        let outer = &workspace["nodes"][0]["rect"];
        let inner = &workspace["nodes"][0]["nodes"][0]["rect"];
        assert_eq!(outer["y"], 44);
        assert_eq!(outer["height"], 1036);
        assert_eq!(
            inner["y"],
            if expected_inner == "stacked" { 110 } else { 66 }
        );
        assert_eq!(
            inner["height"],
            if expected_inner == "stacked" {
                970
            } else {
                1014
            }
        );
    }
}

#[test]
fn layout_splitv_wraps_a_single_window_without_changing_the_workspace_axis() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("fixture-1".into());
    let surface = window.surface.clone();
    window.commit();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    assert!(crate::command::execute(f.niri_state(), "layout splitv")[0].success);
    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    let workspace = &tree["nodes"][1]["nodes"][0];
    assert_eq!(workspace["layout"], "splith");
    assert_eq!(workspace["orientation"], "horizontal");
    assert_eq!(workspace["representation"], "H[V[fixture-1]]");
    assert_eq!(workspace["nodes"][0]["layout"], "splitv");
    assert_eq!(workspace["nodes"][0]["nodes"][0]["app_id"], "fixture-1");
}

#[test]
fn splitting_a_focused_container_keeps_it_nested() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    assert!(crate::command::execute(f.niri_state(), "workspace resize-levels")[0].success);
    assert!(crate::command::execute(f.niri_state(), "split h")[0].success);
    let client = f.add_client();
    map_test_window(&mut f, client, "first");
    map_test_window(&mut f, client, "second");
    map_test_window(&mut f, client, "third");
    map_test_window(&mut f, client, "fourth");
    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
    assert!(crate::command::execute(f.niri_state(), "split h")[0].success);
    map_test_window(&mut f, client, "fifth");

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    let workspace = &tree["nodes"][1]["nodes"][0];
    assert_eq!(
        workspace["representation"],
        "H[H[first second third fourth] fifth]"
    );
    assert_eq!(workspace["nodes"][0]["layout"], "splith");
}

#[test]
fn splitting_the_workspace_keeps_its_representation_until_the_tree_changes() {
    // Sway's workspace_split wraps the children and changes the workspace layout without
    // refreshing the representation (sway/tree/workspace.c:1058-1079). Oracle row:
    // split_workspace_keeps_stale_representation.
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    map_test_window(&mut f, client, "fixture-1");
    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
    assert!(crate::command::execute(f.niri_state(), "split v")[0].success);

    let workspace_json = |f: &mut Fixture| {
        let swayward = f.swayward();
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &Default::default(),
            &Default::default(),
        ))
        .unwrap();
        tree["nodes"][1]["nodes"][0].clone()
    };
    let workspace = workspace_json(&mut f);
    assert_eq!(workspace["layout"], "splitv");
    assert_eq!(workspace["representation"], "H[H[fixture-1]]");
    assert_eq!(workspace["nodes"][0]["layout"], "splith");

    map_test_window(&mut f, client, "fixture-2");
    let workspace = workspace_json(&mut f);
    assert_eq!(workspace["representation"], "V[H[fixture-1] fixture-2]");
}

#[test]
fn removing_one_of_two_split_windows_preserves_the_wrapper() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    map_test_window(&mut f, client, "fixture-1");
    assert!(crate::command::execute(f.niri_state(), "layout splitv")[0].success);
    map_test_window(&mut f, client, "fixture-2");

    assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);
    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    let workspace = &tree["nodes"][1]["nodes"][0];
    assert_eq!(workspace["representation"], "H[V[fixture-1]]");
    assert_eq!(workspace["nodes"][0]["layout"], "splitv");
    assert_eq!(workspace["nodes"][0]["nodes"][0]["app_id"], "fixture-1");
}

#[test]
fn moving_a_single_window_in_its_split_direction_reaps_the_old_wrapper() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("fixture-1".into());
    let surface = window.surface.clone();
    window.commit();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    assert!(crate::command::execute(f.niri_state(), "layout splitv")[0].success);
    assert!(crate::command::execute(f.niri_state(), "move down")[0].success);
    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    let workspace = &tree["nodes"][1]["nodes"][0];
    assert_eq!(workspace["layout"], "splitv");
    assert_eq!(workspace["representation"], "V[fixture-1]");
    assert_eq!(workspace["nodes"][0]["layout"], "none");
    assert_eq!(workspace["nodes"][0]["app_id"], "fixture-1");
}

#[test]
fn moving_a_single_window_across_its_split_direction_rewraps_the_workspace_children() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("fixture-1".into());
    let surface = window.surface.clone();
    window.commit();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    assert!(crate::command::execute(f.niri_state(), "layout splitv")[0].success);
    assert!(crate::command::execute(f.niri_state(), "move right")[0].success);
    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    let workspace = &tree["nodes"][1]["nodes"][0];
    assert_eq!(workspace["layout"], "splith");
    assert_eq!(workspace["representation"], "H[V[fixture-1]]");
    assert_eq!(workspace["nodes"][0]["layout"], "splitv");
    assert_eq!(workspace["nodes"][0]["nodes"][0]["app_id"], "fixture-1");
}

#[test]
fn fullscreen_floating_window_keeps_sways_raw_focus() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("fixture-1".into());
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);
    assert!(crate::command::execute(f.niri_state(), "fullscreen enable")[0].success);

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    assert_eq!(
        find_json_node_with_app_id(&tree, "fixture-1").unwrap()["focused"],
        true
    );
}

// random seed 260 step 5 (sway-1.12-random): a floating fullscreen window
// hides the tiled windows beside it (`view_is_visible`,
// sway/tree/view.c:1187-1193).
#[test]
fn floating_fullscreen_hides_tiled_windows() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    for app_id in ["fixture-1", "fixture-2"] {
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

    assert!(crate::command::execute(f.niri_state(), "floating toggle")[0].success);
    assert!(crate::command::execute(f.niri_state(), "fullscreen toggle")[0].success);

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    let fullscreen = find_json_node_with_app_id(&tree, "fixture-2").unwrap();
    assert_eq!(fullscreen["type"], "floating_con");
    assert_eq!(fullscreen["fullscreen_mode"], 1);
    assert_eq!(fullscreen["visible"], true);
    assert_eq!(
        find_json_node_with_app_id(&tree, "fixture-1").unwrap()["visible"],
        false
    );
}

#[test]
fn layout_commands_apply_to_children_inside_a_floating_group() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();

    for app_id in ["fixture-1", "fixture-2"] {
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
    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
    assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus child")[0].success);

    for (command, expected) in [
        ("layout splitv", crate::layout::tiling_tree::Layout::SplitV),
        ("layout tabbed", crate::layout::tiling_tree::Layout::Tabbed),
        (
            "layout stacking",
            crate::layout::tiling_tree::Layout::Stacked,
        ),
        (
            "layout toggle split",
            crate::layout::tiling_tree::Layout::SplitV,
        ),
        ("layout default", crate::layout::tiling_tree::Layout::SplitV),
    ] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success);
        let workspace = f.swayward().layout.active_workspace().unwrap();
        let (_, tree, _) = workspace.ipc_floating_trees().next().unwrap();
        assert!(
            matches!(
                tree,
                crate::layout::tiling_tree::IpcNode::Split { layout, .. } if layout == expected
            ),
            "{command}: {tree:?}"
        );
    }
}

#[test]
fn focus_parent_then_layout_targets_the_parent_of_the_focused_container() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();

    for command in [None, Some("split v")] {
        if let Some(command) = command {
            assert!(crate::command::execute(f.niri_state(), command)[0].success);
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

    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
    assert!(crate::command::execute(f.niri_state(), "layout tabbed")[0].success);
    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    let workspace = &tree["nodes"][1]["nodes"][0];
    // `split v` retargets the singleton workspace root rather than wrapping it
    // (sway container.c:1565), so the two windows sit directly under the
    // workspace. `focus parent` then focuses that root, and `layout tabbed`
    // targets its parent, the workspace itself.
    assert_eq!(workspace["layout"], "tabbed");
    assert_eq!(workspace["nodes"].as_array().unwrap().len(), 2);
    assert!(workspace["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .all(|node| node["focused"] == false));
}

#[test]
fn focus_child_from_workspace_restores_the_floating_child() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    for floating in [false, true] {
        let window = f.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        if floating {
            assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);
        }
    }
    let floating = f.swayward().layout.focus().unwrap().id();

    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
    assert!(f
        .swayward()
        .layout
        .active_workspace()
        .unwrap()
        .is_workspace_focused());
    assert!(crate::command::execute(f.niri_state(), "focus child")[0].success);

    assert_eq!(f.swayward().layout.focus().unwrap().id(), floating);

    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus tiling")[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus child")[0].success);
    assert_ne!(f.swayward().layout.focus().unwrap().id(), floating);
}

#[test]
fn focused_container_can_be_marked_and_targeted_by_con_id() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    for _ in 0..3 {
        let window = f.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    }

    f.swayward().layout.nest_or_unnest_window_left(None);
    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
    assert!(crate::command::execute(f.niri_state(), "mark parent")[0].success);
    let swayward = f.swayward();
    assert!(!swayward.marks_by_container.is_empty());
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let parent = find_json_node_with_mark(&tree, "parent").unwrap();
    let parent_id = parent["id"].as_i64().unwrap();

    let outcome = crate::command::execute(
        f.niri_state(),
        &format!("[con_id={parent_id}] layout tabbed"),
    );
    assert!(outcome[0].success);
    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    assert_eq!(
        find_json_node(&tree, "con", true).unwrap()["layout"],
        "tabbed"
    );

    let outcome = crate::command::execute(f.niri_state(), "[con_id=__focused__] layout stacking");
    assert!(outcome[0].success);
    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    assert_eq!(
        find_json_node(&tree, "con", true).unwrap()["layout"],
        "stacked"
    );
}

#[test]
fn wrapping_a_tiled_child_does_not_promote_it_past_an_older_floating_child() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();

    for app_id in ["floating", "tiled"] {
        map_test_window(&mut f, client, app_id);
        if app_id == "floating" {
            assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);
        }
    }
    assert!(crate::command::execute(f.niri_state(), "layout stacking")[0].success);

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    let workspace = &tree["nodes"][1]["nodes"][0];
    assert_eq!(workspace["focus"][0], workspace["floating_nodes"][0]["id"]);
}

#[test]
fn scripted_split_nesting_is_bounded() {
    // `splitt; focus parent; splitt` wraps the focused container once per
    // round. Sway nests without limit (container_split,
    // sway/tree/container.c:1508-1560); swayward stops at its depth bound so
    // the recursive geometry and GET_TREE walks cannot exhaust the stack.
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    for _ in 0..3 {
        let window = f.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    }
    for _ in 0..4096 {
        for outcome in crate::command::execute(f.niri_state(), "splitt; focus parent; splitt") {
            assert!(outcome.success, "{outcome:?}");
        }
    }
    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    fn depth(node: &Value) -> usize {
        1 + node["nodes"]
            .as_array()
            .into_iter()
            .flatten()
            .map(depth)
            .max()
            .unwrap_or(0)
    }
    // root, output, workspace, then the tiling tree below the workspace.
    assert!(
        depth(&tree) <= 3 + crate::layout::tiling_tree::MAX_TREE_DEPTH,
        "{}",
        depth(&tree)
    );
    swayward.layout.verify_invariants();
}

fn map_app(f: &mut Fixture, client: super::client::ClientId, app_id: &str) {
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

fn tree_json(f: &mut Fixture) -> serde_json::Value {
    let swayward = f.swayward();
    serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap()
}

/// A view mapped beside a fullscreen view is never committed, so sway
/// reports calloc's `border none` and zero box until something attaches it
/// to a container.
fn assert_uncommitted(view: &serde_json::Value) {
    assert_eq!(view["border"], "none", "{view}");
    assert_eq!(view["current_border_width"], 0, "{view}");
    assert_eq!(view["percent"], 0.0, "{view}");
    assert_eq!(view["rect"]["width"], 0, "{view}");
    assert_eq!(view["rect"]["height"], 0, "{view}");
}

/// Committed but never arranged: the configured border over a zero box below
/// the titlebar, still with no content box.
fn assert_committed_unarranged(view: &serde_json::Value, fullscreen: &serde_json::Value) {
    assert_eq!(view["border"], "normal", "{view}");
    assert_eq!(
        view["current_border_width"], fullscreen["current_border_width"],
        "{view}"
    );
    assert!(view["current_border_width"].as_i64().unwrap() > 0, "{view}");
    let titlebar = view["deco_rect"]["height"].as_i64().unwrap();
    assert!(titlebar > 0, "{view}");
    assert_eq!(view["rect"]["width"], 0, "{view}");
    assert_eq!(view["rect"]["height"], -titlebar, "{view}");
    assert_eq!(view["window_rect"]["width"], 0, "{view}");
    assert_eq!(view["window_rect"]["height"], 0, "{view}");
}

// random seed 29 step 9 (sway-1.12-random): `layout` under a
// fullscreen view wraps the workspace children (`workspace_wrap_children`,
// sway/tree/workspace.c:898-910), and `container_add_child` marks the hidden
// view dirty (sway/tree/container.c:1436-1437). Its border is committed but
// `arrange_workspace` lays out only the fullscreen view
// (sway/tree/arrange.c:310-316), and the wrapper's empty box drops percent.
#[test]
fn layout_wrap_under_fullscreen_commits_the_hidden_views_border() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    map_app(&mut f, client, "fullscreen");
    assert!(crate::command::execute(f.niri_state(), "fullscreen toggle")[0].success);
    map_app(&mut f, client, "hidden");
    assert_uncommitted(find_json_node_with_app_id(&tree_json(&mut f), "hidden").unwrap());

    // The workspace is splith, so a different layout makes the command act.
    assert!(crate::command::execute(f.niri_state(), "layout splitv")[0].success);

    let tree = tree_json(&mut f);
    let hidden = find_json_node_with_app_id(&tree, "hidden").unwrap();
    let fullscreen = find_json_node_with_app_id(&tree, "fullscreen").unwrap();
    assert_committed_unarranged(hidden, fullscreen);
    assert_eq!(hidden["percent"], serde_json::Value::Null);
}

fn rect(view: &serde_json::Value, key: &str) -> [i64; 4] {
    ["x", "y", "width", "height"].map(|field| view[key][field].as_i64().unwrap())
}

// differential seeds 1684 step 4, 2466 step 4 (sway-1.12): under a stacked
// parent `get_deco_rect` places the titlebar row by sibling index and
// `ipc_json_describe_node` subtracts one row per sibling from the box, so an
// unarranged view's empty box is reported 54 px down at height -54
// (sway/ipc-json.c:543-580, 816-825). Its border is irrelevant: calloc's
// `border none` view gets the same rows.
#[test]
fn unarranged_view_under_stacked_parent_reports_every_titlebar_row() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    assert!(crate::command::execute(f.niri_state(), "layout stacking")[0].success);
    map_app(&mut f, client, "fullscreen");
    assert!(crate::command::execute(f.niri_state(), "fullscreen enable")[0].success);
    map_app(&mut f, client, "hidden");

    let tree = tree_json(&mut f);
    let hidden = find_json_node_with_app_id(&tree, "hidden").unwrap();
    let titlebar = hidden["deco_rect"]["height"].as_i64().unwrap();
    assert!(titlebar > 0, "{hidden}");
    assert_eq!(hidden["border"], "none", "{hidden}");
    assert_eq!(rect(hidden, "rect"), [0, 2 * titlebar, 0, -2 * titlebar]);
    assert_eq!(rect(hidden, "deco_rect"), [0, titlebar, 0, titlebar]);
}

// differential seeds 1482 step 4, 2436 step 5, 4912 step 5 (sway-1.12): a
// view mapped into a `layout` wrapper under fullscreen makes `view_map`
// arrange the wrapper (sway/tree/view.c:931-940). The wrapper was never
// arranged, so its children are laid out inside its empty box: the
// fullscreen view reports 0x0 and the new view a titlebar over an empty box
// with a 1x1 content box (sway/tree/arrange.c:183-212,
// sway/tree/view.c:461-462). `workspace_switch` then arranges the workspace,
// which restores only the fullscreen view's output box
// (sway/tree/workspace.c:731-743, sway/tree/arrange.c:310-316).
#[test]
fn view_mapped_into_layout_wrapper_under_fullscreen_arranges_it_at_an_empty_box() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    map_app(&mut f, client, "fullscreen");
    assert!(crate::command::execute(f.niri_state(), "fullscreen toggle")[0].success);
    assert!(crate::command::execute(f.niri_state(), "default_border none")[0].success);
    assert!(crate::command::execute(f.niri_state(), "layout tabbed")[0].success);
    map_app(&mut f, client, "hidden");

    let tree = tree_json(&mut f);
    let fullscreen = find_json_node_with_app_id(&tree, "fullscreen").unwrap();
    let hidden = find_json_node_with_app_id(&tree, "hidden").unwrap();
    assert_eq!(rect(fullscreen, "rect"), [0, 0, 0, 0], "{fullscreen}");
    let titlebar = hidden["deco_rect"]["height"].as_i64().unwrap();
    assert!(titlebar > 0, "{hidden}");
    assert_eq!(rect(hidden, "rect"), [0, titlebar, 0, -titlebar]);
    assert_eq!(rect(hidden, "window_rect"), [0, 0, 1, 1]);
    assert_eq!(hidden["percent"], serde_json::Value::Null);

    assert!(crate::command::execute(f.niri_state(), "workspace 1")[0].success);

    let tree = tree_json(&mut f);
    let fullscreen = find_json_node_with_app_id(&tree, "fullscreen").unwrap();
    let hidden = find_json_node_with_app_id(&tree, "hidden").unwrap();
    assert_eq!(rect(fullscreen, "rect"), [0, 0, 1280, 720], "{fullscreen}");
    assert_eq!(rect(hidden, "rect"), [0, titlebar, 0, -titlebar]);
}

// differential seed 2576 step 4 (sway-1.12): `layout stacking` under
// fullscreen wraps the workspace without arranging the siblings, so the
// tiled view keeps its 640 px tiled box but GET_TREE subtracts both stacked
// rows from it (sway/ipc-json.c:816-825).
#[test]
fn layout_stacking_under_fullscreen_reports_every_row_over_the_kept_box() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    map_app(&mut f, client, "tiled");
    map_app(&mut f, client, "fullscreen");
    assert!(crate::command::execute(f.niri_state(), "fullscreen toggle")[0].success);
    assert!(crate::command::execute(f.niri_state(), "layout stacking")[0].success);

    let tree = tree_json(&mut f);
    let tiled = find_json_node_with_app_id(&tree, "tiled").unwrap();
    let titlebar = tiled["deco_rect"]["height"].as_i64().unwrap();
    assert!(titlebar > 0, "{tiled}");
    assert_eq!(
        rect(tiled, "rect"),
        [0, 2 * titlebar, 640, 720 - 2 * titlebar]
    );
}

// random seed 47 step 6 (sway-1.12-random): with only a floating fullscreen
// view on the workspace, a new view has no tiling sibling, so `view_map`
// attaches it with `workspace_add_tiling` (sway/tree/view.c:849-901), which
// marks it dirty (sway/tree/workspace.c:956-957).
#[test]
fn view_mapped_under_floating_fullscreen_keeps_its_border() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    map_app(&mut f, client, "fullscreen");
    assert!(crate::command::execute(f.niri_state(), "floating toggle")[0].success);
    assert!(crate::command::execute(f.niri_state(), "fullscreen toggle")[0].success);
    map_app(&mut f, client, "hidden");

    let tree = tree_json(&mut f);
    let hidden = find_json_node_with_app_id(&tree, "hidden").unwrap();
    let fullscreen = find_json_node_with_app_id(&tree, "fullscreen").unwrap();
    assert_committed_unarranged(hidden, fullscreen);
    assert_eq!(hidden["percent"], 0.0);
    assert_eq!(hidden["visible"], false);
}

// random seed 29 step 7 (sway-1.12-random): beside a tiled fullscreen view
// `view_map` uses `container_add_sibling`, which does not mark the new view
// dirty (sway/tree/container.c:1410-1423), so it stays uncommitted.
#[test]
fn view_mapped_beside_tiled_fullscreen_stays_uncommitted() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    map_app(&mut f, client, "fullscreen");
    assert!(crate::command::execute(f.niri_state(), "fullscreen toggle")[0].success);
    map_app(&mut f, client, "hidden");

    assert_uncommitted(find_json_node_with_app_id(&tree_json(&mut f), "hidden").unwrap());
}

// random seed 355 step 10 (sway-1.12-random): with the workspace focused
// there is no container, and sway's `fullscreen` succeeds without doing
// anything (sway/commands/fullscreen.c:22-25), so a view mapped later tiles
// normally.
#[test]
fn fullscreen_on_a_focused_workspace_does_nothing() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    map_app(&mut f, client, "first");
    map_app(&mut f, client, "second");
    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);

    assert!(crate::command::execute(f.niri_state(), "fullscreen toggle")[0].success);
    map_app(&mut f, client, "third");

    let tree = tree_json(&mut f);
    for app_id in ["first", "second", "third"] {
        let view = find_json_node_with_app_id(&tree, app_id).unwrap();
        assert_eq!(view["fullscreen_mode"], 0, "{view}");
        assert_eq!(view["border"], "normal", "{view}");
        assert!(view["rect"]["width"].as_i64().unwrap() > 0, "{view}");
    }
}

// random seed 436 step 14 (sway-1.12-random): floating the only child of a
// fullscreen split reaps the split, and destroying the fullscreen container
// ends fullscreen (`container_begin_destroy`, sway/tree/container.c:480-482).
// The view that was mapped hidden under it is then arranged with its border.
#[test]
fn reaping_the_fullscreen_container_arranges_the_hidden_view() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    map_app(&mut f, client, "first");
    map_app(&mut f, client, "fullscreen");
    assert!(crate::command::execute(f.niri_state(), "fullscreen toggle")[0].success);
    map_app(&mut f, client, "hidden");
    assert!(crate::command::execute(f.niri_state(), "splith")[0].success);
    assert_uncommitted(find_json_node_with_app_id(&tree_json(&mut f), "hidden").unwrap());

    assert!(crate::command::execute(f.niri_state(), "floating toggle")[0].success);

    let tree = tree_json(&mut f);
    let hidden = find_json_node_with_app_id(&tree, "hidden").unwrap();
    let first = find_json_node_with_app_id(&tree, "first").unwrap();
    assert_eq!(hidden["border"], first["border"], "{hidden}");
    assert_eq!(hidden["percent"], 0.5, "{hidden}");
    assert!(hidden["rect"]["width"].as_i64().unwrap() > 0, "{hidden}");
}

// random seeds 203 step 5, 447 step 19, 459 step 16 (sway-1.12-random):
// `splith` on a fullscreen view moves fullscreen to the new wrapper
// (`container_replace`, sway/tree/container.c:1471-1501) and returns focus to
// the view (sway/tree/container.c:1554-1560). `fullscreen toggle` then reads
// the view's own mode (sway/commands/fullscreen.c:33), so it fullscreens the
// view and drops the wrapper's (sway/tree/container.c:1312-1315).
#[test]
fn fullscreen_toggle_after_split_targets_the_view() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    map_app(&mut f, client, "fixture-1");
    map_app(&mut f, client, "fixture-2");
    for command in ["fullscreen toggle", "splith"] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success);
    }
    let tree = tree_json(&mut f);
    let wrapper = &tree["nodes"][1]["nodes"][0]["nodes"][1];
    assert_eq!(wrapper["fullscreen_mode"], 1, "{wrapper}");
    assert_eq!(wrapper["nodes"][0]["fullscreen_mode"], 0, "{wrapper}");

    assert!(crate::command::execute(f.niri_state(), "fullscreen toggle")[0].success);
    let tree = tree_json(&mut f);
    let wrapper = &tree["nodes"][1]["nodes"][0]["nodes"][1];
    assert_eq!(wrapper["type"], "con", "{wrapper}");
    assert_eq!(wrapper["fullscreen_mode"], 0, "{wrapper}");
    // Sway arranges only the new fullscreen view (sway/tree/arrange.c:310-316),
    // so the wrapper keeps the output box it had while fullscreen.
    assert_eq!(wrapper["rect"]["width"], 1280, "{wrapper}");
    assert_eq!(wrapper["percent"], 1.0, "{wrapper}");
    assert_eq!(wrapper["nodes"][0]["percent"], 1.0, "{wrapper}");
    assert_eq!(wrapper["nodes"][0]["app_id"], "fixture-2", "{wrapper}");
    assert_eq!(wrapper["nodes"][0]["fullscreen_mode"], 1, "{wrapper}");
    assert_eq!(wrapper["nodes"][0]["focused"], true, "{wrapper}");
}

fn run(f: &mut Fixture, commands: &[&str]) {
    for command in commands {
        assert!(
            crate::command::execute(f.niri_state(), command)[0].success,
            "{command}"
        );
    }
}

// Differential family diff-fam-fullscreen-percent, seeds 1006 1167 1477
// (gaps outer) and 1295 1759 (gaps horizontal). A workspace fullscreen view's
// box is the output's (sway/tree/arrange.c:310-316), and its percent is that
// box over the workspace's pending box (sway/ipc-json.c:744-755), which the
// gaps shrink. Sway does not clamp it to 1.
#[test]
fn workspace_fullscreen_percent_is_the_output_over_the_gapped_workspace() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    map_app(&mut f, client, "fullscreen");
    run(&mut f, &["fullscreen enable", "gaps outer current plus 5"]);

    let tree = tree_json(&mut f);
    let view = find_json_node_with_app_id(&tree, "fullscreen").unwrap();
    assert_eq!(view["rect"]["width"], 1280, "{view}");
    assert_eq!(view["percent"], (1280. / 1270.) * (720. / 710.), "{view}");
}

// Differential family diff-fam-fullscreen-percent, seeds 1029 (gaps), 1158
// (titlebar_border_thickness) and 3041 (titlebar_padding). A global
// fullscreen container is not `workspace->fullscreen`, so a command that
// arranges only the workspace (sway/commands/gaps.c:136,
// sway/commands/titlebar_padding.c:35) lays it out in its tile slot
// (sway/tree/arrange.c:310-322); only its content keeps the root box.
#[test]
fn workspace_arrange_puts_a_global_fullscreen_view_in_its_tile_slot() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    map_app(&mut f, client, "tiled");
    map_app(&mut f, client, "fullscreen");
    run(&mut f, &["fullscreen toggle global", "titlebar_padding 1"]);

    let tree = tree_json(&mut f);
    let view = find_json_node_with_app_id(&tree, "fullscreen").unwrap();
    assert_eq!(view["percent"], 0.5, "{view}");
    assert_eq!(view["rect"]["x"], 640, "{view}");
    assert_eq!(view["rect"]["width"], 640, "{view}");
    assert_eq!(view["window_rect"]["x"], -640, "{view}");
    assert_eq!(view["window_rect"]["width"], 1280, "{view}");

    // smart_gaps arranges the root (sway/commands/smart_gaps.c:25), which
    // gives the global fullscreen view the root box again
    // (sway/tree/arrange.c:349-355).
    run(&mut f, &["smart_gaps on"]);
    let tree = tree_json(&mut f);
    let view = find_json_node_with_app_id(&tree, "fullscreen").unwrap();
    assert_eq!(view["percent"], 1.0, "{view}");
    assert_eq!(view["rect"]["width"], 1280, "{view}");

    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    map_app(&mut f, client, "fullscreen");
    run(
        &mut f,
        &["fullscreen toggle global", "gaps outer current plus 5"],
    );

    let tree = tree_json(&mut f);
    let view = find_json_node_with_app_id(&tree, "fullscreen").unwrap();
    assert_eq!(view["percent"], 1.0, "{view}");
    assert_eq!(view["rect"]["x"], 5, "{view}");
    assert_eq!(view["rect"]["width"], 1270, "{view}");
    // The normal border's title bar is still enabled from the tiled
    // arrange, so window_rect.y is 0 (sway/ipc-json.c:596-601).
    assert_eq!(view["window_rect"]["x"], -5, "{view}");
    assert_eq!(view["window_rect"]["y"], 0, "{view}");
}

// Differential family diff-fam-fullscreen-percent, seed 6264. `layout` wraps
// the workspace children (sway/tree/workspace.c:898-910); detaching the
// global fullscreen view clears `root->fullscreen_global`
// (sway/tree/container.c:1440-1446), so the workspace arrange that follows
// lays the wrapper and the view out in their tile slots.
#[test]
fn layout_wrap_under_global_fullscreen_arranges_the_wrapper() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    map_app(&mut f, client, "fullscreen");
    run(&mut f, &["fullscreen toggle global", "layout splitv"]);

    let tree = tree_json(&mut f);
    let wrapper = &tree["nodes"][1]["nodes"][0]["nodes"][0];
    assert_eq!(wrapper["layout"], "splitv", "{wrapper}");
    assert_eq!(wrapper["percent"], 1.0, "{wrapper}");
    assert_eq!(wrapper["rect"]["width"], 1280, "{wrapper}");
    assert_eq!(wrapper["nodes"][0]["percent"], 1.0, "{wrapper}");
}

// Differential family diff-fam-fullscreen-percent, seeds 2674 and 1158. Sway gives the
// last of three columns of 1280 the 426 px remainder (sway/tree/arrange.c:
// 78-88), and a fullscreen child's percent is the output over that box.
#[test]
fn fullscreen_child_percent_uses_the_parents_whole_pixel_slot() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    for app_id in ["one", "two", "three"] {
        map_app(&mut f, client, app_id);
    }
    run(&mut f, &["splith", "fullscreen toggle global"]);

    let tree = tree_json(&mut f);
    let view = find_json_node_with_app_id(&tree, "three").unwrap();
    assert_eq!(view["percent"], (1280. / 426.) * (720. / 720.), "{view}");
}

/// `layout default` restores the split layout the `layout` command last
/// replaced on that node, including an empty workspace, and fails when none
/// was recorded (sway/commands/layout.c:106-108,160-189). A move that
/// reorients the workspace does not record one (sway/commands/move.c:331-340).
/// Oracle rows differential_seed_1257, 1364, 1718, 1992 and
/// layout_default_prev_split.
#[test]
fn layout_default_restores_only_a_split_the_layout_command_replaced() {
    let expected_syntax = "Expected 'layout default|tabbed|stacking|splitv|splith' or 'layout toggle [split|all]' or 'layout toggle [split|tabbed|stacking|splitv|splith] [split|tabbed|stacking|splitv|splith]...'";
    for layout in ["stacking", "tabbed"] {
        let mut f = Fixture::new();
        f.add_output(1, (1920, 1080));
        assert!(crate::command::execute(f.niri_state(), &format!("layout {layout}"))[0].success);
        let reply = &crate::command::execute(f.niri_state(), "layout default")[0];
        assert!(reply.success, "{layout}: {reply:?}");
        let tree = tree_json(&mut f);
        assert_eq!(tree["nodes"][1]["nodes"][0]["layout"], "splith", "{layout}");
    }

    // With a view focused, the layout lands on a new wrapper whose
    // prev_split_layout starts empty (sway/commands/layout.c:178-183,
    // sway/tree/container.c:112), so `layout default` has nothing to restore.
    for setup in ["layout stacking", "layout tabbed", "move down", "move up"] {
        let mut f = Fixture::new();
        f.add_output(1, (1920, 1080));
        let client = f.add_client();
        map_app(&mut f, client, "fixture-1");
        assert!(crate::command::execute(f.niri_state(), setup)[0].success);
        let reply = &crate::command::execute(f.niri_state(), "layout default")[0];
        assert!(!reply.success, "{setup}: {reply:?}");
        assert_eq!(reply.error.as_deref(), Some(expected_syntax));
        assert_eq!(reply.parse_error, Some(true));
    }
}

#[test]
fn split_toggle_reads_the_parent_layout_like_sway() {
    // sway `cmd_split`/`cmd_splitt` (sway/commands/split.c): split H only when the
    // focused container's parent is V; a focused workspace always splits V.
    // Oracle rows differential_seed_1013 and differential_seed_1732.
    let workspace_layout = |f: &mut Fixture| {
        let swayward = f.swayward();
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &Default::default(),
            &Default::default(),
        ))
        .unwrap();
        let workspace = &tree["nodes"][1]["nodes"][0];
        (
            workspace["layout"].as_str().unwrap().to_owned(),
            workspace["representation"].as_str().map(str::to_owned),
        )
    };

    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    for command in ["split toggle", "split toggle"] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success);
    }
    assert_eq!(workspace_layout(&mut f).0, "splitv");

    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    map_test_window(&mut f, client, "only");
    assert!(crate::command::execute(f.niri_state(), "splitt")[0].success);
    assert_eq!(
        workspace_layout(&mut f),
        ("splitv".to_owned(), Some("V[only]".to_owned()))
    );
}

#[test]
fn layout_toggle_on_an_empty_workspace_toggles_the_workspace() {
    // sway/commands/layout.c:159-163,184-188: with no focused container the
    // workspace layout itself toggles. Oracle rows differential_seed_1174,
    // differential_seed_1111 and differential_seed_1088.
    for command in ["layout toggle split", "layout toggle", "layout toggle all"] {
        let mut f = Fixture::new();
        f.add_output(1, (1920, 1080));
        assert!(crate::command::execute(f.niri_state(), command)[0].success);
        let swayward = f.swayward();
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &Default::default(),
            &Default::default(),
        ))
        .unwrap();
        let workspace = &tree["nodes"][1]["nodes"][0];
        assert_eq!(workspace["layout"], "splitv", "{command}");
        assert_eq!(workspace["representation"], "V[]", "{command}");
    }
}

// random-v2 seed 1057 step 5 (diff-fam-percent-rounding): sway snaps the
// siblings' fractions to their whole-pixel boxes before a command resize
// (`container_resize_tiled`, sway/commands/resize.c:126-131), and a new view
// then takes the average of those fractions (sway/tree/arrange.c:48-52).
#[test]
fn resize_snaps_fractions_to_whole_pixels_before_a_new_view() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    for app_id in ["fixture-1", "fixture-2", "fixture-3"] {
        map_app(&mut f, client, app_id);
    }
    assert!(crate::command::execute(f.niri_state(), "resize grow width 10 px")[0].success);
    map_app(&mut f, client, "fixture-4");

    let tree = tree_json(&mut f);
    let percents = ["fixture-1", "fixture-2", "fixture-3", "fixture-4"]
        .map(|app_id| find_json_node_with_app_id(&tree, app_id).unwrap()["percent"].clone());
    assert_eq!(
        percents,
        [0.24765625, 0.24765625, 0.25546875, 0.24921875].map(serde_json::Value::from)
    );
}

// random-v2 seed 1071 step 5 (diff-fam-percent-rounding): under a fullscreen
// view, sway leaves the other children at the whole-pixel boxes they last had
// over the visible children (`arrange_workspace`, sway/tree/arrange.c:310-316):
// 1280 / 3 rounds to 427 px, so percent is 427 / 1280.
#[test]
fn view_mapped_under_fullscreen_keeps_whole_pixel_sibling_fractions() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    for app_id in ["fixture-1", "fixture-2", "fixture-3"] {
        map_app(&mut f, client, app_id);
    }
    assert!(crate::command::execute(f.niri_state(), "fullscreen toggle")[0].success);
    map_app(&mut f, client, "fixture-4");

    let tree = tree_json(&mut f);
    for app_id in ["fixture-1", "fixture-2"] {
        let view = find_json_node_with_app_id(&tree, app_id).unwrap();
        assert_eq!(view["percent"], 0.33359375, "{view}");
    }
}

#[test]
fn moving_a_fullscreen_floating_window_leaves_the_new_workspace_without_representation() {
    // sway `workspace_add_floating` (sway/tree/workspace.c:960-970) never calls
    // `workspace_update_representation`, so a workspace that only ever held a
    // floating child reports `representation: null`. Oracle rows
    // differential_seed_7532 and move_fullscreen_floating_keeps_null_representation.
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    map_test_window(&mut f, client, "only");
    for command in [
        "floating toggle",
        "fullscreen toggle",
        "move container to workspace 2",
    ] {
        assert!(
            crate::command::execute(f.niri_state(), command)[0].success,
            "{command}"
        );
    }
    let tree = tree_json(&mut f);
    let workspaces = tree["nodes"][1]["nodes"].as_array().unwrap();
    let target = workspaces.iter().find(|ws| ws["name"] == "2").unwrap();
    assert_eq!(
        target["floating_nodes"].as_array().unwrap().len(),
        1,
        "{target}"
    );
    assert!(target["representation"].is_null(), "{target}");
}
