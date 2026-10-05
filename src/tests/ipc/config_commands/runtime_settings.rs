fn add_two_tiled_windows(fixture: &mut Fixture) {
    let client = fixture.add_client();
    for app_id in ["left", "right"] {
        windows::map_window(
            fixture,
            client,
            windows::WindowSpec {
                app_id: Some(app_id),
                ..Default::default()
            },
        );
    }
}

fn tiled_window_rects(fixture: &mut Fixture) -> Vec<Value> {
    let swayward = fixture.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    fn collect(value: &Value, rects: &mut Vec<Value>) {
        if value["type"] == "con" && value["app_id"].is_string() {
            rects.push(value["rect"].clone());
        }
        for key in ["nodes", "floating_nodes"] {
            if let Some(children) = value[key].as_array() {
                for child in children {
                    collect(child, rects);
                }
            }
        }
    }
    let mut rects = Vec::new();
    collect(&tree, &mut rects);
    rects.sort_by_key(|rect| rect["x"].as_i64().unwrap());
    rects
}

/// Tiled window rects on one named workspace.
///
/// [`tiled_window_rects`] flattens every workspace, so a test that creates a
/// second workspace cannot use it: the two sets interleave under the sort.
fn tiled_window_rects_on(fixture: &mut Fixture, workspace: &str) -> Vec<Value> {
    let swayward = fixture.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    fn collect(value: &Value, rects: &mut Vec<Value>) {
        if value["type"] == "con" && value["app_id"].is_string() {
            rects.push(value["rect"].clone());
        }
        for key in ["nodes", "floating_nodes"] {
            if let Some(children) = value[key].as_array() {
                for child in children {
                    collect(child, rects);
                }
            }
        }
    }
    fn find_ws<'a>(value: &'a Value, name: &str) -> Option<&'a Value> {
        if value["type"] == "workspace" && value["name"] == name {
            return Some(value);
        }
        for key in ["nodes", "floating_nodes"] {
            if let Some(children) = value[key].as_array() {
                for child in children {
                    if let Some(found) = find_ws(child, name) {
                        return Some(found);
                    }
                }
            }
        }
        None
    }
    let ws = find_ws(&tree, workspace).expect("workspace not in tree");
    let mut rects = Vec::new();
    collect(ws, &mut rects);
    rects.sort_by_key(|rect| rect["x"].as_i64().unwrap());
    rects
}

#[test]
fn reloaded_gap_defaults_do_not_change_an_existing_workspace() {
    let initial = swayward_config::Config::parse_mem(
        r#"layout {
            gaps 10
            outer-gaps { left -2; right -2; top -2; bottom -2; }
            border { off; }
        }"#,
    )
    .unwrap();
    let (mut fixture, socket) = ipc_fixture_with_config(initial);
    fixture.add_output(1, (1280, 800));
    add_two_tiled_windows(&mut fixture);
    let before = tiled_window_rects(&mut fixture);
    assert_eq!(before[0]["x"], 8);
    assert_eq!(before[0]["y"], 8);
    assert_eq!(before[1]["y"], 8);

    let scratch = ScratchDir::new("gap-reload");
    let path = scratch.join("config.kdl");
    std::fs::write(
        &path,
        r#"layout {
            gaps 16
            outer-gaps { left -2; right -2; top -2; bottom -2; }
            border { off; }
        }"#,
    )
    .unwrap();
    crate::utils::watcher::setup(
        fixture.niri_state(),
        &swayward_config::ConfigPath::Explicit(path.clone()),
        Vec::new(),
    );
    let mut subscriber = UnixStream::connect(socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["workspace"]"#,
        ))
        .unwrap();
    let _ = read_ipc_reply(&mut fixture, &mut subscriber);

    assert!(crate::command::execute(fixture.niri_state(), "reload")[0].success);
    let (event_type, payload) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(event_type, EVENT_WORKSPACE);
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap(),
        serde_json::from_str::<Value>(&sway_fixture!("events/workspace.reload.json")).unwrap()
    );
    assert_eq!(tiled_window_rects(&mut fixture), before);
    assert!(crate::command::execute(fixture.niri_state(), "workspace reload-fresh")[0].success);
    add_two_tiled_windows(&mut fixture);
    assert_eq!(
        tiled_window_rects_on(&mut fixture, "reload-fresh")[0]["x"],
        14
    );
}

#[test]
fn runtime_gaps_all_changes_existing_workspaces() {
    let mut config = swayward_config::Config::default();
    config.layout.gaps = 10.;
    config.layout.outer_gaps = swayward_config::layout::OuterGaps::all(-2.);
    config.layout.border.off = true;
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 800));
    add_two_tiled_windows(&mut fixture);
    let before = tiled_window_rects(&mut fixture);

    assert!(crate::command::execute(fixture.niri_state(), "gaps inner all set 16")[0].success);
    let after = tiled_window_rects(&mut fixture);
    assert_ne!(after, before);
    assert_eq!(after[0]["x"], 14);
    assert_eq!(after[0]["y"], 14);
    assert_eq!(after[1]["y"], 14);
}

/// Sway's two-argument form sets the default for workspaces created later and
/// leaves existing ones alone (`sway/sway/commands/gaps.c:48-91`), because a
/// live workspace reads its own gaps, not the global default
/// (`sway/sway/tree/workspace.c:224-225`).
#[test]
fn gaps_defaults_form_does_not_disturb_an_existing_workspace() {
    let mut config = swayward_config::Config::default();
    config.layout.gaps = 10.;
    config.layout.outer_gaps = swayward_config::layout::OuterGaps::all(-2.);
    config.layout.border.off = true;
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 800));
    add_two_tiled_windows(&mut fixture);
    let before = tiled_window_rects(&mut fixture);
    assert_eq!(before[0]["x"], 8);

    assert!(crate::command::execute(fixture.niri_state(), "gaps inner 40")[0].success);

    // The existing workspace did not move, but a newly created one observes
    // the changed default.
    assert_eq!(tiled_window_rects(&mut fixture), before);
    assert!(crate::command::execute(fixture.niri_state(), "workspace fresh")[0].success);
    add_two_tiled_windows(&mut fixture);
    assert_eq!(tiled_window_rects_on(&mut fixture, "fresh")[0]["x"], 38);
}

/// The converse, and the other half of the "must not fight" requirement: the
/// runtime form mutates the live workspace and must leave the default alone, so
/// a workspace created afterwards still inherits the configured default.
#[test]
fn runtime_gaps_form_does_not_overwrite_the_defaults() {
    let mut config = swayward_config::Config::default();
    config.layout.gaps = 10.;
    config.layout.border.off = true;
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 800));
    add_two_tiled_windows(&mut fixture);

    assert!(crate::command::execute(fixture.niri_state(), "gaps inner current set 30")[0].success);

    // The live workspace moved.
    let after = tiled_window_rects(&mut fixture);
    assert_eq!(after[0]["x"], 30);
    // The default did not: prove it through a later workspace.
    assert!(crate::command::execute(fixture.niri_state(), "workspace fresh")[0].success);
    add_two_tiled_windows(&mut fixture);
    assert_eq!(tiled_window_rects_on(&mut fixture, "fresh")[0]["x"], 10);
}

/// Both directions in one session: setting the default, then a runtime change,
/// then another default write, must not let either clobber the other.
#[test]
fn gaps_defaults_and_runtime_forms_hold_separate_state() {
    let mut config = swayward_config::Config::default();
    config.layout.gaps = 10.;
    config.layout.border.off = true;
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 800));
    add_two_tiled_windows(&mut fixture);

    assert!(crate::command::execute(fixture.niri_state(), "gaps inner 25")[0].success);
    assert!(crate::command::execute(fixture.niri_state(), "gaps inner current set 5")[0].success);

    // Runtime change won for the live workspace; a new workspace observes the
    // default 25.
    assert_eq!(tiled_window_rects(&mut fixture)[0]["x"], 5);
    assert!(crate::command::execute(fixture.niri_state(), "workspace first-fresh")[0].success);
    add_two_tiled_windows(&mut fixture);
    assert_eq!(
        tiled_window_rects_on(&mut fixture, "first-fresh")[0]["x"],
        25
    );

    // A further default write still does not touch the original live
    // workspace, while another new workspace observes 50.
    assert!(crate::command::execute(fixture.niri_state(), "workspace 1")[0].success);
    assert!(crate::command::execute(fixture.niri_state(), "gaps inner 50")[0].success);
    assert_eq!(tiled_window_rects(&mut fixture)[0]["x"], 5);
    assert!(crate::command::execute(fixture.niri_state(), "workspace second-fresh")[0].success);
    add_two_tiled_windows(&mut fixture);
    assert_eq!(
        tiled_window_rects_on(&mut fixture, "second-fresh")[0]["x"],
        50
    );
}

/// `workspace <name> gaps <kind> <px>` is a per-workspace-name default applied
/// at creation (`sway/sway/tree/workspace.c:226-243`), so it must affect a
/// workspace of that name created afterwards and not the current one.
#[test]
fn workspace_gaps_apply_to_a_later_workspace_of_that_name() {
    let mut config = swayward_config::Config::default();
    config.layout.gaps = 10.;
    config.layout.border.off = true;
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 800));
    add_two_tiled_windows(&mut fixture);
    let before = tiled_window_rects(&mut fixture);

    assert!(
        crate::command::execute(fixture.niri_state(), "workspace roomy gaps inner 45")[0].success
    );

    // The current workspace is untouched, as in sway.
    assert_eq!(tiled_window_rects(&mut fixture), before);
    // A workspace with that name picks the value up.
    assert!(crate::command::execute(fixture.niri_state(), "workspace roomy")[0].success);
    add_two_tiled_windows(&mut fixture);
    assert_eq!(tiled_window_rects_on(&mut fixture, "roomy")[0]["x"], 45);

    // A differently named workspace still gets the global default.
    assert!(crate::command::execute(fixture.niri_state(), "workspace plain")[0].success);
    add_two_tiled_windows(&mut fixture);
    assert_eq!(tiled_window_rects_on(&mut fixture, "plain")[0]["x"], 10);
}

/// Sway finds a workspace config with strcmp (`workspace_find_config`,
/// sway/sway/tree/workspace.c:143-150), so `Web` and `web` keep separate
/// gaps. It also floors each set outer side at minus that config's inner gap
/// (`prevent_invalid_outer_gaps`, sway/sway/commands/workspace.c:38-55).
#[test]
fn workspace_gaps_are_per_exact_name_and_clamp_outer_to_inner() {
    let mut config = swayward_config::Config::default();
    config.layout.gaps = 10.;
    config.layout.border.off = true;
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 800));
    for command in ["workspace web gaps inner 5", "workspace Web gaps inner 30"] {
        assert!(
            crate::command::execute(fixture.niri_state(), command)[0].success,
            "{command}"
        );
    }

    // A workspace created as `Web` takes Web's inner gap, not web's.
    assert!(crate::command::execute(fixture.niri_state(), "workspace Web")[0].success);
    add_two_tiled_windows(&mut fixture);
    assert_eq!(tiled_window_rects_on(&mut fixture, "Web")[0]["x"], 30);

    assert!(
        crate::command::execute(fixture.niri_state(), "workspace Web gaps outer -100")[0].success
    );

    let workspaces = &fixture.swayward().config.borrow().workspaces;
    let gaps = |name: &str| {
        let layout = workspaces
            .iter()
            .find(|ws| ws.name.0 == name)
            .and_then(|ws| ws.layout.as_ref())
            .unwrap_or_else(|| panic!("no config for {name}"));
        (
            layout.0.gaps.map(|gaps| gaps.0),
            layout
                .0
                .outer_gaps
                .as_ref()
                .and_then(|outer| outer.top)
                .map(|top| top.0),
        )
    };
    assert_eq!(gaps("web"), (Some(5.), None));
    assert_eq!(gaps("Web"), (Some(30.), Some(-30.)));
}

fn workspace_rect_named(fixture: &mut Fixture, name: &str) -> Value {
    let swayward = fixture.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    fn find(value: &Value, name: &str) -> Option<Value> {
        if value["type"] == "workspace" && value["name"] == name {
            return Some(value["rect"].clone());
        }
        value["nodes"]
            .as_array()?
            .iter()
            .find_map(|child| find(child, name))
    }
    find(&tree, name).expect("workspace in tree")
}

/// Sway's workspace rect always carries the inner gap on each edge, even with
/// no outer gaps configured and no windows (`workspace_add_gaps`,
/// `sway/sway/tree/workspace.c:1024-1031`). Differential family
/// gaps-inner-empty-rect, oracle row `gaps_inner_current_empty_workspace_rect`.
#[test]
fn runtime_inner_gaps_inset_the_workspace_rect() {
    let mut fixture = Fixture::with_config(swayward_config::Config::default());
    fixture.add_output(1, (1270, 1408));

    assert!(crate::command::execute(fixture.niri_state(), "gaps inner current set 10")[0].success);
    assert_eq!(
        workspace_rect_named(&mut fixture, "1"),
        serde_json::json!({"x": 10, "y": 10, "width": 1250, "height": 1388})
    );

    // A window then tiles inside that rect, not inside a second inset.
    add_two_tiled_windows(&mut fixture);
    let rects = tiled_window_rects(&mut fixture);
    assert_eq!(rects[0]["x"], 10);
    assert_eq!(rects[0]["y"], 10);
}

/// Gaps belong to one workspace, so changing them on the focused workspace
/// leaves another workspace's rect alone (`configure_gaps` acts on one
/// `sway_workspace`, sway/sway/commands/gaps.c:112-137). Differential seeds
/// 1318 1676 1679 1876 1950 1492 1612.
#[test]
fn runtime_gaps_leave_another_workspace_rect_alone() {
    let mut fixture = Fixture::with_config(swayward_config::Config::default());
    fixture.add_output(1, (1270, 1408));
    add_two_tiled_windows(&mut fixture);
    assert!(crate::command::execute(fixture.niri_state(), "workspace number 3")[0].success);
    assert!(
        crate::command::execute(fixture.niri_state(), "gaps horizontal current toggle 8")[0]
            .success
    );

    assert_eq!(
        workspace_rect_named(&mut fixture, "1"),
        serde_json::json!({"x": 0, "y": 0, "width": 1270, "height": 1408})
    );
    assert_eq!(
        workspace_rect_named(&mut fixture, "3"),
        serde_json::json!({"x": 8, "y": 0, "width": 1254, "height": 1408})
    );
}

/// A fullscreen view's percent is its output box over the workspace box
/// (sway/sway/ipc-json.c:744-755), so inner gaps lift it above 1: sway 1.12
/// reports 1270*1408 / (1250*1388). Differential seeds 1054 and 1950.
#[test]
fn fullscreen_percent_is_measured_against_the_gapped_workspace() {
    let mut fixture = Fixture::with_config(swayward_config::Config::default());
    fixture.add_output(1, (1270, 1408));
    let client = fixture.add_client();
    windows::map_window(
        &mut fixture,
        client,
        windows::WindowSpec {
            app_id: Some("full"),
            ..Default::default()
        },
    );
    assert!(crate::command::execute(fixture.niri_state(), "fullscreen enable")[0].success);
    assert!(crate::command::execute(fixture.niri_state(), "gaps inner current set 10")[0].success);

    let swayward = fixture.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    let view = &tree["nodes"][1]["nodes"][0]["nodes"][0];
    assert_eq!(view["fullscreen_mode"], 1);
    assert_eq!(
        view["percent"].as_f64().unwrap(),
        1270. * 1408. / (1250. * 1388.)
    );
}

/// `smart_gaps on` drops the gaps only while one tiled view is visible; sway
/// re-runs `workspace_add_gaps` on every arrange
/// (sway/sway/tree/workspace.c:1007-1015), so floating that view brings the
/// workspace's gaps back. Differential seed 2611.
#[test]
fn smart_gaps_return_when_the_only_tiled_view_floats() {
    let mut fixture = Fixture::with_config(swayward_config::Config::default());
    fixture.add_output(1, (1270, 1408));
    assert!(crate::command::execute(fixture.niri_state(), "smart_gaps on")[0].success);
    let client = fixture.add_client();
    windows::map_window(
        &mut fixture,
        client,
        windows::WindowSpec {
            app_id: Some("solo"),
            ..Default::default()
        },
    );
    assert!(
        crate::command::execute(fixture.niri_state(), "gaps horizontal current toggle 8")[0]
            .success
    );
    assert_eq!(
        workspace_rect_named(&mut fixture, "1"),
        serde_json::json!({"x": 0, "y": 0, "width": 1270, "height": 1408})
    );

    assert!(crate::command::execute(fixture.niri_state(), "floating enable")[0].success);
    fixture.double_roundtrip(client);
    assert_eq!(
        workspace_rect_named(&mut fixture, "1"),
        serde_json::json!({"x": 8, "y": 0, "width": 1254, "height": 1408})
    );
}

/// An output mode change re-arranges the workspace, and sway's arrange re-runs
/// `workspace_add_gaps` (sway/sway/config/output.c:1090-1093,
/// sway/sway/tree/arrange.c:306), so smart gaps still suppress the inner gap
/// around a lone tiled view after the resize.
#[test]
fn smart_gaps_survive_an_output_resize() {
    let mut fixture = Fixture::with_config(swayward_config::Config::default());
    fixture.add_output(1, (1270, 1408));
    assert!(crate::command::execute(fixture.niri_state(), "smart_gaps on")[0].success);
    let client = fixture.add_client();
    windows::map_window(
        &mut fixture,
        client,
        windows::WindowSpec {
            app_id: Some("solo"),
            ..Default::default()
        },
    );
    assert!(crate::command::execute(fixture.niri_state(), "gaps inner current set 10")[0].success);
    assert_eq!(
        workspace_rect_named(&mut fixture, "1"),
        serde_json::json!({"x": 0, "y": 0, "width": 1270, "height": 1408})
    );

    let output = fixture.niri_output(1);
    output.change_current_state(
        Some(smithay::output::Mode {
            size: (1200, 1000).into(),
            refresh: 60_000,
        }),
        None,
        None,
        None,
    );
    fixture.swayward().output_resized(&output);
    assert_eq!(
        workspace_rect_named(&mut fixture, "1"),
        serde_json::json!({"x": 0, "y": 0, "width": 1200, "height": 1000})
    );
}

/// `gaps` ends in `arrange_workspace` (sway/sway/commands/gaps.c:136), which
/// lays a global fullscreen container out in its tile slot because it is not
/// `workspace->fullscreen` (sway/sway/tree/arrange.c:310-321). Differential
/// seed 2271.
#[test]
fn gaps_rearrange_a_global_fullscreen_view_into_its_slot() {
    let mut fixture = Fixture::with_config(swayward_config::Config::default());
    fixture.add_output(1, (1270, 1408));
    let client = fixture.add_client();
    windows::map_window(
        &mut fixture,
        client,
        windows::WindowSpec {
            app_id: Some("global"),
            ..Default::default()
        },
    );
    assert!(crate::command::execute(fixture.niri_state(), "fullscreen toggle global")[0].success);
    assert!(
        crate::command::execute(fixture.niri_state(), "gaps horizontal current toggle 8")[0]
            .success
    );

    let swayward = fixture.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    let view = &tree["nodes"][1]["nodes"][0]["nodes"][0];
    assert_eq!(view["fullscreen_mode"], 2);
    assert_eq!(
        view["rect"],
        serde_json::json!({"x": 8, "y": 0, "width": 1254, "height": 1408})
    );
    assert_eq!(view["percent"], 1.0);
}
