#[test]
fn ordinary_modified_keysym_bind_still_matches() {
    let config = swayward_config::Config::parse_mem(
        r#"binds { Super+Return { command "rename workspace to modified"; }; }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    type_key_chords(&mut fixture, &[&[133, 36]]);
    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("modified")
        .is_some());
}

#[test]
fn translated_keysym_uses_post_transition_consumed_modifiers() {
    let config = swayward_config::Config::parse_mem(
        r#"binds { Alt+at { command "rename workspace to translated"; }; }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    type_key_chords(&mut fixture, &[&[64, 50, 11]]);
    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("translated")
        .is_some());
}

#[test]
fn consumed_shift_still_matches_an_unchanged_raw_keysym() {
    let config = swayward_config::Config::parse_mem(
        r#"binds { Super+Shift+BackSpace { command "rename workspace to shifted-backspace"; }; }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    type_key_chords(&mut fixture, &[&[133, 50, 22]]);
    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("shifted-backspace")
        .is_some());
}

#[test]
fn bindcode_uses_the_xkb_keycode_from_real_input() {
    let config = swayward_config::Config::parse_mem(
        r#"binds { "code:39" { command "rename workspace to bindcode"; }; }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    key_event(&mut fixture, 39, true);
    key_event(&mut fixture, 39, false);

    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("bindcode")
        .is_some());
}

#[test]
fn release_key_binding_dispatches_only_on_release_through_real_input() {
    let config = swayward_config::Config::parse_mem(
        r#"binds { x release=true { command "rename workspace to released"; }; }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    key_event(&mut fixture, 53, true);
    assert_ne!(
        active_workspace_name(&mut fixture),
        Some("released".to_owned())
    );

    key_event(&mut fixture, 53, false);
    assert_eq!(
        active_workspace_name(&mut fixture),
        Some("released".to_owned())
    );
}

#[test]
fn another_key_cancels_a_held_release_binding_without_an_ipc_event() {
    let config = swayward_config::Config::parse_mem(
        r#"binds { x release=true { command "nop release"; }; }"#,
    )
    .unwrap();
    let (mut fixture, socket) = ipc_fixture();
    *fixture.swayward().config.borrow_mut() = config;
    fixture.add_output(1, (1280, 720));
    let mut subscriber = UnixStream::connect(socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["binding"]"#,
        ))
        .unwrap();
    let (_, reply) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(reply, r#"{"success": true}"#);

    key_event(&mut fixture, 53, true);
    key_event(&mut fixture, 52, true);
    key_event(&mut fixture, 53, false);
    key_event(&mut fixture, 52, false);
    fixture.swayward().ipc_server.as_ref().unwrap().send_event(
        swayward_ipc::legacy::Event::SwayBinding {
            command: "sentinel".into(),
            event_state_mask: vec![],
            input_codes: vec![],
            input_code: 0,
            symbols: vec!["t".into()],
            symbol: Some("t".into()),
            input_type: "keyboard".into(),
        },
    );

    let mut commands = Vec::new();
    loop {
        let (message_type, payload) = read_ipc_reply(&mut fixture, &mut subscriber);
        assert_eq!(message_type, EVENT_BINDING);
        let command = serde_json::from_str::<Value>(&payload).unwrap()["binding"]["command"]
            .as_str()
            .unwrap()
            .to_owned();
        commands.push(command.clone());
        if command == "sentinel" {
            break;
        }
    }
    assert_eq!(commands, ["sentinel"]);
}

#[test]
fn release_key_binding_survives_mode_change_after_press() {
    let config = swayward_config::Config::parse_mem(
        r#"binds {
            x { command "mode other"; }
            x release=true { command "workspace key-released"; }
        }
        mode "other" { y { command "nop"; }; }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    key_event(&mut fixture, 53, true);
    assert_eq!(fixture.swayward().binding_mode, "other");
    key_event(&mut fixture, 53, false);

    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("key-released")
        .is_some());
}

#[test]
fn release_key_binding_survives_config_reload_after_press() {
    let config = swayward_config::Config::parse_mem(
        r#"mode "held" { x release=true { command "workspace key-released"; }; }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));
    assert!(crate::command::execute(fixture.niri_state(), "mode held")[0].success);

    key_event(&mut fixture, 53, true);
    super::i3_conformance::reload_test_config(&mut fixture, "", "font monospace\n").unwrap();
    assert_eq!(fixture.swayward().binding_mode, "default");
    key_event(&mut fixture, 53, false);

    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("key-released")
        .is_some());
}

#[test]
fn release_key_binding_is_not_replaced_by_the_new_modes_binding() {
    let config = swayward_config::Config::parse_mem(
        r#"mode "held" { x release=true { command "workspace original-release"; }; }
        mode "other" { x release=true { command "workspace wrong-release"; }; }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));
    assert!(crate::command::execute(fixture.niri_state(), "mode held")[0].success);

    key_event(&mut fixture, 53, true);
    assert!(crate::command::execute(fixture.niri_state(), "mode other")[0].success);
    key_event(&mut fixture, 53, false);

    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("original-release")
        .is_some());
    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("wrong-release")
        .is_none());
}

#[test]
fn release_mouse_binding_survives_mode_change_after_press() {
    let config = swayward_config::Config::parse_mem(
        r#"binds {
            MouseLeft { command "mode other"; }
            MouseLeft release=true { command "workspace mouse-released"; }
        }
        mode "other" { MouseLeft release=true { command "workspace wrong-release"; }; }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    pointer_button(&mut fixture, 0x110, true);
    assert_eq!(fixture.swayward().binding_mode, "other");
    pointer_button(&mut fixture, 0x110, false);

    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("mouse-released")
        .is_some());
    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("wrong-release")
        .is_none());
}

#[test]
fn release_mouse_binding_dispatches_only_on_release() {
    let config = swayward_config::Config::parse_mem(
        r#"binds { MouseLeft release=true { command "rename workspace to released"; }; }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    pointer_button(&mut fixture, 0x110, true);
    assert_ne!(
        active_workspace_name(&mut fixture),
        Some("released".to_owned())
    );

    pointer_button(&mut fixture, 0x110, false);
    assert_eq!(
        active_workspace_name(&mut fixture),
        Some("released".to_owned())
    );
}

#[test]
fn pointer_button_event_dispatches_a_real_mouse_binding() {
    let config = swayward_config::Config::parse_mem(
        r#"binds {
            X { command "workspace startup"; }
            MouseLeft { command "workspace clicked"; }
        }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    pointer_button(&mut fixture, 0x110, true);
    pointer_button(&mut fixture, 0x110, false);

    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("clicked")
        .is_some());
}

#[test]
fn binding_modes_switch_binds_emit_events_and_list_over_ipc() {
    let config = swayward_config::Config::parse_mem(
        r#"binds { Super+R { command "mode resize"; }; }
        mode "resize" {
            Super+1 { command "workspace 7"; };
            Escape { command "mode default"; };
        }"#,
    )
    .unwrap();
    let (mut fixture, socket) = ipc_fixture();
    *fixture.swayward().config.borrow_mut() = config;
    fixture.add_output(1, (1920, 1080));
    let mut subscriber = UnixStream::connect(&socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["mode","binding"]"#,
        ))
        .unwrap();
    let (_, reply) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(reply, r#"{"success": true}"#);

    assert!(crate::command::execute(fixture.niri_state(), "mode resize")[0].success);
    let (event_type, payload) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(event_type, EVENT_MODE);
    let expected: Value = serde_json::from_str(&sway_fixture!("events/mode.resize.json")).unwrap();
    assert_event_shape(&expected, &serde_json::from_str(&payload).unwrap(), "$mode");

    type_key_chords(&mut fixture, &[&[133, 10]]);
    let (event_type, payload) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(event_type, EVENT_BINDING);
    let expected: Value = serde_json::from_str(&sway_fixture!("events/binding.run.json")).unwrap();
    assert_event_shape(
        &expected,
        &serde_json::from_str(&payload).unwrap(),
        "$binding",
    );

    // The bind switched to workspace 7, which is what this asserts. It is not
    // necessarily first: sway sorts numbered workspaces numerically
    // (sway/sway/tree/output.c:387-405), so the startup workspace 1 precedes it.
    let workspaces: Vec<swayward_ipc::Workspace> =
        serde_json::from_value(get_workspaces(&mut fixture)).unwrap();
    assert!(workspaces
        .iter()
        .any(|workspace| workspace.num == 7 && workspace.focused));

    assert!(crate::command::execute(fixture.niri_state(), "mode default")[0].success);
    let (event_type, payload) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(event_type, EVENT_MODE);
    let expected: Value = serde_json::from_str(&sway_fixture!("events/mode.default.json")).unwrap();
    assert_event_shape(&expected, &serde_json::from_str(&payload).unwrap(), "$mode");

    let mut query = UnixStream::connect(socket).unwrap();
    query
        .write_all(&swayward_ipc::wire::encode(
            MessageType::GetBindingModes,
            "",
        ))
        .unwrap();
    let (_, payload) = read_ipc_reply(&mut fixture, &mut query);
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap(),
        serde_json::json!(["default", "resize"])
    );

    query
        .write_all(&swayward_ipc::wire::encode(
            MessageType::GetBindingState,
            "",
        ))
        .unwrap();
    let (_, payload) = read_ipc_reply(&mut fixture, &mut query);
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap(),
        serde_json::json!({"name": "default"})
    );

    assert!(crate::command::execute(fixture.niri_state(), "mode resize")[0].success);
    query
        .write_all(&swayward_ipc::wire::encode(
            MessageType::GetBindingState,
            "",
        ))
        .unwrap();
    let (_, payload) = read_ipc_reply(&mut fixture, &mut query);
    let state = serde_json::from_str::<Value>(&payload).unwrap();
    assert_eq!(state, serde_json::json!({"name": "resize"}));
    assert_eq!(state.as_object().unwrap().len(), 1);
}

#[test]
fn runtime_mode_definition_with_set_creates_a_switchable_pango_mode() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (1920, 1080));
    let mut subscriber = UnixStream::connect(&socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["mode"]"#,
        ))
        .unwrap();
    let (_, reply) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(reply, r#"{"success": true}"#);

    let outcome = crate::command::execute(
        fixture.niri_state(),
        "mode --pango_markup created set $destination workspace-7",
    );
    assert!(outcome[0].success, "{outcome:?}");
    assert_eq!(fixture.swayward().binding_mode, "default");
    assert_eq!(
        fixture.swayward().sway_variables,
        [("$destination".into(), "workspace-7".into())]
    );
    let config = fixture.swayward().config.borrow();
    let created = config
        .binding_modes
        .iter()
        .find(|mode| mode.name == "created")
        .unwrap();
    assert!(created.pango_markup);
    assert!(created.binds.0.is_empty());
    drop(config);

    let mut query = UnixStream::connect(&socket).unwrap();
    query
        .write_all(&swayward_ipc::wire::encode(
            MessageType::GetBindingModes,
            "",
        ))
        .unwrap();
    let (_, payload) = read_ipc_reply(&mut fixture, &mut query);
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap(),
        serde_json::json!(["default", "created"])
    );

    let outcome = crate::command::execute(fixture.niri_state(), "mode created");
    assert!(outcome[0].success, "{outcome:?}");
    assert_eq!(fixture.swayward().binding_mode, "created");
    let (event_type, payload) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(event_type, EVENT_MODE);
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap(),
        serde_json::json!({"change":"created","pango_markup":true})
    );

    assert!(crate::command::execute(fixture.niri_state(), "workspace $destination")[0].success);
    assert_eq!(
        fixture
            .swayward()
            .layout
            .active_workspace()
            .unwrap()
            .sway_name(),
        Some("workspace-7".into())
    );

    let outcome = crate::command::execute(
        fixture.niri_state(),
        "mode inline set $next workspace-8; workspace $next",
    );
    assert!(outcome.iter().all(|result| result.success), "{outcome:?}");
    assert_eq!(fixture.swayward().binding_mode, "created");
    assert_eq!(
        fixture
            .swayward()
            .layout
            .active_workspace()
            .unwrap()
            .sway_name(),
        Some("workspace-8".into())
    );

    let outcome = crate::command::execute(fixture.niri_state(), "mode missing");
    assert!(!outcome[0].success);
    assert_eq!(outcome[0].error.as_deref(), Some("Unknown mode `missing'"));
}

/// Gesture binds are the only nested subcommands still refused, top level or
/// nested: swayward has no gesture command-binding table. The refusal happens
/// at parse time, so the named mode is not created either.
#[test]
fn runtime_mode_definition_rejects_only_gesture_binding_subcommands() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1920, 1080));

    for subcommand in ["bindgesture swipe:3:left nop", "unbindgesture swipe:3:left"] {
        let command = format!("mode blocked {subcommand}");
        let outcome = crate::command::execute(fixture.niri_state(), &command);
        assert!(!outcome[0].success, "{command}");
        assert_eq!(outcome[0].parse_error, Some(true), "{command}");
        assert_eq!(
            outcome[0].error.as_deref(),
            Some("gesture events have no sway command-binding model")
        );
    }
    assert!(fixture
        .swayward()
        .config
        .borrow()
        .binding_modes
        .iter()
        .all(|mode| mode.name != "blocked"));
}

/// `mode <name> bindsym` inserts into the named mode without switching to it
/// (`sway/sway/commands/mode.c:69-84`). The binding must fire once that mode
/// is active, stay silent in the default mode, and be discarded by reload.
#[test]
fn runtime_nested_mode_bindsym_fires_only_in_that_mode_and_is_discarded_by_reload() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1280, 720));

    let added = crate::command::execute(
        fixture.niri_state(),
        "mode nested bindsym Mod4+a workspace nested-bound",
    );
    assert!(added[0].success, "{added:?}");
    // The nested form defines the mode but does not enter it.
    assert_eq!(fixture.swayward().binding_mode, "default");

    // Inactive in the default mode.
    assert!(crate::command::execute(fixture.niri_state(), "workspace still-default")[0].success);
    press_mod_a(&mut fixture);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("still-default")
    );

    assert!(crate::command::execute(fixture.niri_state(), "mode nested")[0].success);
    press_mod_a(&mut fixture);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("nested-bound")
    );

    // Unbinding through the nested form removes it again.
    let removed = crate::command::execute(fixture.niri_state(), "mode nested unbindsym Mod4+a");
    assert!(removed[0].success, "{removed:?}");
    assert!(crate::command::execute(fixture.niri_state(), "workspace nested-unbound")[0].success);
    press_mod_a(&mut fixture);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("nested-unbound")
    );

    assert!(
        crate::command::execute(
            fixture.niri_state(),
            "mode nested bindsym Mod4+a workspace should-not-survive",
        )[0]
        .success
    );
    let config =
        swayward_config::Config::parse_mem(r#"mode "nested" { x { command "nop"; }; }"#).unwrap();
    fixture.niri_state().reload_config(Ok(config));
    assert!(crate::command::execute(fixture.niri_state(), "mode nested")[0].success);
    assert!(crate::command::execute(fixture.niri_state(), "workspace reload-check")[0].success);
    press_mod_a(&mut fixture);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("reload-check")
    );
}

/// The keycode and switch nested forms reach the same tables as their
/// top-level counterparts, and target the named mode rather than the active
/// one.
#[test]
fn runtime_nested_mode_bindcode_and_bindswitch_target_the_named_mode() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1280, 720));

    for command in [
        "mode nested bindcode 39 workspace nested-code",
        "mode nested bindswitch lid:on workspace nested-switch",
    ] {
        let outcome = crate::command::execute(fixture.niri_state(), command);
        assert!(outcome[0].success, "{command}: {outcome:?}");
    }
    assert_eq!(fixture.swayward().binding_mode, "default");

    // Neither fires in the default mode.
    assert!(crate::command::execute(fixture.niri_state(), "workspace default-still")[0].success);
    key_event(&mut fixture, 39, true);
    key_event(&mut fixture, 39, false);
    switch_event(
        &mut fixture,
        smithay::backend::input::Switch::Lid,
        smithay::backend::input::SwitchState::On,
    );
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("default-still")
    );

    assert!(crate::command::execute(fixture.niri_state(), "mode nested")[0].success);
    key_event(&mut fixture, 39, true);
    key_event(&mut fixture, 39, false);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("nested-code")
    );
    switch_event(
        &mut fixture,
        smithay::backend::input::Switch::Lid,
        smithay::backend::input::SwitchState::On,
    );
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("nested-switch")
    );

    for command in [
        "mode nested unbindcode 39",
        "mode nested unbindswitch lid:on",
    ] {
        let outcome = crate::command::execute(fixture.niri_state(), command);
        assert!(outcome[0].success, "{command}: {outcome:?}");
    }
    assert!(crate::command::execute(fixture.niri_state(), "workspace nested-unbound")[0].success);
    key_event(&mut fixture, 39, true);
    key_event(&mut fixture, 39, false);
    switch_event(
        &mut fixture,
        smithay::backend::input::Switch::Lid,
        smithay::backend::input::SwitchState::On,
    );
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("nested-unbound")
    );
}

/// A nested bind aimed at the mode the compositor is already in must not
/// leak into the default table, and a nested bind issued while a mode is
/// active must land in the named mode rather than the active one.
#[test]
fn runtime_nested_mode_bindsym_ignores_the_active_mode() {
    let config = swayward_config::Config::parse_mem(
        r#"
binds { Mod4+a { command "workspace default-mode"; }; }
mode "other" { Mod4+b { command "nop"; }; }
"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    assert!(crate::command::execute(fixture.niri_state(), "mode other")[0].success);
    let outcome = crate::command::execute(
        fixture.niri_state(),
        "mode elsewhere bindsym Mod4+a workspace elsewhere-bound",
    );
    assert!(outcome[0].success, "{outcome:?}");
    assert_eq!(fixture.swayward().binding_mode, "other");

    // The active mode did not receive the binding.
    assert!(crate::command::execute(fixture.niri_state(), "workspace untouched")[0].success);
    press_mod_a(&mut fixture);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("untouched")
    );

    // Neither did the default table.
    assert!(crate::command::execute(fixture.niri_state(), "mode default")[0].success);
    press_mod_a(&mut fixture);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("default-mode")
    );

    assert!(crate::command::execute(fixture.niri_state(), "mode elsewhere")[0].success);
    press_mod_a(&mut fixture);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("elsewhere-bound")
    );
}

#[test]
fn num_lock_does_not_modify_workspace_navigation_chord() {
    // Num Lock is state, not part of the overview chord, so overview keys must
    // still match when `input { keyboard { numlock } }` enables it by default.
    let config = swayward_config::Config::parse_mem(
        r#"input { keyboard { numlock; }; }
workspace "1" {}
workspace "2" {}"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));
    let client = fixture.add_client();

    for command in ["workspace 1", "workspace 2"] {
        assert!(crate::command::execute(fixture.niri_state(), command)[0].success);
        let window = fixture.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        fixture.roundtrip(client);
        let window = fixture.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        fixture.double_roundtrip(client);
    }

    let output = fixture.swayward().layout.active_output().unwrap().clone();
    let active_workspace_idx = |fixture: &mut Fixture| {
        fixture
            .swayward()
            .layout
            .monitor_for_output(&output)
            .unwrap()
            .active_workspace_idx()
    };

    assert!(fixture.swayward().layout.open_overview());
    fixture.niri_state().update_keyboard_focus();
    assert!(fixture.swayward().keyboard_focus.is_overview());

    // Assert directly on the predicate: the harness does not latch Num Lock
    // from a keycode, and going through key_event would silently test the
    // unlocked path instead.
    let locked = smithay::input::keyboard::ModifiersState {
        num_lock: true,
        ..Default::default()
    };
    assert!(
        crate::input::hardcoded_overview_bind(smithay::input::keyboard::Keysym::Up, locked)
            .is_some(),
        "a bare Up was rejected while Num Lock was on"
    );

    let before = active_workspace_idx(&mut fixture);
    key_event(&mut fixture, 111, true);
    key_event(&mut fixture, 111, false);
    fixture.swayward().clock.set_complete_instantly(true);
    fixture.swayward().layout.advance_animations();
    fixture.swayward().clock.set_complete_instantly(false);

    assert_ne!(
        active_workspace_idx(&mut fixture),
        before,
        "an overview arrow was rejected while Num Lock was on"
    );
}

/// Hold Super, launch an exclusive layer surface with a chord, release Super,
/// and return the depressed modifier mask the layer client ends up with.
fn depressed_mods_after_chord_launch(binds: &str) -> Option<u32> {
    use smithay::reexports::wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::Layer;
    use smithay::reexports::wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::KeyboardInteractivity;

    let config = swayward_config::Config::parse_mem(binds).unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));
    let client = fixture.add_client();
    fixture.double_roundtrip(client);

    // Super down, then `d` down and up: the chord that starts the launcher.
    key_event(&mut fixture, 133, true);
    key_event(&mut fixture, 40, true);
    key_event(&mut fixture, 40, false);

    // The launcher maps while Super is still held.
    let layer = fixture
        .client(client)
        .create_layer(None, Layer::Overlay, "launcher");
    let surface = layer.surface.clone();
    layer.set_configure_props(crate::tests::client::LayerConfigureProps {
        size: Some((100, 50)),
        kb_interactivity: Some(KeyboardInteractivity::Exclusive),
        ..Default::default()
    });
    layer.commit();
    fixture.double_roundtrip(client);
    let layer = fixture.client(client).layer(&surface);
    layer.attach_new_buffer();
    layer.set_size(100, 50);
    layer.ack_last_and_commit();
    fixture.double_roundtrip(client);
    assert_eq!(
        fixture
            .swayward()
            .keyboard_focus
            .surface()
            .map(|s| s.id().protocol_id()),
        Some(surface.id().protocol_id())
    );
    let held = fixture.client(client).state.keyboard_mods_depressed;
    assert_ne!(held.unwrap_or(0), 0, "the launcher entered with Super held");

    key_event(&mut fixture, 133, false);
    fixture.double_roundtrip(client);
    fixture.client(client).state.keyboard_mods_depressed
}

/// A focused client must learn that a modifier was released even when the
/// release itself is consumed by a bind on the bare modifier key. Otherwise a
/// launcher opened by `Super+d` believes Super is held and drops typed text.
#[test]
fn bare_modifier_bind_still_tells_the_focused_client_the_modifier_was_released() {
    let chord_only = r#"binds { Super+d { command "nop launch"; }; }"#;
    assert_eq!(depressed_mods_after_chord_launch(chord_only), Some(0));

    let with_bare_super = r#"binds {
        Super+d { command "nop launch"; }
        Super_L repeat=false { command "nop show"; }
        Super_L release=true { command "nop hide"; }
    }"#;
    assert_eq!(depressed_mods_after_chord_launch(with_bare_super), Some(0));
}

/// Map one window under `binds` and return it with its client once it holds
/// the keyboard and has no keyboard events recorded.
fn focused_window_with_binds(
    binds: &str,
) -> (
    Fixture,
    crate::tests::client::ClientId,
    smithay::reexports::wayland_server::protocol::wl_surface::WlSurface,
    wayland_client::protocol::wl_surface::WlSurface,
) {
    let config = swayward_config::Config::parse_mem(binds).unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));
    let client = fixture.add_client();
    let window = fixture.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    fixture.roundtrip(client);
    let window = fixture.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    fixture.double_roundtrip(client);

    let server_surface = fixture
        .swayward()
        .keyboard_focus
        .surface()
        .cloned()
        .unwrap();
    assert_eq!(
        server_surface.id().protocol_id(),
        surface.id().protocol_id(),
        "the window holds the keyboard"
    );
    let state = &mut fixture.client(client).state;
    state.keyboard_mods_events.clear();
    state.keyboard_key_events.clear();
    (fixture, client, server_surface, surface)
}

/// Press, then release, the left Super key, and return the client's depressed
/// masks after each, along with every key event the client received.
fn tap_super(
    fixture: &mut Fixture,
    client: crate::tests::client::ClientId,
) -> (Vec<u32>, Vec<u32>, Vec<u32>) {
    key_event(fixture, 133, true);
    fixture.double_roundtrip(client);
    let after_press = fixture.client(client).state.keyboard_mods_events.clone();
    key_event(fixture, 133, false);
    fixture.double_roundtrip(client);
    let state = &fixture.client(client).state;
    (
        after_press,
        state.keyboard_mods_events.clone(),
        state.keyboard_key_events.clone(),
    )
}

/// A client that already holds the keyboard learns about both sides of a bare
/// modifier bind through `wl_keyboard.modifiers` alone, as in sway: the bound
/// key's press and release are consumed, the modifier change is not
/// (sway/input/keyboard.c:693-705).
#[test]
fn bare_modifier_bind_reports_press_and_release_modifiers_without_key_events() {
    let (mut fixture, client, _, _) =
        focused_window_with_binds(r#"binds { Super_L { command "nop show"; }; }"#);

    let (after_press, after_release, keys) = tap_super(&mut fixture, client);
    assert_eq!(after_press, [64], "Mod4 is depressed after the bound press");
    assert_eq!(
        after_release,
        [64, 0],
        "Mod4 is released after the bound release"
    );
    assert_eq!(keys, Vec::<u32>::new(), "the bound key itself is not sent");
}

/// A forwarded modifier key carries its own modifiers event, so the client
/// gets exactly one per change; a bound key that does not change the
/// modifiers adds none.
#[test]
fn forwarded_modifier_and_bound_chord_send_no_extra_modifiers_events() {
    let (mut fixture, client, _, _) =
        focused_window_with_binds(r#"binds { Super+d { command "nop launch"; }; }"#);

    key_event(&mut fixture, 133, true);
    key_event(&mut fixture, 40, true);
    key_event(&mut fixture, 40, false);
    fixture.double_roundtrip(client);
    let state = &fixture.client(client).state;
    assert_eq!(state.keyboard_mods_events, [64]);
    assert_eq!(
        state.keyboard_key_events,
        [125],
        "only the Super press is sent"
    );

    key_event(&mut fixture, 133, false);
    fixture.double_roundtrip(client);
    let state = &fixture.client(client).state;
    assert_eq!(state.keyboard_mods_events, [64, 0]);
    assert_eq!(state.keyboard_key_events, [125, 125]);
}

/// A client holding an `xdg_popup.grab` keyboard grab learns about a bare
/// modifier bind like any focused client. Before any key is forwarded the
/// grab leaves the keyboard focus on the popup's root; `wl_keyboard.modifiers`
/// goes to the client that owns the focus, which is the grabbing client.
#[test]
fn bare_modifier_bind_reports_modifiers_to_a_client_with_a_popup_grab() {
    let (mut fixture, client, server_root, root) =
        focused_window_with_binds(r#"binds { Super_L { command "nop show"; }; }"#);

    let parent = fixture.client(client).window(&root).xdg_surface.clone();
    let seat = fixture.client(client).state.seat.clone().unwrap();
    let serial = fixture.client(client).state.keyboard_enter_serial.unwrap();
    let popup = fixture.client(client).create_popup(&parent);
    popup.xdg_popup.grab(&seat, serial);
    popup.surface.commit();
    fixture.double_roundtrip(client);
    assert!(
        fixture
            .swayward()
            .popup_grab
            .as_ref()
            .is_some_and(|grab| grab.has_keyboard_grab),
        "the popup holds a keyboard grab"
    );
    let keyboard = fixture.swayward().seat.get_keyboard().unwrap();
    assert!(keyboard.is_grabbed());
    assert_eq!(keyboard.current_focus(), Some(server_root));
    let state = &mut fixture.client(client).state;
    state.keyboard_mods_events.clear();
    state.keyboard_key_events.clear();

    let (after_press, after_release, keys) = tap_super(&mut fixture, client);
    assert_eq!(after_press, [64]);
    assert_eq!(after_release, [64, 0]);
    assert_eq!(keys, Vec::<u32>::new());
    assert!(
        fixture.swayward().popup_grab.is_some(),
        "the grab survives the bind"
    );
}
