#[test]
fn closing_last_window_focuses_workspace_node() {
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

    let mapped_id = f
        .swayward()
        .layout
        .windows()
        .find_map(|(_, mapped)| {
            (mapped.toplevel().wl_surface().id().protocol_id() == surface.id().protocol_id())
                .then(|| mapped.id())
        })
        .unwrap();
    let focused_window = crate::ipc::tree::window_id(mapped_id);
    let focused_workspace =
        crate::ipc::tree::workspace_id(f.swayward().layout.active_workspace().unwrap().id().get());

    let swayward = f.swayward();
    let tree = describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    );
    let mut focused = Vec::new();
    collect_focused_nodes(&tree, &mut focused);
    assert_eq!(focused, [focused_window]);

    let window = f.client(client).window(&surface);
    window.attach_null();
    window.commit();
    f.double_roundtrip(client);

    let swayward = f.swayward();
    let tree = describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    );
    focused.clear();
    collect_focused_nodes(&tree, &mut focused);
    assert_eq!(focused, [focused_workspace]);
}

#[test]
fn fullscreen_with_a_focused_floating_window_does_not_target_the_tiling_parent() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    map_test_window(&mut f, client, "tiled");
    map_test_window(&mut f, client, "floating");
    assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus tiling")[0].success);
    assert!(crate::command::execute(f.niri_state(), "layout tabbed")[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus floating")[0].success);
    assert!(crate::command::execute(f.niri_state(), "fullscreen")[0].success);

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    assert_eq!(
        find_json_node_with_app_id(&tree, "floating").unwrap()["fullscreen_mode"],
        1
    );
    assert_eq!(
        find_json_parent_of_app_id(&tree, "tiled").unwrap()["fullscreen_mode"],
        0
    );

    assert!(crate::command::execute(f.niri_state(), "fullscreen disable")[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus tiling")[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
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
        find_json_parent_of_app_id(&tree, "tiled").unwrap()["focused"],
        true
    );
}

#[test]
fn get_tree_has_one_focused_node_after_scratchpad_cycle() {
    let (mut f, _) = ipc_fixture();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    for app_id in ["scratch", "tiled"] {
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
    assert!(crate::command::execute(f.niri_state(), "workspace 2")[0].success);
    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("inactive".into());
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    assert!(crate::command::execute(f.niri_state(), "workspace 1")[0].success);

    for command in [
        r#"[app_id="scratch"] move scratchpad"#,
        "scratchpad show",
        "scratchpad show",
        "scratchpad show",
    ] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success);
    }

    let tree: swayward_ipc::Node = serde_json::from_value(get_tree(&mut f)).unwrap();
    let mut focused = Vec::new();
    collect_focused_nodes(&tree, &mut focused);
    assert_eq!(focused.len(), 1, "focused nodes: {focused:?}");
    let focused_id = focused[0];
    let focused_workspace = tree
        .nodes
        .iter()
        .flat_map(|output| &output.nodes)
        .find(|workspace| {
            workspace
                .nodes
                .iter()
                .chain(&workspace.floating_nodes)
                .any(|node| node.id == focused_id)
        })
        .unwrap();
    assert_eq!(focused_workspace.focus.first(), Some(&focused_id));

    assert!(crate::command::execute(f.niri_state(), "workspace empty")[0].success);
    let tree: swayward_ipc::Node = serde_json::from_value(get_tree(&mut f)).unwrap();
    let active = f.swayward().layout.active_workspace().unwrap().id().get();
    let mut focused = Vec::new();
    collect_focused_nodes(&tree, &mut focused);
    assert_eq!(focused, [crate::ipc::tree::workspace_id(active)]);
}

/// Random oracle seed 380: `focus parent; move scratchpad` on a lone window
/// hides its container and focuses the emptied workspace. Sway's
/// root_scratchpad_add_container refocuses the parent's focus-inactive node
/// (sway/tree/root.c:91-104), so nothing inside the scratchpad is focused.
#[test]
fn hiding_a_container_in_the_scratchpad_focuses_the_workspace() {
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

    for command in ["focus parent", "move scratchpad"] {
        let reply = crate::command::execute(f.niri_state(), command);
        assert!(reply[0].success, "{command}: {reply:?}");
    }

    let tree: swayward_ipc::Node = serde_json::from_value(get_tree(&mut f)).unwrap();
    let active = f.swayward().layout.active_workspace().unwrap().id().get();
    let mut focused = Vec::new();
    collect_focused_nodes(&tree, &mut focused);
    assert_eq!(focused, [crate::ipc::tree::workspace_id(active)]);
}

/// Random oracle seed 487: hiding a shown scratchpad window again focuses the
/// workspace's focus-inactive view, even after `focus parent` selected the
/// workspace before the show (root_scratchpad_hide, sway/tree/root.c:211-229).
#[test]
fn hiding_a_shown_scratchpad_window_after_focus_parent_refocuses_the_tiled_view() {
    let (mut f, _) = ipc_fixture();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
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
    map(&mut f, "scratch");
    assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);
    map(&mut f, "tiled");
    for command in ["focus parent", "scratchpad show", "move scratchpad"] {
        let reply = crate::command::execute(f.niri_state(), command);
        assert!(reply[0].success, "{command}: {reply:?}");
    }

    let tree: swayward_ipc::Node = serde_json::from_value(get_tree(&mut f)).unwrap();
    let mut focused = Vec::new();
    collect_focused_nodes(&tree, &mut focused);
    let tiled = f
        .swayward()
        .layout
        .windows()
        .find(|(_, mapped)| {
            crate::utils::with_toplevel_role(mapped.toplevel(), |role| {
                role.app_id.as_deref() == Some("tiled")
            })
        })
        .map(|(_, mapped)| crate::ipc::tree::window_id(mapped.id()))
        .unwrap();
    assert_eq!(focused, [tiled]);
}

/// A hidden group's children have no titlebar in GET_TREE and report their
/// whole slot (get_deco_rect with no workspace, sway/ipc-json.c:543-553).
#[test]
fn hidden_scratchpad_group_children_report_no_titlebar() {
    let (mut f, _) = ipc_fixture();
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
    // Tabbed, so every child has a titlebar while the group is shown. Pinned
    // sway then reports each hidden child at the group's full box with an
    // empty deco_rect.
    for command in [
        "focus parent",
        "layout tabbed",
        "floating enable",
        "move scratchpad",
    ] {
        let reply = crate::command::execute(f.niri_state(), command);
        assert!(reply[0].success, "{command}: {reply:?}");
    }

    let tree: swayward_ipc::Node = serde_json::from_value(get_tree(&mut f)).unwrap();
    let group = &tree.nodes[0].nodes[0].floating_nodes[0];
    assert_eq!(group.nodes.len(), 2);
    for child in &group.nodes {
        assert_eq!(child.deco_rect, swayward_ipc::Rect::default());
        assert_eq!(child.rect, group.rect);
    }
}

/// Random oracle seeds 323 and 380: showing a group hidden with `focus parent;
/// move scratchpad` focuses its most recently focused view, not the group
/// (root_scratchpad_show: seat_set_focus(seat_get_focus_inactive(con)),
/// sway/tree/root.c:185-186).
#[test]
fn showing_a_hidden_group_focuses_its_view() {
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
    for command in ["focus parent", "move scratchpad", "scratchpad show"] {
        let reply = crate::command::execute(f.niri_state(), command);
        assert!(reply[0].success, "{command}: {reply:?}");
    }

    let tree: swayward_ipc::Node = serde_json::from_value(get_tree(&mut f)).unwrap();
    let mut focused = Vec::new();
    collect_focused_nodes(&tree, &mut focused);
    let group = &tree.nodes[1].nodes[0].floating_nodes[0];
    assert_eq!(focused, [group.nodes[0].id]);
}

#[test]
fn scratchpad_hides_focused_window_and_show_cycles_windows() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let mut surfaces = Vec::new();
    for _ in 0..2 {
        let window = f.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        surfaces.push(surface);
        assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);
    }

    let swayward = f.swayward();
    assert!(swayward.layout.focus().is_none());
    let tree = describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    );
    assert_eq!(tree.nodes[0].nodes[0].floating_nodes.len(), 2);
    assert!(tree.nodes[0].nodes[0]
        .floating_nodes
        .iter()
        .all(|node| node.scratchpad_state.as_deref() == Some("fresh")));
    assert_eq!(tree.nodes[0].nodes[0].fullscreen_mode, 1);
    assert_eq!(
        tree.nodes[0].nodes[0].focus,
        tree.nodes[0].nodes[0]
            .floating_nodes
            .iter()
            .rev()
            .map(|node| node.id)
            .collect::<Vec<_>>()
    );

    assert!(crate::command::execute(f.niri_state(), "scratchpad show")[0].success);
    let first = f.swayward().layout.focus().unwrap().id();
    assert!(crate::command::execute(f.niri_state(), "scratchpad show")[0].success);
    assert!(f.swayward().layout.focus().is_none());
    assert!(crate::command::execute(f.niri_state(), "scratchpad show")[0].success);
    let second = f.swayward().layout.focus().unwrap().id();
    assert_ne!(first, second);
    assert_eq!(f.swayward().layout.scratchpad_windows().count(), 1);

    assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);
    assert!(f.swayward().layout.focus().is_none());
    assert_eq!(f.swayward().layout.scratchpad_windows().count(), 2);
    assert_eq!(surfaces.len(), 2);
}

#[test]
fn sticky_state_survives_moving_a_window_back_to_scratchpad() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();

    map_test_window(&mut f, client, "sticky-scratchpad");
    assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);
    assert!(crate::command::execute(f.niri_state(), "scratchpad show")[0].success);
    assert!(crate::command::execute(f.niri_state(), "sticky enable")[0].success);
    assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);

    let swayward = f.swayward();
    let tree = describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    );
    assert!(tree.nodes[0].nodes[0].floating_nodes[0].sticky);
}

#[test]
fn directional_move_emits_one_settled_sway_move_event() {
    let (mut f, socket) = ipc_fixture();
    f.add_output(1, (1280, 800));
    let client = f.add_client();
    for app_id in ["left", "moved"] {
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(app_id.into());
        window.set_title(app_id);
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    }

    let moved_id = f.swayward().layout.focus().unwrap().id();
    let before = {
        let swayward = f.swayward();
        serde_json::to_value(crate::ipc::tree::describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &swayward.marks_by_window,
            &swayward.marks_by_container,
        ))
        .unwrap()
    };
    let before =
        super::super::ipc::server::find_node_by_id(&before, crate::ipc::tree::window_id(moved_id))
            .unwrap()["rect"]["x"]
            .as_i64()
            .unwrap();

    let mut subscriber = UnixStream::connect(socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["window"]"#,
        ))
        .unwrap();
    let _ = read_ipc_reply(&mut f, &mut subscriber);

    assert!(crate::command::execute(f.niri_state(), "move left")[0].success);
    let (event_type, payload) = read_ipc_reply(&mut f, &mut subscriber);
    assert_eq!(event_type, EVENT_WINDOW);
    let event = serde_json::from_str::<Value>(&payload).unwrap();
    assert_eq!(event["change"], "move");
    assert_eq!(
        event["container"]["id"],
        crate::ipc::tree::window_id(moved_id)
    );
    assert!(event["container"]["rect"]["x"].as_i64().unwrap() < before);
    let expected: Value = serde_json::from_str(&sway_fixture!("events/window.move.json")).unwrap();
    assert_event_shape(&expected, &event, "$window");
}

#[test]
fn scratchpad_show_moves_visible_window_to_current_workspace_and_focuses_it() {
    let (mut f, socket) = ipc_fixture();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let scratchpad = f.client(client).create_window();
    scratchpad.xdg_toplevel.set_app_id("event-one".into());
    scratchpad.set_title("event-one");
    scratchpad.commit();
    let scratchpad_surface = scratchpad.surface.clone();
    f.roundtrip(client);
    let scratchpad = f.client(client).window(&scratchpad_surface);
    scratchpad.attach_new_buffer();
    scratchpad.ack_last_and_commit();
    f.double_roundtrip(client);
    let scratchpad_id = f.swayward().layout.focus().unwrap().id();

    assert!(crate::command::execute(f.niri_state(), "move to scratchpad")[0].success);
    assert!(crate::command::execute(f.niri_state(), "scratchpad show")[0].success);
    assert!(crate::command::execute(f.niri_state(), "workspace target")[0].success);

    let tiled = f.client(client).create_window();
    tiled.commit();
    let tiled_surface = tiled.surface.clone();
    f.roundtrip(client);
    let tiled = f.client(client).window(&tiled_surface);
    tiled.attach_new_buffer();
    tiled.ack_last_and_commit();
    f.double_roundtrip(client);

    let mut subscriber = UnixStream::connect(&socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["window"]"#,
        ))
        .unwrap();
    let _ = read_ipc_reply(&mut f, &mut subscriber);

    let mut command = UnixStream::connect(&socket).unwrap();
    let reply = query_ipc_with_payload(
        &mut f,
        &mut command,
        MessageType::RunCommand,
        "scratchpad show",
    );
    assert_eq!(reply[0]["success"], true);
    let focused = f.swayward().layout.focus().unwrap();
    assert_eq!(focused.id(), scratchpad_id);
    let focused_window = focused.window.clone();
    let workspace = f.swayward().layout.active_workspace().unwrap();
    assert_eq!(workspace.sway_name().as_deref(), Some("target"));
    assert!(workspace.has_window(&focused_window));
    let mut events = Vec::new();
    let mut remainder = Vec::new();
    for _ in 0..2 {
        let ((event_type, payload), next) =
            read_ipc_reply_with_remainder(&mut f, &mut subscriber, remainder);
        remainder = next;
        assert_eq!(event_type, EVENT_WINDOW);
        events.push(serde_json::from_str::<Value>(&payload).unwrap());
    }
    assert_eq!(events[0]["change"], "focus");
    assert_eq!(
        events[0]["container"]["id"],
        crate::ipc::tree::window_id(scratchpad_id)
    );
    assert_eq!(events[0]["container"]["type"], "floating_con");
    assert_eq!(events[1]["change"], "move");
}

#[test]
fn moving_fullscreen_window_to_scratchpad_clears_its_fullscreen_state() {
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

    assert!(crate::command::execute(f.niri_state(), "fullscreen enable")[0].success);
    assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);
    assert!(crate::command::execute(f.niri_state(), "scratchpad show")[0].success);
    let shown = f.swayward().layout.focus().unwrap().window.clone();
    assert_eq!(f.swayward().layout.fullscreen_mode(&shown), None);

    assert!(crate::command::execute(f.niri_state(), "floating toggle")[0].success);
    assert!(!f
        .swayward()
        .layout
        .active_workspace()
        .unwrap()
        .is_floating(&shown));
}

#[test]
fn workspace_fullscreen_descendant_does_not_move_to_an_adjacent_output() {
    let mut f = Fixture::new();
    f.add_named_output_at("left".into(), (100, 100), Some((0, 0)));
    f.add_named_output_at("right".into(), (100, 100), Some((100, 0)));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    assert!(crate::command::execute(f.niri_state(), "split v")[0].success);
    let second = f.client(client).create_window();
    second.commit();
    let second_surface = second.surface.clone();
    f.roundtrip(client);
    let second = f.client(client).window(&second_surface);
    second.attach_new_buffer();
    second.ack_last_and_commit();
    f.double_roundtrip(client);
    for command in ["focus parent", "fullscreen enable", "focus child"] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success);
    }
    let id = f.swayward().layout.focus().unwrap().id();
    assert!(crate::command::execute(f.niri_state(), "move right")[0].success);

    let swayward = f.swayward();
    let (_, mapped) = swayward
        .layout
        .windows()
        .find(|(_, mapped)| mapped.id() == id)
        .unwrap();
    let output = swayward
        .layout
        .workspaces()
        .find(|(_, _, workspace)| workspace.has_window(&mapped.window))
        .and_then(|(monitor, _, _)| monitor)
        .unwrap()
        .output_name();
    assert_eq!(output, "left");
}

#[test]
fn targeted_fullscreen_toggle_replaces_another_windows_fullscreen() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    map_test_window(&mut f, client, "first");
    let first = f.swayward().layout.focus().unwrap().window.clone();
    map_test_window(&mut f, client, "second");
    let second = f.swayward().layout.focus().unwrap().window.clone();

    assert!(
        crate::command::execute(f.niri_state(), r#"[app_id="first"] fullscreen enable"#)[0].success
    );
    assert!(
        crate::command::execute(f.niri_state(), r#"[app_id="second"] fullscreen toggle"#)[0]
            .success
    );

    assert_eq!(f.swayward().layout.fullscreen_mode(&first), None);
    assert_eq!(
        f.swayward().layout.fullscreen_mode(&second),
        Some(crate::layout::tiling_tree::FullscreenMode::Workspace)
    );
}

#[test]
fn targeted_global_fullscreen_selects_and_focuses_the_windows_workspace() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.set_title("target");
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    let target = f.swayward().layout.focus().unwrap().window.clone();
    let target_workspace = f.swayward().layout.active_workspace().unwrap().id();

    assert!(crate::command::execute(f.niri_state(), "workspace other")[0].success);
    assert_ne!(
        f.swayward().layout.active_workspace().unwrap().id(),
        target_workspace
    );
    assert!(
        crate::command::execute(
            f.niri_state(),
            r#"[title="target"] fullscreen enable global"#
        )[0]
        .success
    );

    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().id(),
        target_workspace
    );
    assert_eq!(f.swayward().layout.focus().unwrap().window, target);
    assert_eq!(
        f.swayward().layout.fullscreen_mode(&target),
        Some(crate::layout::tiling_tree::FullscreenMode::Global)
    );
    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    assert_eq!(
        find_json_node(&tree, "con", true).unwrap()["fullscreen_mode"],
        2
    );
}

#[test]
fn scratchpad_show_disables_target_workspace_and_global_fullscreen() {
    for fullscreen in ["fullscreen enable", "fullscreen enable global"] {
        let mut f = Fixture::new();
        f.add_output(1, (1920, 1080));
        let client = f.add_client();
        let mut ids = Vec::new();

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

        assert!(crate::command::execute(f.niri_state(), fullscreen)[0].success);
        assert!(f.swayward().layout.focused_fullscreen_mode().is_some());
        let first = crate::ipc::tree::window_id(ids[0]);
        assert!(
            crate::command::execute(f.niri_state(), &format!("[con_id={first}] move scratchpad"))
                [0]
            .success
        );
        assert!(
            crate::command::execute(f.niri_state(), &format!("[con_id={first}] scratchpad show"))
                [0]
            .success
        );

        assert_eq!(
            f.swayward().layout.focused_fullscreen_mode(),
            None,
            "{fullscreen}"
        );
        assert!(
            !f.swayward().layout.global_fullscreen_active(),
            "{fullscreen}"
        );
    }
}

#[test]
fn scratchpad_show_toggles_the_only_window() {
    let (mut f, socket) = ipc_fixture();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("only".into());
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    for command in ["move scratchpad", "scratchpad show"] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success);
    }
    let mut stream = UnixStream::connect(&socket).unwrap();
    let tree = query_ipc(&mut f, &mut stream, MessageType::GetTree);
    assert_eq!(
        find_json_node_with_app_id(&tree, "only").unwrap()["focused"],
        true
    );

    assert!(crate::command::execute(f.niri_state(), "scratchpad show")[0].success);
    let tree = query_ipc(&mut f, &mut stream, MessageType::GetTree);
    let hidden = find_json_node_with_app_id(&tree, "only").unwrap();
    assert_eq!(hidden["focused"], false);
    assert_eq!(hidden["scratchpad_state"], "fresh");
}

/// `floating enable` on a view inside a floating split acts on the split,
/// which is already floating, so the split stays the scratchpad container
/// (sway/commands/floating.c:40-51, sway/tree/container.c:941-944).
/// Differential seed 31529.
#[test]
fn floating_enable_inside_a_shown_scratchpad_split_keeps_it_in_the_scratchpad() {
    let (mut f, socket) = ipc_fixture();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("pad".into());
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    for command in [
        "move scratchpad",
        "scratchpad show",
        "splith",
        "floating enable",
    ] {
        assert!(
            crate::command::execute(f.niri_state(), command)[0].success,
            "{command}"
        );
    }
    let mut stream = UnixStream::connect(&socket).unwrap();
    let tree = query_ipc(&mut f, &mut stream, MessageType::GetTree);
    let workspace = &tree["nodes"][1]["nodes"][0];
    let split = &workspace["floating_nodes"][0];
    assert_eq!(split["type"], "floating_con");
    assert_eq!(split["scratchpad_state"], "fresh");
    assert_eq!(split["nodes"][0]["scratchpad_state"], "none");

    // The split is still a scratchpad container, so `scratchpad show` hides it.
    assert!(crate::command::execute(f.niri_state(), "scratchpad show")[0].success);
    let tree = query_ipc(&mut f, &mut stream, MessageType::GetTree);
    assert_eq!(
        tree["nodes"][1]["nodes"][0]["floating_nodes"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
}

/// Moving the only view out of a shown scratchpad split tiles it and reaps
/// the split, which leaves the scratchpad (sway/commands/move.c:204-232,
/// sway/tree/container.c:495-497, 508-524), so `scratchpad show` then fails
/// with "Scratchpad is empty". Differential seed 31529.
#[test]
fn moving_a_view_out_of_a_shown_scratchpad_split_empties_the_scratchpad() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("pad".into());
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    for command in [
        "move scratchpad",
        "scratchpad show",
        "splith",
        "[workspace=__focused__] move container to workspace 2",
    ] {
        assert!(
            crate::command::execute(f.niri_state(), command)[0].success,
            "{command}"
        );
    }
    let reply = &crate::command::execute(f.niri_state(), "scratchpad show")[0];
    assert!(!reply.success);
    assert_eq!(reply.error.as_deref(), Some("Scratchpad is empty"));
}

#[test]
fn moving_one_view_out_of_a_two_view_scratchpad_split_keeps_the_split_in_the_scratchpad() {
    // Oracle row rv_move_first_of_two_out: the split keeps view B, so it is
    // not reaped and stays a scratchpad container that `scratchpad show`
    // hides (sway/tree/container.c:495-497).
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let mut surfaces = Vec::new();
    for (index, app_id) in ["pad-a", "pad-b"].into_iter().enumerate() {
        if index == 1 {
            for command in ["move scratchpad", "scratchpad show", "splith"] {
                assert!(
                    crate::command::execute(f.niri_state(), command)[0].success,
                    "{command}"
                );
            }
        }
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

    let command = "[app_id=\"^pad-a$\"] move container to workspace 2";
    assert!(crate::command::execute(f.niri_state(), command)[0].success);
    let reply = &crate::command::execute(f.niri_state(), "scratchpad show")[0];
    assert!(reply.success, "{:?}", reply.error);

    let swayward = f.swayward();
    let tree = describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    );
    let scratch = &tree.nodes[0].nodes[0];
    assert_eq!(scratch.floating_nodes.len(), 1);
    let split = &scratch.floating_nodes[0];
    assert_eq!(split.scratchpad_state.as_deref(), Some("fresh"));
    assert_eq!(split.nodes.len(), 1);
    let swayward_ipc::NodeProperties::View(view) = &split.nodes[0].properties else {
        panic!("split child is not a view");
    };
    assert_eq!(view.app_id.as_deref(), Some("pad-b"));
}

/// Focus on a view mapped into a shown scratchpad split is focus inside the
/// split, and `scratchpad show` acts on the split, so it hides the whole
/// split (sway/commands/scratchpad.c:21-36). Oracle row
/// scratchpad_split_second_view_show_hides.
#[test]
fn scratchpad_show_from_a_second_view_in_a_shown_split_hides_the_split() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    for (index, app_id) in ["pad-a", "pad-b"].into_iter().enumerate() {
        if index == 1 {
            for command in ["move scratchpad", "scratchpad show", "splith"] {
                assert!(
                    crate::command::execute(f.niri_state(), command)[0].success,
                    "{command}"
                );
            }
        }
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

    let reply = &crate::command::execute(f.niri_state(), "scratchpad show")[0];
    assert!(reply.success, "{:?}", reply.error);

    let swayward = f.swayward();
    let tree = describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    );
    let scratch = &tree.nodes[0].nodes[0];
    assert_eq!(scratch.floating_nodes.len(), 1);
    assert_eq!(scratch.floating_nodes[0].nodes.len(), 2);
    assert!(tree.nodes[1].nodes[0].floating_nodes.is_empty());
}

#[test]
fn empty_scratch_workspace_is_always_serialized() {
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
    for command in ["move scratchpad", "scratchpad show"] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success);
    }

    let swayward = f.swayward();
    let tree = describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    );
    assert_eq!(tree.nodes[0].nodes[0].name.as_deref(), Some("__i3_scratch"));
    assert!(tree.nodes[0].nodes[0].floating_nodes.is_empty());
}

#[test]
fn get_workspaces_distinguishes_seat_focus_from_output_visibility() {
    let (mut f, _) = ipc_fixture();
    f.add_output(1, (1920, 1080));
    f.add_output(2, (1920, 1080));
    f.niri_focus_output(2);

    let workspaces: Vec<swayward_ipc::Workspace> =
        serde_json::from_value(get_workspaces(&mut f)).unwrap();
    assert_eq!(
        workspaces
            .iter()
            .filter(|workspace| workspace.focused)
            .count(),
        1
    );
    assert_eq!(
        workspaces
            .iter()
            .filter(|workspace| workspace.visible)
            .count(),
        2
    );
    assert!(
        workspaces
            .iter()
            .find(|workspace| workspace.focused)
            .unwrap()
            .visible
    );
}

#[test]
fn workspace_commands_create_sparse_global_identities() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));

    let client = f.add_client();
    for command in ["workspace 1", "workspace 3", "workspace 7"] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success);
        let window = f.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    }

    let swayward = f.swayward();
    let workspaces = describe_workspaces(&swayward.layout, &swayward.global_space);
    assert_eq!(
        workspaces
            .iter()
            .map(|workspace| (workspace.num, workspace.name.as_str(), workspace.focused))
            .collect::<Vec<_>>(),
        [(1, "1", false), (3, "3", false), (7, "7", true)]
    );
}

#[test]
fn focus_next_and_prev_follow_the_immediate_parent_layout() {
    for layout in ["splith", "splitv", "tabbed", "stacking"] {
        let mut f = Fixture::new();
        f.add_output(1, (1920, 1080));
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

        assert!(crate::command::execute(f.niri_state(), &format!("layout {layout}"))[0].success);
        assert!(crate::command::execute(f.niri_state(), r#"[app_id="first"] focus"#)[0].success);
        let first = f.swayward().layout.focus().unwrap().id();

        let outcome = crate::command::execute(f.niri_state(), "focus next");
        assert!(outcome[0].success, "{layout}: {outcome:?}");
        assert_ne!(f.swayward().layout.focus().unwrap().id(), first, "{layout}");

        let outcome = crate::command::execute(f.niri_state(), "focus prev");
        assert!(outcome[0].success, "{layout}: {outcome:?}");
        assert_eq!(f.swayward().layout.focus().unwrap().id(), first, "{layout}");
    }

    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    assert!(f.swayward().layout.focus().is_none());
    let outcome = crate::command::execute(f.niri_state(), "focus next");
    assert!(outcome[0].success, "{outcome:?}");
    assert!(f.swayward().layout.focus().is_none());
}

/// Differential family diff-fam-focus-prev-floating (seeds 1449, 1970): sway answers
/// `focus next|prev` on a floating window with success, taking the axis from the workspace
/// layout and leaving focus alone when no other floater lies that way
/// (sway/commands/focus.c:17-58,434-475).
#[test]
fn focus_next_and_prev_on_a_lone_floating_window_succeed() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    for app_id in ["tiled", "floater"] {
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
    let floater = f.swayward().layout.focus().unwrap().id();

    for command in ["focus prev", "focus next"] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(outcome[0].success, "{command}: {outcome:?}");
        assert_eq!(
            f.swayward().layout.focus().unwrap().id(),
            floater,
            "{command}"
        );
    }
}

/// Oracle row focus_prev_next_two_floating: sway moves `focus next|prev` among floaters along the
/// workspace axis and never wraps, so with nothing on that side focus stays put
/// (node_get_in_direction_floating, sway/commands/focus.c:243-258). This workspace is splith, so
/// next looks right and prev looks left; the oracle row covers the splitv axis.
#[test]
fn focus_next_and_prev_between_two_floaters_do_not_wrap() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let mut floaters = Vec::new();
    for (app_id, x) in [("left", 0), ("right", 600)] {
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(app_id.into());
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        for command in [
            "floating enable".to_owned(),
            format!("move position {x} px 0 px"),
        ] {
            assert!(crate::command::execute(f.niri_state(), &command)[0].success);
        }
        floaters.push(f.swayward().layout.focus().unwrap().id());
    }
    let [left, right] = [floaters[0], floaters[1]];

    for (command, expected) in [
        ("focus next", right),
        ("focus prev", left),
        ("focus prev", left),
        ("focus next", right),
    ] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(outcome[0].success, "{command}: {outcome:?}");
        assert_eq!(
            f.swayward().layout.focus().unwrap().id(),
            expected,
            "{command}"
        );
    }
}

/// Differential family diff-fam-focus-sibling-floating-split (seed 15053): a view inside a
/// floating split is not itself floating, so sway runs `focus next sibling` through
/// `node_get_in_direction_tiling` (sway/commands/focus.c:454-460), which wraps within the split
/// (sway/commands/focus.c:177-192, 216-221).
#[test]
fn focus_next_sibling_wraps_inside_a_floating_split() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
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
        f.swayward().layout.focus().unwrap().id()
    };
    let first = map(&mut f, "first");
    for command in ["floating toggle", "splitv"] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success);
    }
    let second = map(&mut f, "second");
    assert_ne!(first, second);

    for (command, expected) in [
        ("focus next sibling", first),
        ("focus next sibling", second),
        ("focus prev sibling", first),
        ("focus prev sibling", second),
    ] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(outcome[0].success, "{command}: {outcome:?}");
        assert_eq!(
            f.swayward().layout.focus().unwrap().id(),
            expected,
            "{command}"
        );
    }
}

/// Random oracle seed 15489 (differential_seed_15489): `focus parent; move
/// scratchpad` on a workspace wraps its children and hides the wrapper
/// (sway/commands/move.c:921-931); a criteria `kill` then matches a view
/// inside the hidden wrapper and closes it (sway/commands/kill.c).
#[test]
fn criteria_kill_closes_a_view_inside_a_hidden_workspace_wrapper() {
    let (mut f, _) = ipc_fixture();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let mut surfaces = Vec::new();
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
        surfaces.push(surface);
    }
    for command in ["focus parent", "move scratchpad", r#"[app_id="two"] kill"#] {
        let reply = crate::command::execute(f.niri_state(), command);
        assert!(reply[0].success, "{command}: {reply:?}");
    }
    f.double_roundtrip(client);
    let closed = surfaces
        .iter()
        .map(|surface| f.client(client).window(surface).close_requested)
        .collect::<Vec<_>>();
    assert_eq!(closed, [false, true]);
}

/// `floating enable` and `move scratchpad` on a focused workspace wrap its
/// children and then reset the emptied workspace to splith, whatever its
/// layout was (sway/commands/floating.c:28-32, sway/commands/move.c:929-933).
/// Oracle: criteria_kill_in_hidden_scratchpad_wrapper.
#[test]
fn wrapping_a_tall_workspace_leaves_it_splith() {
    for command in ["move scratchpad", "floating enable"] {
        let (mut f, _) = ipc_fixture();
        f.add_output(1, (1080, 1920));
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
        let workspace_layout = |f: &mut Fixture| {
            let tree: swayward_ipc::Node = serde_json::from_value(get_tree(f)).unwrap();
            tree.nodes[1].nodes[0].layout
        };
        assert_eq!(workspace_layout(&mut f), swayward_ipc::NodeLayout::SplitV);
        for command in ["focus parent", command] {
            let reply = crate::command::execute(f.niri_state(), command);
            assert!(reply[0].success, "{command}: {reply:?}");
        }
        assert_eq!(
            workspace_layout(&mut f),
            swayward_ipc::NodeLayout::SplitH,
            "{command}"
        );
    }
}

// differential seed 30071 (v3): sway lists hidden scratchpad containers in
// `root->scratchpad` order (sway/ipc-json.c:476-482), so a floating split
// hidden after a bare window comes after it, and `scratchpad show` takes the
// bottom of that list (sway/commands/scratchpad.c:64-71).
#[test]
fn hidden_scratchpad_split_follows_the_bare_window_hidden_before_it() {
    let (mut f, _) = ipc_fixture();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    map_test_window(&mut f, client, "first");
    assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);
    map_test_window(&mut f, client, "second");
    for command in ["floating enable", "splitv", "move scratchpad"] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success);
    }

    let tree = get_tree(&mut f);
    let scratch = tree["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|output| output["name"] == "__i3")
        .unwrap();
    let hidden = scratch["nodes"][0]["floating_nodes"].as_array().unwrap();
    assert_eq!(hidden.len(), 2, "{hidden:?}");
    assert_eq!(hidden[0]["app_id"], "first");
    assert_eq!(hidden[1]["layout"], "splitv");
    assert_eq!(hidden[1]["nodes"][0]["app_id"], "second");

    assert!(crate::command::execute(f.niri_state(), "scratchpad show")[0].success);
    let tree = get_tree(&mut f);
    assert!(find_json_node_with_app_id(&tree, "first").unwrap()["visible"] == true);
}

/// `resize set` on a view inside a floating split resizes it as a tiled child
/// of the split, ppt measured against the split (`resize_set_tiled`,
/// sway/commands/resize.c:286-336, reached because `container_is_floating`
/// is false for the child, resize.c:523). Differential seed 32562.
#[test]
fn resize_set_ppt_inside_a_shown_scratchpad_split_resizes_the_child() {
    let mut config = swayward_config::Config::default();
    config.animations.off = true;
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1270, 1408));
    let client = f.add_client();
    let map = |f: &mut Fixture, app: &str| {
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(app.into());
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    };
    map(&mut f, "a");
    for command in ["move scratchpad", "[app_id=a] focus", "splitv"] {
        assert!(
            crate::command::execute(f.niri_state(), command)[0].success,
            "{command}"
        );
        f.double_roundtrip(client);
    }
    map(&mut f, "b");
    assert!(
        crate::command::execute(f.niri_state(), "resize set width 30 ppt height 40 ppt")[0].success
    );

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
    let height = |node: &Value| node["rect"]["height"].as_i64().unwrap();
    let total = height(split);
    let (a, b) = (&split["nodes"][0], &split["nodes"][1]);
    // 40 ppt of the split's height; the rest goes to the sibling. The split
    // has no horizontal ancestor, so the width part changes nothing.
    assert_eq!(height(b), total * 40 / 100, "{split}");
    assert_eq!(height(a) + height(b), total, "{split}");
    assert_eq!(a["rect"]["width"], split["rect"]["width"], "{split}");
}

/// `focus parent` from a floating dialog focuses the workspace; `split v`
/// then wraps the workspace's tiling children and focuses the wrapper
/// (`workspace_split`, sway/tree/workspace.c:1058-1079), so `move scratchpad`
/// sends that split, not the dialog (sway/commands/move.c:921-949).
/// Differential family diff-fam-v3-hinted-split-move-scratchpad-border,
/// random-v3 seed 32482.
#[test]
fn move_scratchpad_after_focus_parent_split_from_a_dialog_hides_the_wrapper() {
    let mut config = swayward_config::Config::default();
    config.animations.off = true;
    let (mut f, _) = ipc_fixture_with_config(config);
    f.add_output(1, (1270, 1408));
    let client = f.add_client();
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
    let dialog = f.client(client).create_window();
    dialog.xdg_toplevel.set_app_id("dialog".into());
    dialog.set_parent(Some(&parent_toplevel));
    let dialog_surface = dialog.surface.clone();
    dialog.commit();
    f.roundtrip(client);
    let dialog = f.client(client).window(&dialog_surface);
    dialog.attach_new_buffer();
    dialog.ack_last_and_commit();
    f.double_roundtrip(client);

    for command in ["focus parent; split v", "move scratchpad"] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(outcome.iter().all(|o| o.success), "{command}: {outcome:?}");
        f.double_roundtrip(client);
    }
    let tree = get_tree(&mut f);
    let scratch = &tree["nodes"][0]["nodes"][0]["floating_nodes"];
    assert_eq!(scratch.as_array().unwrap().len(), 1, "{tree:#}");
    let hidden = &scratch[0];
    assert_eq!(hidden["border"], "none", "{hidden:#}");
    assert_eq!(hidden["current_border_width"], 0, "{hidden:#}");
    assert_eq!(hidden["layout"], "splitv", "{hidden:#}");
    assert_eq!(hidden["nodes"][0]["app_id"], "parent", "{hidden:#}");
    let dialog = find_json_node_with_app_id(&tree, "dialog").unwrap();
    assert_eq!(dialog["scratchpad_state"], "none", "{dialog:#}");
}

/// A shown scratchpad view taken global fullscreen goes back to the floating
/// layer when `focus` on a tiled view ends it: `container_fullscreen_disable`
/// leaves `container_is_floating` true (sway/commands/focus.c:389-394,
/// sway/tree/container.c:1246-1258). Differential seed 32873.
#[test]
fn focusing_a_tiled_view_ends_a_shown_scratchpad_global_fullscreen_back_to_floating() {
    let (mut f, socket) = ipc_fixture();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    map_test_window(&mut f, client, "pad");
    assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);
    map_test_window(&mut f, client, "tiled");
    for command in [
        r#"[app_id="pad"] scratchpad show"#,
        "fullscreen enable global",
        r#"[app_id="tiled"] focus"#,
    ] {
        assert!(
            crate::command::execute(f.niri_state(), command)[0].success,
            "{command}"
        );
    }

    let mut stream = UnixStream::connect(&socket).unwrap();
    let tree = query_ipc(&mut f, &mut stream, MessageType::GetTree);
    let pad = find_json_node_with_app_id(&tree, "pad").unwrap();
    assert_eq!(pad["type"], "floating_con");
    assert_eq!(pad["fullscreen_mode"], 0);
    let tiled = find_json_node_with_app_id(&tree, "tiled").unwrap();
    assert_eq!(tiled["focused"], true);
    assert_eq!(tiled["percent"], 1.0);
}
