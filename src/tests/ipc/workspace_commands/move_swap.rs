#[test]
fn criteria_targeted_move_workspace_moves_all_matches_without_changing_focus() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    for app_id in ["special", "special", "ordinary"] {
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

    assert!(crate::command::execute(f.niri_state(), r#"[app_id="ordinary"] focus"#)[0].success);
    let outcome = crate::command::execute(
        f.niri_state(),
        r#"[app_id="special"] move workspace target"#,
    );
    assert!(outcome[0].success, "{outcome:?}");
    let workspaces = f.swayward().layout.workspaces().collect::<Vec<_>>();
    let source = workspaces
        .iter()
        .find(|(_, _, workspace)| workspace.sway_name().as_deref() != Some("target"))
        .unwrap()
        .2;
    let target = workspaces
        .iter()
        .find(|(_, _, workspace)| workspace.sway_name().as_deref() == Some("target"))
        .unwrap()
        .2;
    assert_eq!(source.windows().count(), 1);
    assert_eq!(
        source.active_window().and_then(|window| {
            crate::utils::with_toplevel_role(window.toplevel(), |role| role.app_id.clone())
        }),
        Some("ordinary".into())
    );
    assert_eq!(target.windows().count(), 2);
}

#[test]
fn directional_move_to_output_inserts_before_destination_focus() {
    let mut f = Fixture::new();
    f.add_named_output_at("left".into(), (100, 100), Some((0, 0)));
    f.add_named_output_at("right".into(), (100, 100), Some((100, 0)));
    let client = f.add_client();

    assert!(crate::command::execute(f.niri_state(), "focus output left")[0].success);
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

    assert!(crate::command::execute(f.niri_state(), "move right")[0].success);
    // The move left the seat on the source workspace, which `focus output left` focuses
    // itself, so a plain `move right` is refused there, as pinned sway 1.12 answers
    // (sway/commands/move.c:683-687). Target the remaining view instead.
    let refused = crate::command::execute(f.niri_state(), "focus output left, move right");
    assert!(refused[0].success);
    assert_eq!(
        refused[1].error.as_deref(),
        Some("Cannot move workspaces in a direction")
    );
    assert!(crate::command::execute(f.niri_state(), r#"[app_id="first"] move right"#)[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus output right")[0].success);

    let apps = f
        .swayward()
        .layout
        .active_workspace()
        .unwrap()
        .tiles()
        .filter_map(|tile| {
            crate::utils::with_toplevel_role(tile.window().toplevel(), |role| role.app_id.clone())
        })
        .collect::<Vec<_>>();
    assert_eq!(apps, ["first", "second"]);
}

#[test]
fn criteria_move_workspace_to_output_uses_the_matched_workspace() {
    let mut f = Fixture::new();
    f.add_named_output_at("west".into(), (100, 100), Some((0, 0)));
    f.add_named_output_at("middle".into(), (100, 100), Some((100, 0)));
    f.add_named_output_at("east".into(), (100, 100), Some((200, 0)));
    let client = f.add_client();

    assert!(
        crate::command::execute(f.niri_state(), "focus output middle, workspace target")
            .iter()
            .all(|outcome| outcome.success)
    );
    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("moveme".into());
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    assert!(crate::command::execute(f.niri_state(), "focus output west")[0].success);

    let workspace_output = |f: &mut Fixture| {
        f.swayward()
            .layout
            .workspaces()
            .find(|(_, _, workspace)| workspace.sway_name().as_deref() == Some("target"))
            .and_then(|(monitor, _, _)| monitor.map(|monitor| monitor.output_name().clone()))
            .unwrap()
    };

    let outcome = crate::command::execute(
        f.niri_state(),
        r#"[app_id="moveme"] move workspace to output right"#,
    );
    assert!(outcome[0].success, "{outcome:?}");
    assert_eq!(workspace_output(&mut f), "east");

    let outcome = crate::command::execute(
        f.niri_state(),
        r#"[workspace="target"] move workspace to middle"#,
    );
    assert!(outcome[0].success, "{outcome:?}");
    assert_eq!(workspace_output(&mut f), "middle");

    let outcome = crate::command::execute(
        f.niri_state(),
        r#"[workspace="target"] move workspace to output missing"#,
    );
    assert!(!outcome[0].success);
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("Can't find output with name/direction 'missing'")
    );
    assert_eq!(workspace_output(&mut f), "middle");
    assert_eq!(f.swayward().layout.active_output().unwrap().name(), "west");

    let mut f = Fixture::new();
    f.add_named_output_at("middle".into(), (100, 100), Some((100, 0)));
    f.add_named_output_at("east".into(), (100, 100), Some((200, 0)));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    let floating = f.swayward().layout.focus().unwrap().window.clone();
    assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);
    f.swayward().layout.move_floating_window(
        Some(&floating),
        swayward_ipc::legacy::PositionChange::AdjustFixed(20.),
        swayward_ipc::legacy::PositionChange::AdjustFixed(10.),
        false,
    );
    let old_center = f.swayward().layout.window_center(&floating).unwrap();

    assert!(crate::command::execute(f.niri_state(), "move workspace to output right")[0].success);
    assert_eq!(f.swayward().layout.active_output().unwrap().name(), "east");
    let new_center = f.swayward().layout.window_center(&floating).unwrap();
    assert_eq!(new_center.x - old_center.x, 100);
    assert_eq!(new_center.y, old_center.y);
    let wrapped = crate::command::execute(f.niri_state(), "move workspace to output right");
    assert!(wrapped[0].success, "{wrapped:?}");
    assert_eq!(
        f.swayward().layout.active_output().unwrap().name(),
        "middle"
    );
}

#[test]
fn criteria_move_workspace_ignores_hidden_scratchpad_matches() {
    let mut f = Fixture::new();
    f.add_named_output_at("fake-0".into(), (100, 100), Some((0, 0)));
    f.add_named_output_at("fake-1".into(), (100, 100), Some((100, 0)));
    let client = f.add_client();

    for (output, workspace, app_id, scratchpad) in [
        ("fake-0", "ws0", "a", false),
        ("fake-1", "ws1", "b", false),
        ("fake-1", "ws1", "c", true),
    ] {
        let command = format!("focus output {output}, workspace {workspace}");
        assert!(crate::command::execute(f.niri_state(), &command)
            .iter()
            .all(|outcome| outcome.success));
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(app_id.into());
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        if scratchpad {
            assert!(crate::command::execute(f.niri_state(), "move to scratchpad")[0].success);
        }
    }

    let outcome = crate::command::execute(
        f.niri_state(),
        r#"[app_id=".*"] move workspace to output fake-1"#,
    );
    assert!(outcome[0].success, "{outcome:?}");
    let swayward = f.swayward();
    assert!(
        describe_workspaces(&swayward.layout, &swayward.global_space)
            .iter()
            .any(|workspace| workspace.name == "ws0" && workspace.output == "fake-1")
    );
}

#[test]
fn cross_workspace_swap_exchanges_positions_marks_and_fullscreen() {
    let mut f = Fixture::new();
    f.add_output_at(1, (600, 800), Some((0, 0)));
    f.add_output_at(2, (1000, 800), Some((600, 0)));
    let client = f.add_client();

    let mut windows = Vec::new();
    for (output, workspace, mark, fullscreen) in [
        ("headless-1", "one", "A", true),
        ("headless-2", "two", "B", false),
    ] {
        assert!(crate::command::execute(
            f.niri_state(),
            &format!("focus output {output}, workspace {workspace}")
        )
        .iter()
        .all(|outcome| outcome.success));
        let window = f.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        let mapped = f.swayward().layout.focus().unwrap();
        windows.push((workspace, mapped.id(), mapped.window.clone()));
        assert!(crate::command::execute(f.niri_state(), &format!("mark {mark}"))[0].success);
        if fullscreen {
            assert!(crate::command::execute(f.niri_state(), "fullscreen enable")[0].success);
        }
    }

    let result = crate::command::execute(f.niri_state(), "[con_mark=B] swap container with mark A");
    assert!(result[0].success, "{result:?}");
    let (one_id, two_id) = {
        let layout = &f.swayward().layout;
        let one = layout
            .workspaces()
            .find(|(_, _, workspace)| workspace.sway_name().as_deref() == Some("one"))
            .unwrap()
            .2
            .id();
        let two = layout
            .workspaces()
            .find(|(_, _, workspace)| workspace.sway_name().as_deref() == Some("two"))
            .unwrap()
            .2
            .id();
        (one, two)
    };
    assert_eq!(
        f.swayward().layout.window_workspace_id(&windows[0].2),
        Some(two_id)
    );
    assert_eq!(
        f.swayward().layout.window_workspace_id(&windows[1].2),
        Some(one_id)
    );
    for workspace in [one_id, two_id] {
        let tree = f
            .swayward()
            .layout
            .workspaces()
            .find(|(_, _, candidate)| candidate.id() == workspace)
            .unwrap()
            .2
            .ipc_tiling_tree();
        assert_eq!(tree.nodes().len(), 2);
    }
    let first_center = f.swayward().layout.window_center(&windows[0].2).unwrap();
    let second_center = f.swayward().layout.window_center(&windows[1].2).unwrap();
    assert!(first_center.x >= 600, "{first_center:?}");
    assert!(second_center.x < 600, "{second_center:?}");
    assert_eq!(f.swayward().layout.fullscreen_mode(&windows[0].2), None);
    assert_eq!(
        f.swayward().layout.fullscreen_mode(&windows[1].2),
        Some(crate::layout::tiling_tree::FullscreenMode::Workspace)
    );
    assert!(f
        .swayward()
        .marks_by_window
        .get(&windows[0].1)
        .is_some_and(|marks| marks.as_slice() == ["A"]));
    assert!(f
        .swayward()
        .marks_by_window
        .get(&windows[1].1)
        .is_some_and(|marks| marks.as_slice() == ["B"]));
    assert_eq!(
        f.swayward()
            .marks_by_window
            .values()
            .flatten()
            .filter(|mark| *mark == "A" || *mark == "B")
            .count(),
        2
    );
}

// random seed 274 step 10 (sway-1.12-random): moving a window out of a
// fullscreen split container moves only the window. The container stays
// behind and is reaped, so the moved window does not arrive fullscreen
// (`container_move_to_workspace`, sway/commands/move.c:220-229).
#[test]
fn moving_a_window_out_of_a_fullscreen_container_leaves_fullscreen_behind() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    for app_id in ["moved", "other"] {
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
        "splitv",
        "splith",
        "focus left",
        "fullscreen toggle",
        "splith",
        "move left",
        "splith",
        "move container to workspace 2",
    ] {
        assert!(
            crate::command::execute(f.niri_state(), command)[0].success,
            "{command}"
        );
    }

    let swayward = f.swayward();
    let tree = serde_json::to_value(crate::ipc::tree::describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    assert_eq!(
        find_json_node_with_app_id(&tree, "moved").unwrap()["fullscreen_mode"],
        0
    );
}

// random seed 105 step 17 (sway-1.12-random): a floating fullscreen window
// moved to another workspace stays a floating container there
// (`container_move_to_workspace`, sway/commands/move.c:203-219).
#[test]
fn moving_a_floating_fullscreen_window_keeps_it_floating() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    for (workspace, app_id) in [("2", "tiled"), ("1", "floating")] {
        assert!(
            crate::command::execute(f.niri_state(), &format!("workspace {workspace}"))[0].success
        );
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
    assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);
    assert!(crate::command::execute(f.niri_state(), "fullscreen enable")[0].success);

    assert!(crate::command::execute(f.niri_state(), "move container to workspace 2")[0].success);

    let swayward = f.swayward();
    let tree = serde_json::to_value(crate::ipc::tree::describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let moved = find_json_node_with_app_id(&tree, "floating").unwrap();
    assert_eq!(moved["type"], "floating_con");
    assert_eq!(moved["fullscreen_mode"], 1);
}

#[test]
fn live_ipc_move_to_an_empty_workspace_preserves_the_container_layout() {
    let (mut f, socket) = ipc_fixture();
    f.add_output(1, (1270, 1408));
    let client = f.add_client();
    assert!(crate::command::execute(f.niri_state(), "splith")[0].success);
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
    f.swayward().layout.nest_or_unnest_window_left(None);
    let mut stream = UnixStream::connect(socket).unwrap();
    for command in [
        "focus parent",
        "mark group",
        "workspace target",
        "[con_mark=group] move workspace target",
    ] {
        let outcome = query_ipc_with_payload(&mut f, &mut stream, MessageType::RunCommand, command);
        assert_eq!(outcome[0]["success"], true, "{command}: {outcome}");
    }
    let tree = query_ipc(&mut f, &mut stream, MessageType::GetTree);
    let workspaces = tree["nodes"][1]["nodes"].as_array().unwrap();
    let target = workspaces
        .iter()
        .find(|workspace| workspace["name"] == "target")
        .unwrap();
    assert_eq!(target["layout"], "splitv");
    assert_eq!(target["representation"], "V[second first]");
    assert_eq!(target["nodes"].as_array().unwrap().len(), 2);
    let app_ids = target["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| node["app_id"].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(app_ids, ["first", "second"].into());
}

// random seed 61 step 10 (sway-1.12-random): with the workspace focused,
// sway wraps its tiling children before resolving the destination, so a
// move to the same workspace still leaves one wrapper container.
#[test]
fn moving_a_workspace_to_itself_still_wraps_its_children() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    for app_id in ["first", "second", "third"] {
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

    let outcome = crate::command::execute(f.niri_state(), "move container to workspace 1");
    assert!(outcome[0].success, "{outcome:?}");

    let swayward = f.swayward();
    let tree = serde_json::to_value(crate::ipc::tree::describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    let workspace = &tree["nodes"][1]["nodes"][0];
    assert_eq!(workspace["nodes"].as_array().unwrap().len(), 1);
    assert_eq!(workspace["nodes"][0]["nodes"].as_array().unwrap().len(), 3);
}

// differential seed 15111 (random-v2): sway wraps a focused workspace's
// tiling children before it resolves the destination, so a move to a missing
// mark or output fails but leaves the wrapper (sway/commands/move.c:430-436),
// unarranged. Oracle row workspace_move_to_missing_mark_or_output_wraps_children.
#[test]
fn failed_workspace_move_to_missing_mark_or_output_still_wraps_children() {
    for command in [
        "move container to mark missing",
        "move container to output MISSING",
    ] {
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
        assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);

        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(!outcome[0].success, "{command}: {outcome:?}");

        let swayward = f.swayward();
        let tree = serde_json::to_value(crate::ipc::tree::describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &Default::default(),
            &Default::default(),
        ))
        .unwrap();
        let workspace = &tree["nodes"][1]["nodes"][0];
        assert_eq!(workspace["representation"], "H[H[(null)]]", "{command}");
        assert_eq!(workspace["nodes"].as_array().unwrap().len(), 1, "{command}");
        assert_eq!(
            workspace["nodes"][0]["nodes"].as_array().unwrap().len(),
            1,
            "{command}"
        );
        // Sway returns before arranging, so the wrapper keeps calloc's empty
        // box: percent 0 over the workspace, none for the view below it.
        let wrapper = &workspace["nodes"][0];
        assert_eq!(wrapper["percent"], 0.0, "{command}");
        assert_eq!(wrapper["rect"]["width"], 0, "{command}");
        assert!(wrapper["nodes"][0]["percent"].is_null(), "{command}");

        // `workspace X` runs `arrange_workspace`, which gives the wrapper its box
        // (sway/tree/workspace.c:741, sway/tree/arrange.c:317-321).
        let name = workspace["name"].as_str().unwrap().to_owned();
        let switch = format!("workspace --no-auto-back-and-forth {name}");
        assert!(crate::command::execute(f.niri_state(), &switch)[0].success);
        let swayward = f.swayward();
        let tree = serde_json::to_value(crate::ipc::tree::describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &Default::default(),
            &Default::default(),
        ))
        .unwrap();
        let wrapper = &tree["nodes"][1]["nodes"][0]["nodes"][0];
        assert_eq!(wrapper["percent"], 1.0, "{command}");
        assert_eq!(wrapper["nodes"][0]["percent"], 1.0, "{command}");
    }
}

#[test]
fn criteria_targeted_move_workspace_preserves_a_container_subtree() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    assert!(crate::command::execute(f.niri_state(), "workspace source")[0].success);
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
    f.swayward().layout.nest_or_unnest_window_left(None);
    assert!(crate::command::execute(f.niri_state(), r#"[app_id="first"] focus"#)[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
    assert!(crate::command::execute(f.niri_state(), "mark group")[0].success);
    assert!(crate::command::execute(f.niri_state(), "workspace target")[0].success);
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    let outcome = crate::command::execute(f.niri_state(), "[con_mark=group] move workspace target");
    assert!(outcome[0].success, "{outcome:?}");
    let workspaces = f.swayward().layout.workspaces().collect::<Vec<_>>();
    let source = workspaces
        .iter()
        .find(|(_, _, workspace)| workspace.sway_name().as_deref() == Some("source"))
        .unwrap()
        .2;
    let target = workspaces
        .iter()
        .find(|(_, _, workspace)| workspace.sway_name().as_deref() == Some("target"))
        .unwrap()
        .2;
    assert_eq!(source.windows().count(), 0);
    assert_eq!(target.windows().count(), 3);
    assert_eq!(target.ipc_tiling_tree().nodes().len(), 5);
    assert!(target.windows().any(|window| {
        crate::utils::with_toplevel_role(window.toplevel(), |role| role.app_id.clone())
            == Some("first".into())
    }));
}

#[test]
fn criteria_targeted_scratchpad_show_toggles_every_matching_window() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    for _ in 0..2 {
        let window = f.client(client).create_window();
        window.set_title("toggle-window");
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);
    }

    for expected_hidden in [0, 2, 0] {
        let outcome =
            crate::command::execute(f.niri_state(), r#"[title="toggle-"] scratchpad show"#);
        assert!(outcome[0].success, "{outcome:?}");
        assert_eq!(
            f.swayward().layout.scratchpad_windows().count(),
            expected_hidden
        );
    }
    for expected_hidden in [1, 2] {
        assert!(crate::command::execute(f.niri_state(), "scratchpad show")[0].success);
        assert_eq!(
            f.swayward().layout.scratchpad_windows().count(),
            expected_hidden
        );
    }
}

#[test]
fn criteria_targeted_scratchpad_show_toggles_each_match_from_its_own_state() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let mut windows = Vec::new();
    for title in ["mixed-toggle-1", "mixed-toggle-2"] {
        let window = f.client(client).create_window();
        window.set_title(title);
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        let mapped = f
            .swayward()
            .layout
            .active_workspace()
            .unwrap()
            .active_window()
            .unwrap();
        windows.push((mapped.window.clone(), mapped.id()));
        assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);
    }

    let [(first, first_id), (second, _)] = windows.as_slice() else {
        unreachable!()
    };
    let first_id = crate::ipc::tree::window_id(*first_id);
    assert!(
        crate::command::execute(
            f.niri_state(),
            &format!("[con_id={first_id}] scratchpad show")
        )[0]
        .success
    );
    assert!(!f.swayward().layout.is_scratchpad_hidden(first));
    assert!(f.swayward().layout.is_scratchpad_hidden(second));

    let outcome =
        crate::command::execute(f.niri_state(), r#"[title="mixed-toggle-"] scratchpad show"#);
    assert!(outcome[0].success, "{outcome:?}");
    assert!(f.swayward().layout.is_scratchpad_hidden(first));
    assert!(!f.swayward().layout.is_scratchpad_hidden(second));
}

#[test]
fn criteria_targeted_scratchpad_commands_move_only_the_matching_window() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    for app_id in ["ordinary", "special"] {
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
        crate::command::execute(f.niri_state(), r#"[app_id="special"] move scratchpad"#)[0].success
    );
    assert_eq!(f.swayward().layout.scratchpad_windows().count(), 1);
    let ordinary = f
        .swayward()
        .layout
        .active_workspace()
        .unwrap()
        .active_window()
        .unwrap();
    assert_eq!(
        crate::utils::with_toplevel_role(ordinary.toplevel(), |role| role.app_id.clone()),
        Some("ordinary".into())
    );
    assert!(
        crate::command::execute(f.niri_state(), r#"[app_id="special"] scratchpad show"#)[0].success
    );
    let workspace = f.swayward().layout.active_workspace().unwrap();
    assert_eq!(workspace.windows().count(), 2);
    let active = workspace.active_window().unwrap();
    assert_eq!(
        crate::utils::with_toplevel_role(active.toplevel(), |role| role.app_id.clone()),
        Some("special".into())
    );
}

fn dialog_rect_after_parent_move(animations_off: bool) -> Value {
    let mut config = swayward_config::Config::default();
    config.animations.off = animations_off;
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let first = f.client(client).create_window();
    first.commit();
    let first_surface = first.surface.clone();
    f.roundtrip(client);
    let first = f.client(client).window(&first_surface);
    first.attach_new_buffer();
    first.ack_last_and_commit();
    f.double_roundtrip(client);

    let parent = f.client(client).create_window();
    parent.xdg_toplevel.set_app_id("parent".into());
    let parent_surface = parent.surface.clone();
    let parent_toplevel = parent.xdg_toplevel.clone();
    parent.commit();
    f.roundtrip(client);
    let parent = f.client(client).window(&parent_surface);
    parent.attach_new_buffer();
    parent.ack_last_and_commit();
    f.double_roundtrip(client);
    assert!(crate::command::execute(f.niri_state(), "move left")[0].success);
    if !animations_off {
        assert!(f.swayward().layout.are_animations_ongoing(None));
    }

    let child = f.client(client).create_window();
    child.xdg_toplevel.set_app_id("dialog".into());
    child.set_parent(Some(&parent_toplevel));
    let child_surface = child.surface.clone();
    child.commit();
    f.roundtrip(client);
    let child = f.client(client).window(&child_surface);
    child.attach_new_buffer();
    child.ack_last_and_commit();
    f.double_roundtrip(client);

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    fn find_app(value: &Value) -> Option<&Value> {
        if value["app_id"] == "dialog" {
            return Some(value);
        }
        ["nodes", "floating_nodes"]
            .into_iter()
            .find_map(|key| value[key].as_array()?.iter().find_map(find_app))
    }
    find_app(&tree).unwrap()["rect"].clone()
}

fn fullscreen_parent_after_child_map(
    policy: swayward_config::PopupDuringFullscreen,
) -> (bool, bool) {
    let mut config = swayward_config::Config {
        popup_during_fullscreen: policy,
        ..Default::default()
    };
    config.animations.off = true;
    let mut f = Fixture::with_config(config);
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
    let parent_id = f.swayward().layout.focus().unwrap().id();
    let parent_window = f.swayward().layout.focus().unwrap().window.clone();
    f.swayward().layout.set_fullscreen(&parent_window, true);

    let child = f.client(client).create_window();
    child.set_parent(Some(&parent_toplevel));
    let child_surface = child.surface.clone();
    child.commit();
    f.roundtrip(client);
    let child = f.client(client).window(&child_surface);
    child.attach_new_buffer();
    child.ack_last_and_commit();
    f.double_roundtrip(client);

    (
        f.swayward()
            .layout
            .fullscreen_mode(&parent_window)
            .is_some(),
        f.swayward().layout.focus().unwrap().id() != parent_id,
    )
}

// sway/tree/container.c:977-980 removes the container from the scratchpad
// before returning it to tiling. root_scratchpad_remove_container emits
// `move` (sway/tree/root.c:150-154) and container_set_floating then emits
// `floating` (sway/tree/container.c:1015).
#[test]
fn unfloating_a_scratchpad_window_emits_move_then_floating() {
    let (mut f, socket) = ipc_fixture();
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
    let id = f.swayward().layout.focus().unwrap().id();

    assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);
    assert!(crate::command::execute(f.niri_state(), "scratchpad show")[0].success);
    f.niri_state().refresh_and_flush_clients();

    let mut subscriber = UnixStream::connect(socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["window"]"#,
        ))
        .unwrap();
    let _ = read_ipc_reply(&mut f, &mut subscriber);

    assert!(crate::command::execute(f.niri_state(), "floating disable")[0].success);
    let mut events = Vec::new();
    let mut remainder = Vec::new();
    for _ in 0..2 {
        let ((event_type, payload), rest) =
            read_ipc_reply_with_remainder(&mut f, &mut subscriber, remainder);
        remainder = rest;
        assert_eq!(event_type, EVENT_WINDOW);
        events.push(serde_json::from_str::<Value>(&payload).unwrap());
    }
    for event in &events {
        assert_eq!(event["container"]["id"], crate::ipc::tree::window_id(id));
        assert_eq!(event["container"]["type"], "con");
        assert_eq!(event["container"]["scratchpad_state"], "none");
    }
    assert_eq!(events[0]["change"], "move");
    assert_eq!(events[1]["change"], "floating");
}

#[test]
fn floating_toggle_after_moving_scratchpad_window_between_workspaces_does_not_panic() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    for _ in 0..5 {
        windows::map_window(&mut f, client, windows::WindowSpec::default());
    }

    for (command, expected_success) in [
        ("focus parent", true),
        ("move scratchpad", true),
        ("scratchpad show", true),
        ("move container to workspace 2", true),
        // `scratchpad show` focuses the group's most recently focused view
        // (sway/tree/root.c:185-186), so the moved view leaves its sibling
        // focused: pinned sway answers success to both of these.
        ("floating toggle", true),
        ("move container to workspace 2", true),
        ("workspace 2", true),
        ("floating toggle", true),
    ] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert_eq!(
            outcome[0].success, expected_success,
            "{command}: {outcome:?}"
        );
        f.swayward().layout.verify_invariants();
    }
}

#[test]
fn moving_a_window_away_refocuses_the_most_recent_container_under_its_parent() {
    // Oracle random seeds 217 step 8 and 124 step 8: sway refocuses
    // `seat_get_focus_inactive(old_parent)` after a move (sway/commands/move.c:598-608),
    // and its focus stack holds a split whenever a descendant was focused
    // (sway/input/seat.c:1178-1190), so a vacated split can be refocused as a container.
    // Seed 474 step 15: when the move reaps a container, sway raises the most recent view
    // under its parent instead (sway/input/seat.c:273-323).
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    let map = |f: &mut Fixture, app_id: &str| {
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(app_id.into());
        let surface = window.surface.clone();
        window.commit();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    };
    let focused = |f: &mut Fixture| {
        let swayward = f.swayward();
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &Default::default(),
            &Default::default(),
        ))
        .unwrap();
        let node = find_json_node(&tree, "con", true).unwrap();
        (node["layout"].clone(), node["app_id"].clone())
    };

    map(&mut f, "first");
    map(&mut f, "second");
    for command in ["layout splitv", "focus up", "move right"] {
        assert!(
            crate::command::execute(f.niri_state(), command)[0].success,
            "{command}"
        );
    }
    assert!(crate::command::execute(f.niri_state(), "move container to workspace 2")[0].success);
    assert_eq!(
        focused(&mut f),
        (serde_json::json!("splitv"), serde_json::Value::Null),
        "the vacated split was focused through its window, so sway refocuses the split"
    );
}

// random seed 230 step 15 (sway-1.12-random): a window moved onto a workspace
// whose child is fullscreen keeps its configured border and titlebar.
// `container_move_to_workspace` (sway/commands/move.c:220-229) zeroes its
// size, and `arrange_workspace` lays out only the fullscreen container
// (sway/tree/arrange.c:310-316), so GET_TREE reports a zero-width box below
// the titlebar rather than the border-less placeholder of a freshly mapped
// window.
#[test]
fn moving_a_window_under_fullscreen_keeps_its_border() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    for (workspace, app_id) in [("2", "fullscreen"), ("1", "moved")] {
        assert!(
            crate::command::execute(f.niri_state(), &format!("workspace {workspace}"))[0].success
        );
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(app_id.into());
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        if app_id == "fullscreen" {
            assert!(crate::command::execute(f.niri_state(), "fullscreen enable")[0].success);
        }
    }

    assert!(crate::command::execute(f.niri_state(), "move container to workspace 2")[0].success);

    let swayward = f.swayward();
    let tree = serde_json::to_value(crate::ipc::tree::describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let moved = find_json_node_with_app_id(&tree, "moved").unwrap();
    let fullscreen = find_json_node_with_app_id(&tree, "fullscreen").unwrap();
    assert_eq!(moved["border"], "normal");
    assert_eq!(
        moved["current_border_width"],
        fullscreen["current_border_width"]
    );
    assert_eq!(moved["percent"], 0.0);
    assert_eq!(fullscreen["percent"], 1.0);
    let titlebar = moved["deco_rect"]["height"].as_i64().unwrap();
    assert!(titlebar > 0);
    assert_eq!(moved["rect"]["width"], 0);
    assert_eq!(moved["rect"]["height"], -titlebar);
    // `workspace_focus_fullscreen` raises the fullscreen view above the moved one.
    let workspace = tree["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|output| output["nodes"].as_array().unwrap())
        .find(|workspace| workspace["name"] == "2")
        .unwrap();
    assert_eq!(workspace["focus"][0], fullscreen["id"]);
}

// random seed 253 step 13 (sway-1.12-random): the destination workspace's
// focus-inactive container is a tabbed split, so sway adds the moved window
// as its child (`container_move_to_container`,
// sway/commands/move.c:241-262), not beside it at workspace level.
#[test]
fn moving_a_window_onto_a_focused_split_joins_that_split() {
    let (mut f, socket) = ipc_fixture();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    let mut stream = UnixStream::connect(socket).unwrap();
    let map = |f: &mut Fixture, app_id: &str| {
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
    map(&mut f, "first");
    map(&mut f, "second");
    for command in ["layout tabbed", "focus parent", "workspace other"] {
        let outcome = query_ipc_with_payload(&mut f, &mut stream, MessageType::RunCommand, command);
        assert_eq!(outcome[0]["success"], true, "{command}: {outcome}");
    }
    map(&mut f, "third");
    let outcome = query_ipc_with_payload(
        &mut f,
        &mut stream,
        MessageType::RunCommand,
        "move container to workspace 1",
    );
    assert_eq!(outcome[0]["success"], true, "{outcome}");
    let tree = query_ipc(&mut f, &mut stream, MessageType::GetTree);
    let workspaces = tree["nodes"][1]["nodes"].as_array().unwrap();
    let target = workspaces
        .iter()
        .find(|workspace| workspace["name"] == "1")
        .unwrap();
    assert_eq!(
        target["representation"], "H[T[first second third]]",
        "{target}"
    );
}

// random seed 368 step 5 (sway-1.12-random): `move down` from H[a b*] wraps
// the workspace into V[H[a] b]. The new H wrapper was never focused, so it
// joins the tail of sway's focus stack (`seat_node_from_node`,
// sway/input/seat.c:327-349) and the workspace reports b first, the
// floating window next, and the wrapper last.
#[test]
fn a_reorienting_move_lists_the_new_wrapper_last_in_focus() {
    let (mut f, socket) = ipc_fixture();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    let mut stream = UnixStream::connect(socket).unwrap();
    let map = |f: &mut Fixture, app_id: &str| {
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
    map(&mut f, "floater");
    let outcome = query_ipc_with_payload(
        &mut f,
        &mut stream,
        MessageType::RunCommand,
        "floating toggle",
    );
    assert_eq!(outcome[0]["success"], true, "{outcome}");
    map(&mut f, "a");
    map(&mut f, "b");
    let outcome = query_ipc_with_payload(&mut f, &mut stream, MessageType::RunCommand, "move down");
    assert_eq!(outcome[0]["success"], true, "{outcome}");

    let tree = query_ipc(&mut f, &mut stream, MessageType::GetTree);
    let workspace = &tree["nodes"][1]["nodes"][0];
    let wrapper = workspace["nodes"][0]["id"].clone();
    let b = workspace["nodes"][1]["id"].clone();
    let floater = workspace["floating_nodes"][0]["id"].clone();
    assert_eq!(
        workspace["focus"],
        serde_json::json!([b, floater, wrapper]),
        "{workspace}"
    );
}

// random seed 197 step 12 (sway-1.12-random): a tabbed container in the
// top half of a V workspace measures its child's percent against its own
// box (`ipc_json_describe_container`, sway/ipc-json.c:744-754). The tab bar
// takes one titlebar from the top of that box, so the split below it
// reports (360 - titlebar) / 360 (0.925 with sway's 27px titlebar), not the
// workspace-relative (720 - titlebar) / 720 (0.9625).
#[test]
fn a_tabbed_childs_percent_uses_the_tabbed_containers_own_box() {
    let (mut f, socket) = ipc_fixture();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    let mut stream = UnixStream::connect(socket).unwrap();
    let map = |f: &mut Fixture, app_id: &str| {
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
    map(&mut f, "a");
    for command in ["splitv", "layout tabbed"] {
        let outcome = query_ipc_with_payload(&mut f, &mut stream, MessageType::RunCommand, command);
        assert_eq!(outcome[0]["success"], true, "{command}: {outcome}");
    }
    map(&mut f, "b");
    for command in ["splitv", "focus parent", "focus parent", "splith"] {
        let outcome = query_ipc_with_payload(&mut f, &mut stream, MessageType::RunCommand, command);
        assert_eq!(outcome[0]["success"], true, "{command}: {outcome}");
    }
    map(&mut f, "c");
    let outcome = query_ipc_with_payload(&mut f, &mut stream, MessageType::RunCommand, "move down");
    assert_eq!(outcome[0]["success"], true, "{outcome}");

    let tree = query_ipc(&mut f, &mut stream, MessageType::GetTree);
    let workspace = &tree["nodes"][1]["nodes"][0];
    let representation = workspace["representation"].as_str().unwrap();
    let tabbed = &workspace["nodes"][0]["nodes"][0];
    assert_eq!(tabbed["layout"], "tabbed", "{representation}");
    let split = tabbed["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["layout"] == "splitv")
        .unwrap_or_else(|| panic!("{representation}"));
    // The split's rect starts below the tab bar and its own titlebar, as in
    // sway's capture (y = 2 * 27); only the tab bar comes out of the percent.
    let titlebar =
        (split["rect"]["y"].as_f64().unwrap() - tabbed["rect"]["y"].as_f64().unwrap()) / 2.;
    assert!(titlebar > 0., "{representation}");
    let box_height = tabbed["rect"]["height"].as_f64().unwrap();
    let percent = split["percent"].as_f64().unwrap();
    assert!(
        (percent - (box_height - titlebar) / box_height).abs() < 1e-9,
        "{percent} in {representation}"
    );
}

// random seed 183 step 17 (sway-1.12-random): in H[1 H[2 H[3*]]], 3 floats
// and leaves H[1 H[2]]; 5 maps beside 2 and `move right` takes it out of
// the split. The split keeps the place focus gave it when it entered 5
// (`seat_set_raw_focus`, sway/input/seat.c), so the workspace focus list is
// [5, split, 3, 1], not [5, 3, split, 1] ranked by 2's older focus.
#[test]
fn a_container_keeps_its_focus_place_after_the_focused_view_leaves() {
    let (mut f, socket) = ipc_fixture();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    let mut stream = UnixStream::connect(socket).unwrap();
    let map = |f: &mut Fixture, app_id: &str| {
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
    let run = |f: &mut Fixture, stream: &mut UnixStream, command: &str| {
        let outcome = query_ipc_with_payload(f, stream, MessageType::RunCommand, command);
        assert_eq!(outcome[0]["success"], true, "{command}: {outcome}");
    };
    map(&mut f, "1");
    map(&mut f, "2");
    run(&mut f, &mut stream, "splith");
    map(&mut f, "3");
    run(&mut f, &mut stream, "splith");
    run(&mut f, &mut stream, "floating toggle");
    map(&mut f, "5");
    run(&mut f, &mut stream, "move right");

    let tree = query_ipc(&mut f, &mut stream, MessageType::GetTree);
    let workspace = &tree["nodes"][1]["nodes"][0];
    let name = |id: &serde_json::Value| {
        workspace["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .chain(workspace["floating_nodes"].as_array().unwrap())
            .find(|node| node["id"] == *id)
            .map(|node| node["app_id"].as_str().unwrap_or("split").to_owned())
            .unwrap()
    };
    let order: Vec<_> = workspace["focus"]
        .as_array()
        .unwrap()
        .iter()
        .map(name)
        .collect();
    assert_eq!(
        order,
        ["5", "split", "3", "1"],
        "{}",
        workspace["representation"]
    );
}

// random seed 15 step 15 (sway-1.12-random): window 1 moves to workspace
// 2, then `focus parent` up to workspace 1 and `move container to workspace
// 2` moves the whole workspace. Its children were focused after window 1,
// so the moved wrapper ranks first on workspace 2 even though window 1 sits
// after it in tree order, which once pulled the wrapper behind window 1.
#[test]
fn a_moved_workspace_ranks_by_its_own_focus_not_tree_order() {
    let (mut f, socket) = ipc_fixture();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    let mut stream = UnixStream::connect(socket).unwrap();
    let map = |f: &mut Fixture, app_id: &str| {
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
    let run = |f: &mut Fixture, stream: &mut UnixStream, command: &str| {
        let outcome = query_ipc_with_payload(f, stream, MessageType::RunCommand, command);
        assert_eq!(outcome[0]["success"], true, "{command}: {outcome}");
    };
    // The seed's commands, minus a no-op `sticky toggle` on a tiled window.
    map(&mut f, "1");
    run(&mut f, &mut stream, "splith");
    run(&mut f, &mut stream, "splitv");
    map(&mut f, "2");
    run(&mut f, &mut stream, "splith");
    map(&mut f, "3");
    for command in [
        "layout tabbed",
        "focus child",
        "focus parent",
        "focus child",
        "focus up",
        "move container to workspace 2",
        "focus parent",
        "move container to workspace 2",
    ] {
        query_ipc_with_payload(&mut f, &mut stream, MessageType::RunCommand, command);
    }

    let tree = query_ipc(&mut f, &mut stream, MessageType::GetTree);
    let target = tree["nodes"][1]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|workspace| workspace["name"] == "2")
        .unwrap();
    let kind = |id: &serde_json::Value| {
        target["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|node| node["id"] == *id)
            .map(|node| node["app_id"].as_str().unwrap_or("wrapper").to_owned())
            .unwrap()
    };
    let order: Vec<_> = target["focus"]
        .as_array()
        .unwrap()
        .iter()
        .map(kind)
        .collect();
    assert_eq!(order, ["wrapper", "1"], "{}", target["representation"]);
}

/// Sway moves each criteria match after the destination's focus-inactive tiling
/// container, which the previous match becomes once it is newer on the seat's focus
/// stack (`seat_get_focus_inactive_tiling`, sway/input/seat.c:1374-1389;
/// sway/commands/move.c:515). The matches keep their order. Oracle row:
/// `criteria_move_to_workspace_keeps_match_order`.
#[test]
fn criteria_move_to_workspace_keeps_match_order() {
    let mut f = Fixture::new();
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
    assert!(crate::command::execute(f.niri_state(), "workspace 2")[0].success);
    open(&mut f, "two");
    open(&mut f, "three");

    let outcome = crate::command::execute(
        f.niri_state(),
        r#"[workspace="2"] move container to workspace 1"#,
    );
    assert!(outcome[0].success, "{outcome:?}");

    let target = f
        .swayward()
        .layout
        .workspaces()
        .find(|(_, _, workspace)| workspace.sway_name().as_deref() == Some("1"))
        .unwrap()
        .2;
    let apps = target
        .tiles()
        .filter_map(|tile| {
            crate::utils::with_toplevel_role(tile.window().toplevel(), |role| role.app_id.clone())
        })
        .collect::<Vec<_>>();
    assert_eq!(apps, ["one", "two", "three"]);
}

fn map_window(f: &mut Fixture, client: crate::tests::client::ClientId, app_id: &str) {
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

// Differential family diff-fam-move-baf-no-history (random-v2 seed 10023):
// without history sway refuses with CMD_FAILURE "No workspace was previously
// active." (sway/commands/move.c:460-468), not a parse error.
#[test]
fn move_to_workspace_back_and_forth_without_history_uses_sway_error() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    map_window(&mut f, client, "moved");

    let outcome =
        crate::command::execute(f.niri_state(), "move container to workspace back_and_forth");
    assert!(!outcome[0].success);
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("No workspace was previously active.")
    );
    assert_eq!(outcome[0].parse_error, Some(false));
}

// With a previous workspace that was reaped while empty, sway recreates it
// by name (sway/commands/move.c:462-463).
#[test]
fn move_to_workspace_back_and_forth_recreates_a_reaped_previous_workspace() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    for workspace in ["alpha", "1"] {
        assert!(
            crate::command::execute(f.niri_state(), &format!("workspace {workspace}"))[0].success
        );
    }
    map_window(&mut f, client, "moved");

    let outcome =
        crate::command::execute(f.niri_state(), "move container to workspace back_and_forth");
    assert!(outcome[0].success, "{outcome:?}");

    let swayward = f.swayward();
    let tree = serde_json::to_value(crate::ipc::tree::describe_tree(
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
        .find(|workspace| workspace["name"] == "alpha")
        .expect("previous workspace recreated");
    assert!(find_json_node_with_app_id(workspace, "moved").is_some());
}

// Differential family diff-fam-v3-move-to-mark-focus (random-v3 seeds 40058,
// 40239, 40373). Moving a container never raises it in the seat stack; only
// a moved focused container refocuses the most recent entry under its old
// parent, else the workspace, and refocusing the current focus is a no-op
// (sway/commands/move.c:589-608; sway/input/seat.c:1131-1134). Oracle rows
// move_to_mark_in_split_focuses_moved_first and
// move_to_mark_from_split_focuses_old_sibling.
fn mark_move_workspace(f: &mut Fixture) -> serde_json::Value {
    f.niri_state().refresh_and_flush_clients();
    let swayward = f.swayward();
    let tree = serde_json::to_value(crate::ipc::tree::describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    tree["nodes"][1]["nodes"][0].clone()
}

fn focus_apps(container: &serde_json::Value) -> Vec<String> {
    let children = container["nodes"].as_array().unwrap();
    container["focus"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| {
            let child = children.iter().find(|child| child["id"] == *id).unwrap();
            child["app_id"].as_str().unwrap_or("split").to_owned()
        })
        .collect()
}

#[test]
fn move_to_mark_in_split_keeps_the_moved_view_first_in_focus() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    map_window(&mut f, client, "mtm-1");
    map_window(&mut f, client, "mtm-2");
    for command in ["split v", "mark m", r#"[app_id="^mtm-1$"] focus"#] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success);
    }
    let outcome = crate::command::execute(f.niri_state(), "move container to mark m");
    assert!(outcome[0].success, "{outcome:?}");

    let workspace = mark_move_workspace(&mut f);
    let split = &workspace["nodes"][0];
    assert_eq!(
        workspace["representation"], "H[V[mtm-2 mtm-1]]",
        "{workspace}"
    );
    assert_eq!(split["nodes"][1]["focused"], true);
    assert_eq!(focus_apps(split), ["mtm-1", "mtm-2"]);
}

#[test]
fn move_to_mark_from_split_focuses_the_old_sibling() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    map_window(&mut f, client, "mtm-1");
    assert!(crate::command::execute(f.niri_state(), "mark m")[0].success);
    map_window(&mut f, client, "mtm-2");
    assert!(crate::command::execute(f.niri_state(), "split v")[0].success);
    map_window(&mut f, client, "mtm-3");
    let outcome = crate::command::execute(f.niri_state(), "move container to mark m");
    assert!(outcome[0].success, "{outcome:?}");

    let workspace = mark_move_workspace(&mut f);
    assert_eq!(
        workspace["representation"], "H[mtm-1 mtm-3 V[mtm-2]]",
        "{workspace}"
    );
    assert_eq!(
        workspace["nodes"][2]["nodes"][0]["focused"], true,
        "{workspace}"
    );
    assert_eq!(workspace["nodes"][1]["focused"], false);
    assert_eq!(focus_apps(&workspace), ["split", "mtm-3", "mtm-1"]);
}

// Seed 40373: a failed move wraps a tabbed workspace's children without
// arranging, so the wrapper reports calloc's empty box and percent 0 even
// under a tabbed workspace (sway/commands/move.c:430-436,
// sway/ipc-json.c:744-755). Oracle row
// tabbed_workspace_move_to_missing_mark_wraps_unarranged.
#[test]
fn failed_move_wraps_tabbed_workspace_children_unarranged() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    map_window(&mut f, client, "mtm-1");
    for command in ["focus parent", "layout tabbed"] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success);
    }
    let outcome = crate::command::execute(f.niri_state(), "move container to mark m");
    assert!(!outcome[0].success, "{outcome:?}");

    let workspace = mark_move_workspace(&mut f);
    let wrapper = &workspace["nodes"][0];
    assert_eq!(wrapper["layout"], "tabbed", "{workspace}");
    assert_eq!(wrapper["percent"], 0.0, "{wrapper}");
    assert_eq!(wrapper["rect"]["width"], 0);
    assert!(wrapper["nodes"][0]["percent"].is_null(), "{wrapper}");
    // GET_TREE takes the tab bar from the wrapper's empty box, and the view
    // keeps the box the tabbed workspace gave it (sway/ipc-json.c:816-825).
    let view = &wrapper["nodes"][0];
    let tab = view["deco_rect"]["height"].as_i64().unwrap();
    assert!(tab > 0, "{view}");
    assert_eq!(wrapper["rect"]["y"], tab, "{wrapper}");
    assert_eq!(wrapper["rect"]["height"], -tab, "{wrapper}");
    assert_eq!(view["rect"]["y"], tab, "{view}");
    assert_eq!(view["rect"]["height"], 720 - tab, "{view}");
}

// Seed 40373: moving a focused workspace to a mark on one of its own views
// wraps the children, then does nothing because the mark is inside the
// wrapper; sway still arranges the workspace, so the wrapper reports its
// tabbed share (sway/commands/move.c:430-436, 243-246 and 628-635). Oracle
// row workspace_move_to_own_mark_wraps_children.
#[test]
fn workspace_move_to_own_mark_wraps_children_arranged() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    map_window(&mut f, client, "mtm-1");
    assert!(crate::command::execute(f.niri_state(), "mark m")[0].success);
    map_window(&mut f, client, "mtm-2");
    for command in ["focus parent", "layout tabbed"] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success);
    }
    let outcome = crate::command::execute(f.niri_state(), "move container to mark m");
    assert!(outcome[0].success, "{outcome:?}");

    let workspace = mark_move_workspace(&mut f);
    assert_eq!(
        workspace["representation"], "T[T[mtm-1 mtm-2]]",
        "{workspace}"
    );
    let wrapper = &workspace["nodes"][0];
    let percent = wrapper["percent"].as_f64().unwrap();
    assert!(percent > 0.9 && percent < 1., "{wrapper}");
    assert!(wrapper["rect"]["height"].as_i64().unwrap() > 0, "{wrapper}");
}

// Seed 40058: after a tiled view moves onto a floating mark, the workspace
// focus list keeps the seat stack's recency across layers behind the moved
// view, so a tiled view focused after the marked floating one stays ahead of
// it (sway/input/seat.c, `focus_inactive_children_iterator`,
// sway/ipc-json.c). Oracle row move_to_floating_mark_keeps_seat_recency.
#[test]
fn move_to_floating_mark_keeps_seat_recency_across_layers() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    map_window(&mut f, client, "mtm-1");
    for command in ["floating enable", "mark m"] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success);
    }
    map_window(&mut f, client, "mtm-2");
    map_window(&mut f, client, "mtm-3");
    let outcome = crate::command::execute(f.niri_state(), "move container to mark m");
    assert!(outcome[0].success, "{outcome:?}");

    let workspace = mark_move_workspace(&mut f);
    let all = workspace["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .chain(workspace["floating_nodes"].as_array().unwrap())
        .collect::<Vec<_>>();
    let focus = workspace["focus"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| {
            all.iter().find(|node| node["id"] == *id).unwrap()["app_id"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect::<Vec<_>>();
    assert_eq!(focus, ["mtm-3", "mtm-2", "mtm-1"], "{workspace}");
}

// Seed 40058: a floating container moved to its own mark stays put and
// succeeds (`container_move_to_container`, sway/commands/move.c:243-246).
// Oracle row floating_container_move_to_own_mark_is_noop.
#[test]
fn floating_container_move_to_own_mark_is_a_successful_noop() {
    let mut f = Fixture::new();
    // Portrait, so the workspace starts splitv and floating it resets the
    // workspace to splith (sway/commands/floating.c:29-31).
    f.add_output(1, (1270, 1408));
    let client = f.add_client();
    map_window(&mut f, client, "mtm-1");
    for command in ["focus parent", "floating enable", "mark m"] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success);
    }
    let before = mark_move_workspace(&mut f);
    assert_eq!(before["representation"], "H[]", "{before}");
    let outcome = crate::command::execute(f.niri_state(), "move container to mark m");
    assert!(outcome[0].success, "{outcome:?}");
    assert_eq!(mark_move_workspace(&mut f), before);
}

fn workspace_apps(f: &mut Fixture, name: &str) -> Vec<String> {
    let workspace = f
        .swayward()
        .layout
        .workspaces()
        .find(|(_, _, workspace)| workspace.sway_name().as_deref() == Some(name))
        .unwrap()
        .2;
    workspace
        .tiles()
        .filter_map(|tile| {
            crate::utils::with_toplevel_role(tile.window().toplevel(), |role| role.app_id.clone())
        })
        .collect()
}

// Differential family diff-fam-v3-layout-misc (random-v3 seeds 40044, 40210,
// 40445, 40492, two outputs): a container moved to another output's workspace
// lands after that workspace's focus-inactive tiling container
// (`seat_get_focus_inactive_tiling`, sway/input/seat.c:1374-1389;
// sway/commands/move.c:515). Sway keeps one seat-wide focus stack, so a window
// that was focused more recently than the destination's focus, and moved there
// without activation, becomes that container: the next arrival lands after it.
#[test]
fn move_to_other_output_keeps_arrival_order() {
    let mut f = Fixture::new();
    f.add_named_output_at("left".into(), (800, 600), Some((0, 0)));
    f.add_named_output_at("right".into(), (800, 600), Some((800, 0)));
    let client = f.add_client();

    assert!(crate::command::execute(f.niri_state(), "focus output left")[0].success);
    map_window(&mut f, client, "one");
    assert!(crate::command::execute(f.niri_state(), "focus output right")[0].success);
    let right = f
        .swayward()
        .layout
        .active_workspace()
        .unwrap()
        .sway_name()
        .unwrap();
    map_window(&mut f, client, "two");
    map_window(&mut f, client, "three");
    assert!(crate::command::execute(f.niri_state(), "focus output left")[0].success);
    // `three` was focused after `two`, so after crossing it is the right
    // workspace's focus-inactive container and `four` lands after it.
    map_window(&mut f, client, "four");
    assert!(crate::command::execute(f.niri_state(), "move container to output right")[0].success);
    assert_eq!(workspace_apps(&mut f, &right), ["two", "three", "four"]);
    map_window(&mut f, client, "five");
    assert!(crate::command::execute(f.niri_state(), "move container to output right")[0].success);
    assert_eq!(
        workspace_apps(&mut f, &right),
        ["two", "three", "four", "five"]
    );
}

// Seed 40445: a view assigned to an unfocused workspace maps without focus,
// so sway appends it to the end of the seat focus stack (`seat_node_from_node`,
// sway/input/seat.c:327-354). The destination's focus-inactive container is
// still the view focused there before, and a container moved in lands after it.
#[test]
fn move_to_other_output_lands_after_focus_not_an_assigned_view() {
    let mut f = Fixture::new();
    f.add_named_output_at("left".into(), (800, 600), Some((0, 0)));
    f.add_named_output_at("right".into(), (800, 600), Some((800, 0)));
    let client = f.add_client();

    assert!(crate::command::execute(f.niri_state(), "focus output left")[0].success);
    map_window(&mut f, client, "one");
    assert!(crate::command::execute(f.niri_state(), "focus output right")[0].success);
    let right = f
        .swayward()
        .layout
        .active_workspace()
        .unwrap()
        .sway_name()
        .unwrap();
    map_window(&mut f, client, "two");
    map_window(&mut f, client, "three");
    assert!(crate::command::execute(f.niri_state(), "focus output left")[0].success);
    let assign = format!(r#"assign [app_id="four"] workspace {right}"#);
    assert!(crate::command::execute(f.niri_state(), &assign)[0].success);
    map_window(&mut f, client, "four");
    assert_eq!(workspace_apps(&mut f, &right), ["two", "three", "four"]);
    map_window(&mut f, client, "five");
    assert!(crate::command::execute(f.niri_state(), "move container to output right")[0].success);
    assert_eq!(
        workspace_apps(&mut f, &right),
        ["two", "three", "five", "four"]
    );
}

/// `swap_floating_with_tiled_mark` in the oracle: sway swaps a floating view
/// and a tiled one, each taking the other's place, and focus follows the
/// container that had it (`swap_places`, `swap_focus`,
/// sway/tree/container.c:1718-1798). Differential seed 11697.
#[test]
fn swap_trades_places_between_a_floating_and_a_tiled_view() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    for app_id in ["one", "two", "three"] {
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(app_id.into());
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        if app_id == "one" {
            assert!(crate::command::execute(f.niri_state(), "mark tiled")[0].success);
        }
    }
    assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);

    let app_id = |window: &crate::window::Mapped| {
        crate::utils::with_toplevel_role(window.toplevel(), |role| role.app_id.clone()).unwrap()
    };
    let state = |f: &mut Fixture| {
        let layout = &f.swayward().layout;
        let workspace = layout.active_workspace().unwrap();
        let tiled = workspace
            .tiling()
            .tiles()
            .map(|tile| app_id(tile.window()))
            .collect::<Vec<_>>();
        let floating = workspace
            .floating()
            .tiles()
            .map(|tile| app_id(tile.window()))
            .collect::<Vec<_>>();
        (tiled, floating, app_id(layout.focus().unwrap()))
    };

    let outcome = crate::command::execute(f.niri_state(), "swap container with mark tiled");
    assert!(outcome[0].success, "{outcome:?}");
    assert_eq!(
        state(&mut f),
        (
            vec!["three".into(), "two".into()],
            vec!["one".into()],
            "three".into()
        )
    );

    assert!(crate::command::execute(f.niri_state(), r#"[app_id="^two$"] focus"#)[0].success);
    let outcome = crate::command::execute(f.niri_state(), "swap container with mark tiled");
    assert!(outcome[0].success, "{outcome:?}");
    assert_eq!(
        state(&mut f),
        (
            vec!["three".into(), "one".into()],
            vec!["two".into()],
            "two".into()
        )
    );
    assert_eq!(f.swayward().marks_by_window.values().flatten().count(), 1);
}

/// `swap_tiled_with_floating_mark_maps_beside_focus_inactive` in the oracle:
/// when the focused tiled view swaps with a floating mark, it floats with
/// focus, and the next view maps beside the most recent tiled view rather
/// than the view that swapped in (`seat_get_focus_inactive_tiling`,
/// sway/tree/view.c:851-866). Differential seed 17577.
#[test]
fn a_view_mapped_after_a_tiled_floating_swap_lands_beside_the_focus_inactive_view() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
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
    let run = |f: &mut Fixture, command: &str| {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(outcome[0].success, "{command}: {outcome:?}");
    };
    open(&mut f, "one");
    run(&mut f, "mark --add floater");
    run(&mut f, "floating enable");
    open(&mut f, "three");
    open(&mut f, "four");
    run(&mut f, "swap container with mark floater");
    open(&mut f, "five");

    let apps = f
        .swayward()
        .layout
        .active_workspace()
        .unwrap()
        .tiling()
        .tiles()
        .filter_map(|tile| {
            crate::utils::with_toplevel_role(tile.window().toplevel(), |role| role.app_id.clone())
        })
        .collect::<Vec<_>>();
    assert_eq!(apps, ["three", "five", "one"]);
}

// differential family diff-fam-tabbed-split-border-none-percent (random-v2
// seed 14815): in a tabbed wrapper holding a lone V split, `border none` on
// the view leaves the split's box below the tab bar (`apply_tabbed_layout`,
// sway/tree/arrange.c:185-197), so the split still reports
// (720 - titlebar) / 720 (0.9625), not 1.0.
#[test]
fn a_lone_split_under_tabs_keeps_the_tab_bar_out_of_its_percent_after_border_none() {
    let (mut f, socket) = ipc_fixture();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    let mut stream = UnixStream::connect(socket).unwrap();
    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("a".into());
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    let split_percent = |f: &mut Fixture, stream: &mut UnixStream, command: &str| {
        let outcome = query_ipc_with_payload(f, stream, MessageType::RunCommand, command);
        assert_eq!(outcome[0]["success"], true, "{command}: {outcome}");
        let tree = query_ipc(f, stream, MessageType::GetTree);
        let tabbed = tree["nodes"][1]["nodes"][0]["nodes"][0].clone();
        assert_eq!(tabbed["layout"], "tabbed", "{command}: {tabbed:#}");
        let split = &tabbed["nodes"][0];
        assert_eq!(split["layout"], "splitv", "{command}: {tabbed:#}");
        // The split's rect starts below the tab bar and its own titlebar, as
        // in sway's capture (y = 2 * 27); only the tab bar comes out of the
        // percent.
        let titlebar = split["rect"]["y"].as_f64().unwrap() / 2.;
        assert!(titlebar > 0., "{command}: {tabbed:#}");
        let percent = split["percent"].as_f64().unwrap();
        assert!(
            (percent - (720. - titlebar) / 720.).abs() < 1e-9,
            "{command}: {percent} in {tabbed:#}"
        );
    };
    let outcome = query_ipc_with_payload(
        &mut f,
        &mut stream,
        MessageType::RunCommand,
        "layout tabbed",
    );
    assert_eq!(outcome[0]["success"], true, "{outcome}");
    split_percent(&mut f, &mut stream, "split v");
    split_percent(&mut f, &mut stream, "border none");
}

// differential family diff-fam-v3-swap-floating-con-id-residual: sway's
// `container_swap` trades any two containers, wherever they live
// (sway/tree/container.c:1718-1798, 1800-1890).
fn open_swap_view(f: &mut Fixture, client: crate::tests::client::ClientId, app_id: &str) -> i64 {
    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id(app_id.into());
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    crate::ipc::tree::window_id(f.swayward().layout.focus().unwrap().id())
}

fn run_swap_commands(f: &mut Fixture, commands: &[&str]) {
    for command in commands {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(outcome.iter().all(|o| o.success), "{command}: {outcome:?}");
    }
}

/// A workspace's name, tiled app ids, floating app ids, and whether its
/// floating layer is active.
type SwapWorkspaceState = (String, Vec<String>, Vec<String>, bool);

/// Every non-empty workspace, then the focused app id.
fn swap_layout_state(f: &mut Fixture) -> (Vec<SwapWorkspaceState>, String) {
    let app_id = |window: &crate::window::Mapped| {
        crate::utils::with_toplevel_role(window.toplevel(), |role| role.app_id.clone()).unwrap()
    };
    let layout = &f.swayward().layout;
    let workspaces = layout
        .workspaces()
        .map(|(_, _, workspace)| {
            (
                workspace.sway_name().unwrap_or_default(),
                workspace
                    .tiling()
                    .tiles()
                    .map(|t| app_id(t.window()))
                    .collect(),
                workspace
                    .floating()
                    .tiles()
                    .map(|t| app_id(t.window()))
                    .collect(),
                workspace.floating_is_active(),
            )
        })
        .filter(|(_, tiled, floating, _): &SwapWorkspaceState| {
            !tiled.is_empty() || !floating.is_empty()
        })
        .collect();
    (workspaces, app_id(layout.focus().unwrap()))
}

/// Random-v3 seeds 30236 and 32570: a tiled view swaps with a view inside
/// a floating split container. The view that arrives in the split keeps
/// focus, and the split's focus stack is left alone, so its other child
/// stays ahead of the wrapper the arrival landed in.
#[test]
fn swap_trades_a_floating_split_child_with_a_tiled_view() {
    let (mut f, socket) = ipc_fixture();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    let mut stream = UnixStream::connect(socket).unwrap();
    let one = open_swap_view(&mut f, client, "one");
    open_swap_view(&mut f, client, "two");
    run_swap_commands(&mut f, &["move down", "focus parent; floating toggle"]);
    open_swap_view(&mut f, client, "three");
    run_swap_commands(&mut f, &[&format!("swap container with con_id {one}")]);

    let tree = query_ipc(&mut f, &mut stream, MessageType::GetTree);
    let workspace = &tree["nodes"][1]["nodes"][0];
    let tiled = workspace["nodes"].as_array().unwrap();
    assert_eq!(tiled.len(), 1, "{workspace:#}");
    assert_eq!(tiled[0]["app_id"], "one", "{workspace:#}");
    let group = &workspace["floating_nodes"][0];
    let children = group["nodes"].as_array().unwrap();
    assert_eq!(children.len(), 2, "{group:#}");
    let (wrapper, two) = (&children[0], &children[1]);
    assert_eq!(wrapper["nodes"][0]["app_id"], "three", "{group:#}");
    assert_eq!(wrapper["nodes"][0]["focused"], true, "{group:#}");
    assert_eq!(two["app_id"], "two", "{group:#}");
    assert_eq!(
        group["focus"],
        serde_json::json!([two["id"], wrapper["id"]]),
        "{group:#}"
    );
}

/// Random-v3 seeds 31781, 30812 and 32158: a floating view swaps with a tiled
/// view on another workspace, and the seat focuses whichever now holds the
/// focused one's place.
#[test]
fn swap_trades_a_floating_view_with_a_tiled_view_on_another_workspace() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    open_swap_view(&mut f, client, "one");
    run_swap_commands(&mut f, &["mark m", "workspace number 3"]);
    open_swap_view(&mut f, client, "seven");
    run_swap_commands(&mut f, &["floating enable", "swap container with mark m"]);
    assert_eq!(
        swap_layout_state(&mut f),
        (
            vec![
                ("1".into(), vec!["seven".into()], vec![], false),
                ("3".into(), vec![], vec!["one".into()], true),
            ],
            "one".into()
        )
    );

    // The tiled view on the hidden workspace is the target this time.
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    open_swap_view(&mut f, client, "seven");
    run_swap_commands(&mut f, &["floating enable"]);
    let eight = open_swap_view(&mut f, client, "eight");
    run_swap_commands(
        &mut f,
        &[
            "move container to workspace oracle",
            &format!("swap container with con_id {eight}"),
        ],
    );
    assert_eq!(
        swap_layout_state(&mut f),
        (
            vec![
                ("1".into(), vec![], vec!["eight".into()], true),
                ("oracle".into(), vec!["seven".into()], vec![], false),
            ],
            "eight".into()
        )
    );
}

/// Random-v3 seed 32390: the same swap across two outputs, with the
/// floating view unfocused on the other output.
#[test]
fn swap_trades_a_floating_view_with_a_tiled_view_on_another_output() {
    let mut f = Fixture::new();
    f.add_named_output_at("left".into(), (1280, 720), Some((0, 0)));
    f.add_named_output_at("right".into(), (1280, 720), Some((1280, 0)));
    let client = f.add_client();
    run_swap_commands(&mut f, &["focus output left"]);
    open_swap_view(&mut f, client, "one");
    run_swap_commands(&mut f, &["floating enable", "mark m", "focus output right"]);
    open_swap_view(&mut f, client, "two");
    run_swap_commands(&mut f, &["swap container with mark m"]);
    let (workspaces, focus) = swap_layout_state(&mut f);
    assert_eq!(focus, "one");
    let mut tiled = workspaces
        .iter()
        .map(|(_, t, fl, _)| (t.clone(), fl.clone()))
        .collect::<Vec<_>>();
    tiled.sort();
    assert_eq!(
        tiled,
        [
            (vec![], vec!["two".to_owned()]),
            (vec!["one".to_owned()], vec![])
        ]
    );
}

/// Random-v3 seed 30844: a tiled view swaps with a floating view on a hidden
/// workspace.
#[test]
fn swap_trades_a_tiled_view_with_a_floating_view_on_a_hidden_workspace() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    open_swap_view(&mut f, client, "three");
    run_swap_commands(&mut f, &["move container to workspace 2"]);
    open_swap_view(&mut f, client, "four");
    run_swap_commands(
        &mut f,
        &[
            r#"[app_id="three"] mark --add x"#,
            r#"[app_id="three"] floating enable"#,
            "swap container with mark x",
        ],
    );
    assert_eq!(
        swap_layout_state(&mut f),
        (
            vec![
                ("1".into(), vec!["three".into()], vec![], false),
                ("2".into(), vec![], vec!["four".into()], true),
            ],
            "three".into()
        )
    );
}

/// Random-v3 seeds 32240 and 32745: the view taking a shown scratchpad
/// view's floating place is shown from the scratchpad, which focuses it
/// (`root_scratchpad_show`, sway/tree/root.c:157-204).
#[test]
fn swap_with_a_shown_scratchpad_view_focuses_the_view_that_floats() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    let four = open_swap_view(&mut f, client, "four");
    open_swap_view(&mut f, client, "six");
    run_swap_commands(
        &mut f,
        &[
            "move scratchpad",
            r#"[app_id="six"] focus"#,
            &format!("swap container with con_id {four}"),
        ],
    );
    assert_eq!(
        swap_layout_state(&mut f),
        (
            vec![("1".into(), vec!["six".into()], vec!["four".into()], true)],
            "four".into()
        )
    );
}

/// Random-v3 seed 30812: a fullscreen floating view swaps with a tiled view on
/// another workspace. Sway keeps a fullscreen floating view in the floating
/// list, so the view taking its place floats, fullscreen, and the one leaving
/// is tiled (`swap_places`, sway/tree/container.c:1747-1760).
#[test]
fn swap_hands_a_fullscreen_floating_place_to_the_view_taking_it() {
    let (mut f, socket) = ipc_fixture();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    let mut stream = UnixStream::connect(socket).unwrap();
    open_swap_view(&mut f, client, "two");
    let three = open_swap_view(&mut f, client, "three");
    run_swap_commands(
        &mut f,
        &[
            "move container to workspace 2",
            "floating toggle",
            "fullscreen enable",
            &format!("swap container with con_id {three}"),
        ],
    );
    let tree = query_ipc(&mut f, &mut stream, MessageType::GetTree);
    let workspaces = tree["nodes"][1]["nodes"].as_array().unwrap();
    let one = &workspaces[0];
    assert_eq!(one["nodes"], serde_json::json!([]), "{one:#}");
    let floater = &one["floating_nodes"][0];
    assert_eq!(floater["app_id"], "three", "{one:#}");
    assert_eq!(floater["fullscreen_mode"], 1, "{one:#}");
    assert_eq!(floater["focused"], true, "{one:#}");
    let two = &workspaces[1];
    assert_eq!(two["floating_nodes"], serde_json::json!([]), "{two:#}");
    assert_eq!(two["nodes"][0]["app_id"], "two", "{two:#}");
    assert_eq!(two["nodes"][0]["fullscreen_mode"], 0, "{two:#}");
}

/// The same on one workspace: the tiled view takes the fullscreen floating
/// view's place in the floating list.
#[test]
fn swap_on_one_workspace_hands_a_fullscreen_floating_place_to_the_tiled_view() {
    let (mut f, socket) = ipc_fixture();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    let mut stream = UnixStream::connect(socket).unwrap();
    open_swap_view(&mut f, client, "two");
    let three = open_swap_view(&mut f, client, "three");
    run_swap_commands(
        &mut f,
        &[
            r#"[app_id="two"] focus"#,
            "floating toggle",
            "fullscreen enable",
            &format!("swap container with con_id {three}"),
        ],
    );
    let tree = query_ipc(&mut f, &mut stream, MessageType::GetTree);
    let workspace = &tree["nodes"][1]["nodes"][0];
    assert_eq!(workspace["representation"], "H[two]", "{workspace:#}");
    let floater = &workspace["floating_nodes"][0];
    assert_eq!(floater["app_id"], "three", "{workspace:#}");
    assert_eq!(floater["fullscreen_mode"], 1, "{workspace:#}");
}
