#[test]
fn initial_workspaces_keep_the_pre_config_output_orientation() {
    let (mut f, socket) = ipc_fixture();
    f.add_output(1, (1280, 720));
    f.add_output(2, (1280, 720));
    for output in [f.niri_output(1), f.niri_output(2)] {
        let mode = smithay::output::Mode {
            size: (1270, 1408).into(),
            refresh: 60_000,
        };
        output.change_current_state(Some(mode), None, None, None);
        f.swayward().layout.update_output_size(&output);
    }

    let mut stream = UnixStream::connect(socket).unwrap();
    let workspaces = query_ipc(&mut f, &mut stream, MessageType::GetWorkspaces);
    assert_eq!(workspaces.as_array().unwrap().len(), 2);
    assert!(workspaces
        .as_array()
        .unwrap()
        .iter()
        .all(|workspace| workspace["layout"] == "splith"));
}

#[test]
fn moving_workspace_reaps_an_interrupted_empty_workspace_switch() {
    let mut f = Fixture::new();
    f.add_named_output_at("left".into(), (1270, 1408), Some((0, 0)));
    f.add_named_output_at("right".into(), (1270, 1408), Some((1270, 0)));
    let client = f.add_client();

    map_test_window(&mut f, client, "first");
    assert!(crate::command::execute(f.niri_state(), "workspace 3")[0].success);
    assert!(crate::command::execute(f.niri_state(), "workspace 4")[0].success);
    map_test_window(&mut f, client, "moved");
    assert!(crate::command::execute(f.niri_state(), "move workspace to output right")[0].success);

    let mut numbers = f
        .swayward()
        .layout
        .workspaces()
        .filter_map(|(_, _, workspace)| workspace.number())
        .collect::<Vec<_>>();
    numbers.sort_unstable();
    assert_eq!(numbers, [1, 4]);
}

#[test]
fn repeatedly_moving_a_workspace_does_not_duplicate_it_in_output_order() {
    let mut f = Fixture::new();
    f.add_named_output_at("left".into(), (1270, 1408), Some((0, 0)));
    f.add_named_output_at("right".into(), (1270, 1408), Some((1270, 0)));
    let client = f.add_client();
    map_test_window(&mut f, client, "moved");

    for _ in 0..100 {
        assert!(
            crate::command::execute(f.niri_state(), "move workspace to output right")[0].success
        );
        assert!(crate::command::execute(f.niri_state(), "output right disable")[0].success);
        assert!(crate::command::execute(f.niri_state(), "output right enable")[0].success);
        assert!(
            crate::command::execute(f.niri_state(), "move workspace to output left")[0].success
        );
    }

    let swayward = f.swayward();
    let tree = describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    );
    let workspace_ids = tree
        .nodes
        .iter()
        .flat_map(|output| &output.nodes)
        .map(|workspace| workspace.id)
        .collect::<Vec<_>>();
    assert_eq!(
        workspace_ids
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len(),
        workspace_ids.len()
    );
}

#[test]
fn repeated_output_unplug_does_not_duplicate_a_restored_workspace() {
    let mut f = Fixture::new();
    f.add_named_output_at("left".into(), (1270, 1408), Some((0, 0)));
    f.add_named_output_at("right".into(), (1270, 1408), Some((1270, 0)));
    let client = f.add_client();
    assert!(crate::command::execute(f.niri_state(), "focus output right")[0].success);
    map_test_window(&mut f, client, "restored");

    assert!(crate::command::execute(f.niri_state(), "output right disable")[0].success);
    assert!(crate::command::execute(f.niri_state(), "output right enable")[0].success);
    assert!(crate::command::execute(f.niri_state(), "output right disable")[0].success);

    let swayward = f.swayward();
    let tree = describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    );
    let workspace_ids = tree
        .nodes
        .iter()
        .flat_map(|output| &output.nodes)
        .map(|workspace| workspace.id)
        .collect::<Vec<_>>();
    assert_eq!(
        workspace_ids
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len(),
        workspace_ids.len()
    );
}

#[test]
fn workspace_back_and_forth_without_history_uses_sway_error() {
    let (mut f, socket) = ipc_fixture();
    f.add_output(1, (1920, 1080));
    let mut stream = UnixStream::connect(socket).unwrap();
    stream
        .write_all(&swayward_ipc::wire::encode(
            MessageType::RunCommand,
            "workspace back_and_forth",
        ))
        .unwrap();
    let (_, reply) = read_ipc_reply(&mut f, &mut stream);
    assert_eq!(
        reply,
        r#"[{"success":false,"error":"There is no previous workspace","parse_error":true}]"#
    );
}

#[test]
fn interrupted_workspace_switch_reaps_its_empty_source() {
    let (mut f, socket) = ipc_fixture();
    f.add_output(1, (1270, 1408));

    for workspace in ["/tmp/first", "/tmp/second", "/tmp/third"] {
        assert!(
            crate::command::execute(f.niri_state(), &format!("workspace {workspace}"))[0].success
        );
    }
    f.swayward().clock.set_complete_instantly(true);
    f.swayward().layout.advance_animations();

    let mut stream = UnixStream::connect(socket).unwrap();
    let workspaces = query_ipc(&mut f, &mut stream, MessageType::GetWorkspaces);
    let names = workspaces
        .as_array()
        .unwrap()
        .iter()
        .map(|workspace| workspace["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(names, ["/tmp/third"]);
}

#[test]
fn workspace_back_and_forth_recreates_a_reaped_previous_workspace() {
    let (mut f, socket) = ipc_fixture();
    f.add_output(1, (1920, 1080));
    let mut stream = UnixStream::connect(socket).unwrap();

    for command in ["workspace 1", "workspace 2"] {
        stream
            .write_all(&swayward_ipc::wire::encode(
                MessageType::RunCommand,
                command,
            ))
            .unwrap();
        let (_, reply) = read_ipc_reply(&mut f, &mut stream);
        assert_eq!(reply, r#"[{"success":true}]"#);
    }

    f.swayward().clock.set_complete_instantly(true);
    f.swayward().layout.advance_animations();
    f.swayward().clock.set_complete_instantly(false);
    assert!(f
        .swayward()
        .layout
        .workspaces()
        .all(|(_, _, workspace)| workspace.number() != Some(1)));

    stream
        .write_all(&swayward_ipc::wire::encode(
            MessageType::RunCommand,
            "workspace back_and_forth",
        ))
        .unwrap();
    let (_, reply) = read_ipc_reply(&mut f, &mut stream);
    assert_eq!(reply, r#"[{"success":true}]"#);
    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().number(),
        Some(1)
    );
}

#[test]
fn move_workspace_back_and_forth_targets_the_previous_workspace() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();

    for workspace in ["1", "2"] {
        assert!(
            crate::command::execute(f.niri_state(), &format!("workspace {workspace}"))[0].success
        );
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(workspace.into());
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    }

    assert!(crate::command::execute(f.niri_state(), "move workspace back_and_forth")[0].success);

    let workspace_apps = f
        .swayward()
        .layout
        .workspaces()
        .filter_map(|(_, _, workspace)| {
            workspace.number().map(|number| {
                let apps = workspace
                    .windows()
                    .map(|window| {
                        crate::utils::with_toplevel_role(window.toplevel(), |role| {
                            role.app_id.clone().unwrap()
                        })
                    })
                    .collect::<Vec<_>>();
                (number, apps)
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        workspace_apps,
        [(1, vec!["1".into(), "2".into()]), (2, vec![])]
    );
}

#[test]
fn move_workspace_current_keeps_the_window_on_its_workspace() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    assert!(crate::command::execute(f.niri_state(), "workspace 7")[0].success);
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    assert!(crate::command::execute(f.niri_state(), "move workspace current")[0].success);

    let swayward = f.swayward();
    let workspaces = describe_workspaces(&swayward.layout, &swayward.global_space);
    // Find workspace 7 rather than assuming it is first: sway sorts numbered
    // workspaces numerically (sway/sway/tree/output.c:387-405), so the startup
    // workspace 1 precedes it. The point of this test is that the window stays
    // on the workspace it was on, not where that workspace sorts.
    let seven = workspaces
        .iter()
        .find(|workspace| workspace.num == 7)
        .expect("workspace 7 exists");
    assert_eq!(seven.focus.len(), 1);
}

#[test]
fn move_to_workspace_creates_the_target_and_moves_the_window() {
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

    assert!(crate::command::execute(f.niri_state(), "move to workspace 7")[0].success);

    let swayward = f.swayward();
    let workspaces = describe_workspaces(&swayward.layout, &swayward.global_space);
    let workspace = workspaces
        .iter()
        .find(|workspace| workspace.num == 7)
        .unwrap();
    assert_eq!(workspace.focus.len(), 1);
    assert!(!workspace.focused);
}

#[test]
fn moving_fullscreen_away_from_a_window_mapped_under_it_keeps_the_moved_leaf_tiled() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();

    assert!(crate::command::execute(f.niri_state(), "workspace source")[0].success);
    map_test_window(&mut f, client, "left");
    map_test_window(&mut f, client, "fullscreen");
    assert!(crate::command::execute(f.niri_state(), "fullscreen enable")[0].success);
    map_test_window(&mut f, client, "mapped-under-fullscreen");

    assert!(crate::command::execute(f.niri_state(), "move workspace destination")[0].success);

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let moved = find_json_node_with_app_id(&tree, "fullscreen").unwrap();
    assert_eq!(moved["type"], "con");
    assert_eq!(moved["floating"], "auto_off");
    assert_eq!(moved["fullscreen_mode"], 1);
}

#[test]
fn move_workspace_focuses_the_moved_window_in_the_destination_reply() {
    let (mut f, socket) = ipc_fixture();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    map_test_window(&mut f, client, "destination-window");
    assert!(crate::command::execute(f.niri_state(), "workspace 2")[0].success);
    map_test_window(&mut f, client, "moved-window");

    let mut stream = UnixStream::connect(socket).unwrap();
    assert_eq!(
        query_ipc_with_payload(
            &mut f,
            &mut stream,
            MessageType::RunCommand,
            "move workspace 1",
        ),
        serde_json::json!([{"success": true}])
    );
    assert_eq!(
        query_ipc_with_payload(&mut f, &mut stream, MessageType::RunCommand, "workspace 1",),
        serde_json::json!([{"success": true}])
    );
    let tree = query_ipc(&mut f, &mut stream, MessageType::GetTree);

    assert_eq!(
        find_json_node_with_app_id(&tree, "moved-window").unwrap()["focused"],
        true
    );
    assert_eq!(
        find_json_node_with_app_id(&tree, "destination-window").unwrap()["focused"],
        false
    );
}

/// Sway evacuates a non-empty workspace intact and records the fallback output
/// in its priority list (`sway/tree/output.c:205-247`). When the unplugged
/// output returns, `restore_workspaces` moves the workspace back to its
/// highest-priority available output (`sway/tree/output.c:31-45,158-159`).
#[test]
fn nested_tabbed_and_stacked_splits_survive_output_unplug() {
    fn output_holding<'a>(node: &'a Value, app_id: &str) -> Option<&'a Value> {
        if node["type"] == "output" && find_json_node_with_app_id(node, app_id).is_some() {
            return Some(node);
        }
        node["nodes"]
            .as_array()?
            .iter()
            .find_map(|child| output_holding(child, app_id))
    }

    for (command_layout, ipc_layout) in [("tabbed", "tabbed"), ("stacking", "stacked")] {
        let (mut f, socket) = ipc_fixture();
        f.add_identified_output(1, (1280, 720));
        f.add_identified_output(2, (1920, 1080));
        let unplugged_name = f.niri_output(1).name();
        let fallback_name = f.niri_output(2).name();
        let client = f.add_client();

        map_test_window(&mut f, client, "outside");
        assert!(crate::command::execute(f.niri_state(), "split horizontal")[0].success);
        map_test_window(&mut f, client, "nested-1");
        assert!(crate::command::execute(f.niri_state(), "split vertical")[0].success);
        map_test_window(&mut f, client, "nested-2");
        assert!(
            crate::command::execute(f.niri_state(), &format!("layout {command_layout}"))[0].success
        );

        let removed = f.niri_output(1);
        f.swayward().remove_output(&removed);

        let mut stream = UnixStream::connect(&socket).unwrap();
        let tree = query_ipc(&mut f, &mut stream, MessageType::GetTree);
        let output = output_holding(&tree, "outside").unwrap();
        assert_eq!(
            output["name"], fallback_name,
            "{command_layout}: evacuation"
        );
        let workspace = find_json_parent_of_app_id(output, "outside").unwrap();
        assert_eq!(
            workspace["layout"], "splith",
            "{command_layout}: outer split"
        );
        assert_eq!(workspace["nodes"].as_array().unwrap().len(), 2);
        assert_eq!(workspace["nodes"][0]["app_id"], "outside");
        let nested = &workspace["nodes"][1];
        assert_eq!(
            nested["layout"], ipc_layout,
            "{command_layout}: nested split"
        );
        assert_eq!(nested["nodes"][0]["app_id"], "nested-1");
        assert_eq!(nested["nodes"][1]["app_id"], "nested-2");
        assert_eq!(nested["focus"][0], nested["nodes"][1]["id"]);
        assert_eq!(
            nested["nodes"][1]["focused"], true,
            "{command_layout}: focus"
        );

        f.add_identified_output(1, (1280, 720));
        let tree = query_ipc(&mut f, &mut stream, MessageType::GetTree);
        let output = output_holding(&tree, "outside").unwrap();
        assert_eq!(
            output["name"], unplugged_name,
            "{command_layout}: restoration"
        );
        let workspace = find_json_parent_of_app_id(output, "outside").unwrap();
        assert_eq!(workspace["layout"], "splith");
        assert_eq!(workspace["nodes"][1]["layout"], ipc_layout);
        assert_eq!(workspace["nodes"][1]["nodes"][0]["app_id"], "nested-1");
        assert_eq!(workspace["nodes"][1]["nodes"][1]["app_id"], "nested-2");
        assert_eq!(
            workspace["nodes"][1]["focus"][0],
            workspace["nodes"][1]["nodes"][1]["id"]
        );
        assert_eq!(workspace["nodes"][1]["nodes"][1]["focused"], true);
    }
}

/// wlroots headless outputs set no make, model or serial, so every one has
/// sway's identifier "Unknown Unknown Unknown" (sway/config/output.c:31-38).
/// A workspace evacuated from HEADLESS-2 gains that identifier in its
/// priority list, and it names HEADLESS-1 first, so re-enabling HEADLESS-2
/// does not take the workspace back (`restore_workspaces`,
/// sway/tree/output.c:31-57). HEADLESS-2 gets a fresh workspace instead.
/// Oracle row: state `output_reenable` (scenario output_config_live_changes).
#[test]
fn reenabled_headless_output_keeps_workspaces_on_the_first_output() {
    let (mut f, socket) = ipc_fixture();
    f.add_named_output_at("headless-1".into(), (1280, 720), Some((0, 0)));
    f.add_named_output_at("headless-2".into(), (1280, 720), Some((1280, 0)));
    let client = f.add_client();
    assert!(crate::command::execute(f.niri_state(), "focus output headless-2")[0].success);
    map_test_window(&mut f, client, "evacuated");

    assert!(crate::command::execute(f.niri_state(), "output headless-2 disable")[0].success);
    assert!(crate::command::execute(f.niri_state(), "output headless-2 enable")[0].success);

    let mut stream = UnixStream::connect(socket).unwrap();
    let outputs = query_ipc(&mut f, &mut stream, MessageType::GetOutputs);
    let current = |name: &str| {
        outputs
            .as_array()
            .unwrap()
            .iter()
            .find(|output| output["name"] == name)
            .map(|output| output["current_workspace"].clone())
            .unwrap()
    };
    let tree = query_ipc(&mut f, &mut stream, MessageType::GetTree);
    let holder = tree["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|output| find_json_node_with_app_id(output, "evacuated").is_some())
        .unwrap();
    assert_eq!(holder["name"], "headless-1");
    assert_eq!(current("headless-1"), "2");
    assert_eq!(current("headless-2"), "1");
}

/// A floating container on a workspace with a fullscreen view: sway moves
/// the floater when the output moves (floating_fix_coordinates) but skips
/// arrange_floating while the workspace is fullscreen
/// (sway/tree/arrange.c:288-321), so the children keep their old global
/// rects. Oracle row: state `output_position` (scenario
/// output_config_live_changes).
#[test]
fn floating_group_children_keep_their_rects_under_fullscreen_output_moves() {
    let (mut f, socket) = ipc_fixture();
    f.add_named_output_at("headless-1".into(), (1280, 720), Some((0, 0)));
    let client = f.add_client();
    // The action sequence of oracle scenario output_config_live_changes.
    map_test_window(&mut f, client, "tiled");
    assert!(crate::command::execute(f.niri_state(), "splith")[0].success);
    map_test_window(&mut f, client, "group-1");
    map_test_window(&mut f, client, "group-2");
    for command in ["focus parent", "floating enable"] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success);
    }
    map_test_window(&mut f, client, "fullscreen");
    assert!(crate::command::execute(f.niri_state(), "fullscreen enable")[0].success);
    f.niri_state().refresh_and_flush_clients();

    let mut stream = UnixStream::connect(socket).unwrap();
    let child_rect = |tree: &Value| {
        let child = find_json_node_with_app_id(tree, "group-1").unwrap();
        (
            child["rect"]["x"].as_i64().unwrap(),
            child["rect"]["y"].as_i64().unwrap(),
        )
    };
    let floater_rect = |tree: &Value| {
        let workspace = find_json_parent_of_app_id(tree, "fullscreen").unwrap();
        let floater = &workspace["floating_nodes"][0];
        (
            floater["rect"]["x"].as_i64().unwrap(),
            floater["rect"]["y"].as_i64().unwrap(),
        )
    };
    let before = query_ipc(&mut f, &mut stream, MessageType::GetTree);

    assert!(
        crate::command::execute(f.niri_state(), "output headless-1 position 200 300")[0].success
    );
    f.niri_state().refresh_and_flush_clients();
    let after = query_ipc(&mut f, &mut stream, MessageType::GetTree);

    let (bx, by) = floater_rect(&before);
    assert_eq!(floater_rect(&after), (bx + 200, by + 300));
    assert_eq!(child_rect(&after), child_rect(&before));
}

#[test]
fn moving_the_active_workspace_away_shows_the_most_recently_focused_one() {
    // Oracle state scenario move_workspace_back_shows_recent_workspace: sway's
    // source output shows its seat focus-inactive workspace afterwards
    // (`output_get_active_workspace`, sway/sway/desktop/output.c:76-86).
    let mut f = Fixture::new();
    f.add_named_output_at("left".into(), (1270, 1408), Some((0, 0)));
    f.add_named_output_at("right".into(), (1270, 1408), Some((1270, 0)));
    let client = f.add_client();
    assert!(crate::command::execute(f.niri_state(), "focus output right")[0].success);
    map_test_window(&mut f, client, "on-two");
    assert!(crate::command::execute(f.niri_state(), "focus output left")[0].success);
    map_test_window(&mut f, client, "on-one");
    for command in [
        "move workspace to output right",
        "workspace oracle",
        "move workspace to output left",
    ] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success);
    }

    let right = f
        .swayward()
        .layout
        .monitors()
        .find(|monitor| monitor.output_name() == "right")
        .unwrap();
    assert_eq!(right.active_workspace_ref().number(), Some(1));
}

/// Sway's `prev_workspace_name` is a string copied when focus leaves a workspace
/// (sway/input/seat.c:1104-1106), so renaming that workspace afterwards leaves it alone and
/// `back_and_forth` creates a workspace under the old name
/// (sway/commands/workspace.c:215-222). Oracle: diff-fam-v3-rename-then-back-and-forth,
/// seed 32729.
#[test]
fn back_and_forth_after_renaming_the_previous_workspace_uses_the_old_name() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();

    assert!(crate::command::execute(f.niri_state(), "workspace oracle")[0].success);
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    for command in [
        "workspace --no-auto-back-and-forth 1",
        "rename workspace oracle to oracle-renamed",
        "workspace back_and_forth",
    ] {
        assert!(
            crate::command::execute(f.niri_state(), command)[0].success,
            "{command}"
        );
    }

    let layout = &f.swayward().layout;
    let active = layout.active_workspace().unwrap();
    assert_eq!(active.sway_name().as_deref(), Some("oracle"));
    assert!(!active.has_windows());
    let renamed = layout
        .workspaces()
        .find(|(_, _, workspace)| workspace.sway_name().as_deref() == Some("oracle-renamed"))
        .map(|(_, _, workspace)| workspace.has_windows());
    assert_eq!(renamed, Some(true));
}
