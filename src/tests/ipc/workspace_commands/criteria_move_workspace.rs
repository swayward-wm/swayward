// Differential family diff-fam-v3-workspace-focused-criteria-move (random-v3
// seeds 40019 40115 40132 40221 40495 40204 40252 40446 40229 40460):
// criteria `move container to workspace` as sway's `cmd_move_container` runs
// it once per match (sway/commands/move.c:419-627). Oracle row
// workspace_focused_criteria_move.

fn criteria_move_tree(f: &mut Fixture) -> serde_json::Value {
    let swayward = f.swayward();
    serde_json::to_value(crate::ipc::tree::describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap()
}

fn criteria_move_workspace<'a>(tree: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
    tree["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|output| output["nodes"].as_array().unwrap())
        .find(|workspace| workspace["name"] == name)
        .unwrap_or_else(|| panic!("workspace {name}: {tree}"))
}

fn criteria_move_apps(workspace: &serde_json::Value) -> Vec<String> {
    workspace["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| node["app_id"].as_str().unwrap_or("split").to_owned())
        .collect()
}

/// `__focused__` compares against the focused view, so with a split focused
/// it matches nothing (`criteria_matches_view`, sway/criteria.c:193-197,
/// 453-465).
#[test]
fn focused_workspace_criteria_match_nothing_without_a_focused_view() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    map_test_window(&mut f, client, "one");
    map_test_window(&mut f, client, "two");
    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);

    let outcome = crate::command::execute(
        f.niri_state(),
        "[workspace=__focused__] move container to workspace 2",
    );
    assert!(!outcome[0].success, "{outcome:?}");
    assert_eq!(outcome[0].error.as_deref(), Some("No matching node."));
    let tree = criteria_move_tree(&mut f);
    assert_eq!(
        criteria_move_apps(criteria_move_workspace(&tree, "1")),
        ["one", "two"]
    );
}

/// On its own workspace a match moves after the focus-inactive tiling
/// container (move.c:516, 241-261); the focused view is its own destination
/// and stays.
#[test]
fn criteria_move_to_own_workspace_follows_the_focus_inactive_container() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    map_test_window(&mut f, client, "one");
    map_test_window(&mut f, client, "two");

    let outcome = crate::command::execute(
        f.niri_state(),
        "[workspace=__focused__] move container to workspace 1",
    );
    assert!(outcome[0].success, "{outcome:?}");
    let tree = criteria_move_tree(&mut f);
    let workspace = criteria_move_workspace(&tree, "1");
    assert_eq!(criteria_move_apps(workspace), ["two", "one"]);
    assert_eq!(workspace["nodes"][0]["focused"], true, "{workspace}");
}

/// A global fullscreen match is refused (move.c:438-441); the other match
/// still moves.
#[test]
fn criteria_move_refuses_a_global_fullscreen_match() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    map_test_window(&mut f, client, "one");
    map_test_window(&mut f, client, "two");
    for command in ["[app_id=\"^one$\"] focus", "fullscreen enable global"] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success);
    }

    let outcome = crate::command::execute(
        f.niri_state(),
        "[workspace=\"1\"] move container to workspace 2",
    );
    assert!(!outcome[0].success, "{outcome:?}");
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("Can't move fullscreen global container")
    );
    let tree = criteria_move_tree(&mut f);
    assert_eq!(
        criteria_move_apps(criteria_move_workspace(&tree, "1")),
        ["one"]
    );
    assert_eq!(
        criteria_move_apps(criteria_move_workspace(&tree, "2")),
        ["two"]
    );
}

/// A view moved into a split it lands in without focus does not raise the
/// split (`container_add_child`, sway/tree/container.c:1426-1438), so the
/// workspace's focus list keeps the split where it was. Random-v3 seed 40115.
#[test]
fn moved_view_does_not_raise_the_split_it_joins() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    assert!(crate::command::execute(f.niri_state(), "workspace 2")[0].success);
    map_test_window(&mut f, client, "two");
    assert!(crate::command::execute(f.niri_state(), "workspace 1")[0].success);
    map_test_window(&mut f, client, "three");
    for command in ["focus parent", "move container to workspace 2"] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success);
    }
    map_test_window(&mut f, client, "four");
    let before = criteria_move_workspace(&criteria_move_tree(&mut f), "2")["focus"].clone();

    assert!(crate::command::execute(f.niri_state(), "move container to workspace 2")[0].success);
    let tree = criteria_move_tree(&mut f);
    let workspace = criteria_move_workspace(&tree, "2");
    assert_eq!(criteria_move_apps(workspace), ["two", "split"]);
    assert_eq!(workspace["focus"], before, "{workspace}");
    let split = &workspace["nodes"][1];
    let apps = split["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| node["app_id"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(apps, ["three", "four"]);
    assert_eq!(split["focus"][0], split["nodes"][1]["id"], "{split}");
}
