// Differential family diff-fam-v3-move-to-floating-mark (random-v3 seeds 6,
// 24, 34, 70): a tiled container moved to a floating view's mark becomes
// floating, inserted into `workspace->floating` right after the marked view
// (`container_move_to_container`, sway/commands/move.c:243-261;
// `container_add_sibling`, sway/tree/container.c:1410-1423). Oracle rows
// move_tiled_to_floating_mark_joins_floating and
// move_tiled_to_floating_mark_stacks_above_mark.

fn floating_mark_tree(f: &mut Fixture) -> serde_json::Value {
    let swayward = f.swayward();
    serde_json::to_value(crate::ipc::tree::describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap()
}

fn floating_app_ids(workspace: &serde_json::Value) -> Vec<String> {
    workspace["floating_nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| node["app_id"].as_str().unwrap_or_default().to_owned())
        .collect()
}

fn run_ok(f: &mut Fixture, command: &str) {
    let outcome = crate::command::execute(f.niri_state(), command);
    assert!(outcome.iter().all(|o| o.success), "{command}: {outcome:?}");
}

#[test]
fn move_tiled_to_floating_mark_joins_floating() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    map_window(&mut f, client, "fixture-fmark-1");
    map_window(&mut f, client, "fixture-fmark-2");
    run_ok(&mut f, "floating enable");
    run_ok(&mut f, "mark m");
    run_ok(&mut f, r#"[app_id="^fixture-fmark-1$"] focus"#);
    run_ok(&mut f, "move container to mark m");

    let tree = floating_mark_tree(&mut f);
    let workspace = &tree["nodes"][1]["nodes"][0];
    assert_eq!(
        workspace["nodes"].as_array().unwrap().len(),
        0,
        "{workspace}"
    );
    assert_eq!(
        floating_app_ids(workspace),
        ["fixture-fmark-2", "fixture-fmark-1"]
    );
    let moved = &workspace["floating_nodes"][1];
    assert_eq!(moved["type"], "floating_con");
    assert_eq!(moved["focused"], true, "{moved}");
}

#[test]
fn move_tiled_to_floating_mark_stacks_above_mark() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    map_window(&mut f, client, "fixture-fmark-1");
    run_ok(&mut f, "splitv");
    map_window(&mut f, client, "fixture-fmark-2");
    map_window(&mut f, client, "fixture-fmark-3");
    run_ok(&mut f, "floating enable");
    run_ok(&mut f, "mark m");
    map_window(&mut f, client, "fixture-fmark-4");
    run_ok(&mut f, "floating enable");
    run_ok(&mut f, r#"[app_id="^fixture-fmark-2$"] focus"#);
    run_ok(&mut f, "move container to mark m");

    let tree = floating_mark_tree(&mut f);
    let workspace = &tree["nodes"][1]["nodes"][0];
    assert_eq!(workspace["representation"], "V[fixture-fmark-1]");
    assert_eq!(
        floating_app_ids(workspace),
        ["fixture-fmark-3", "fixture-fmark-2", "fixture-fmark-4"]
    );
    assert_eq!(workspace["floating_nodes"][1]["focused"], true);
    // The seat stack after the move: the moved view, then the floating
    // views by their last focus, then the tiled view.
    let app_of = |id: &serde_json::Value| {
        workspace["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .chain(workspace["floating_nodes"].as_array().unwrap())
            .find(|node| node["id"] == *id)
            .map(|node| node["app_id"].as_str().unwrap().to_owned())
            .unwrap()
    };
    let focus = workspace["focus"]
        .as_array()
        .unwrap()
        .iter()
        .map(app_of)
        .collect::<Vec<_>>();
    assert_eq!(
        focus,
        [
            "fixture-fmark-2",
            "fixture-fmark-4",
            "fixture-fmark-3",
            "fixture-fmark-1"
        ]
    );
}

#[test]
fn move_tiled_split_to_floating_mark_floats_the_split() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    map_window(&mut f, client, "fixture-fmark-1");
    map_window(&mut f, client, "fixture-fmark-2");
    run_ok(&mut f, "splitv");
    map_window(&mut f, client, "fixture-fmark-3");
    map_window(&mut f, client, "fixture-fmark-4");
    run_ok(&mut f, "floating enable");
    run_ok(&mut f, "mark m");
    run_ok(&mut f, r#"[app_id="^fixture-fmark-3$"] focus"#);
    run_ok(&mut f, "focus parent");
    run_ok(&mut f, "move container to mark m");

    let tree = floating_mark_tree(&mut f);
    let workspace = &tree["nodes"][1]["nodes"][0];
    assert_eq!(workspace["representation"], "H[fixture-fmark-1]");
    let floating = workspace["floating_nodes"].as_array().unwrap();
    assert_eq!(floating.len(), 2, "{workspace}");
    assert_eq!(floating[0]["app_id"], "fixture-fmark-4");
    assert_eq!(floating[1]["type"], "floating_con");
    assert_eq!(floating[1]["nodes"].as_array().unwrap().len(), 2);
    assert_eq!(floating[1]["focused"], true, "{}", floating[1]);
}

#[test]
fn move_tiled_to_floating_mark_on_another_workspace() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    map_window(&mut f, client, "fixture-fmark-1");
    run_ok(&mut f, "floating enable");
    run_ok(&mut f, "mark m");
    run_ok(&mut f, "workspace 2");
    map_window(&mut f, client, "fixture-fmark-2");
    map_window(&mut f, client, "fixture-fmark-3");
    run_ok(&mut f, "move container to mark m");

    let tree = floating_mark_tree(&mut f);
    let output = &tree["nodes"][1];
    let first = &output["nodes"][0];
    let second = &output["nodes"][1];
    assert_eq!(first["name"], "1");
    assert_eq!(
        floating_app_ids(first),
        ["fixture-fmark-1", "fixture-fmark-3"]
    );
    assert_eq!(second["representation"], "H[fixture-fmark-2]");
    assert_eq!(second["nodes"][0]["focused"], true);
}

#[test]
fn move_tiled_to_floating_mark_refreshes_a_fresh_workspace_representation() {
    // The floater reaches workspace 2 through `workspace_add_floating`, which
    // leaves its representation null (sway/sway/tree/workspace.c:960-970).
    // Moving a tiled view to its mark inserts it with `container_add_sibling`,
    // whose `container_update_representation` climbs to
    // `workspace_update_representation` (sway/sway/tree/container.c:750-773,
    // 1410-1423), so workspace 2 reports "H[]" from then on. Differential
    // seed 32909 (diff-fam-v3-recreated-workspace-representation-residual).
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    map_window(&mut f, client, "fixture-fmark-1");
    run_ok(&mut f, "floating enable");
    run_ok(&mut f, "mark z");
    run_ok(&mut f, "move container to workspace 2");
    map_window(&mut f, client, "fixture-fmark-2");

    let tree = floating_mark_tree(&mut f);
    let second = &tree["nodes"][1]["nodes"][1];
    assert_eq!(second["name"], "2");
    assert_eq!(second["representation"], serde_json::Value::Null);

    run_ok(&mut f, "move container to mark z");
    let tree = floating_mark_tree(&mut f);
    let workspaces = tree["nodes"][1]["nodes"].as_array().unwrap();
    let second = workspaces
        .iter()
        .find(|workspace| workspace["name"] == "2")
        .unwrap();
    assert_eq!(
        floating_app_ids(second),
        ["fixture-fmark-1", "fixture-fmark-2"]
    );
    assert_eq!(second["representation"], "H[]", "{second}");
    // The moved view was the seat's latest focus, so it leads workspace 2's
    // focus list ahead of the marked floater.
    let focus = second["focus"].as_array().unwrap();
    let ids: Vec<_> = second["floating_nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| node["id"].clone())
        .collect();
    assert_eq!(focus, &[ids[1].clone(), ids[0].clone()], "{second}");
}
