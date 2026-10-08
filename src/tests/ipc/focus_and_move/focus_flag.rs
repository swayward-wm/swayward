// Family diff-fam-v3-focused-flag-misc: which node GET_TREE marks `focused` after a
// command removes, floats or unfloats the focused container.

fn focus_flag_tree(f: &mut Fixture, commands: &[&str]) -> serde_json::Value {
    for command in commands {
        for outcome in crate::command::execute(f.niri_state(), command) {
            assert!(outcome.success, "{command}: {outcome:?}");
        }
    }
    focus_stack_tree(f)
}

/// Closing the only view under a focused split destroys the split, and the seat focuses
/// the most recent view under its parent rather than the workspace
/// (`handle_seat_node_destroy`, sway/input/seat.c:263-315). Seeds 40068, 40206.
#[test]
fn focus_flag_kill_focused_split_focuses_the_sibling_view() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    map_test_window(&mut f, client, "one");
    let two = map_focus_window(&mut f, client, "two");
    focus_flag_tree(&mut f, &["split toggle", "focus parent"]);
    close_focus_window(&mut f, client, &two);

    let tree = focus_stack_tree(&mut f);
    let workspace = focus_stack_workspace(&tree);
    assert_eq!(workspace["focused"], false);
    assert_eq!(workspace["nodes"][0]["app_id"], "one");
    assert_eq!(workspace["nodes"][0]["focused"], true);
}

/// Floating a view by criteria while the workspace is focused keeps the workspace focused:
/// `container_set_floating` only moves focus when the view was the seat focus
/// (sway/tree/container.c:946-975). Seeds 40011, 40075.
#[test]
fn focus_flag_criteria_float_under_focused_workspace_keeps_workspace_focus() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    map_test_window(&mut f, client, "two");
    let tree = focus_flag_tree(
        &mut f,
        &[
            "focus parent",
            r#"[app_id="^(two|three)$"] floating enable"#,
        ],
    );
    let workspace = focus_stack_workspace(&tree);
    assert_eq!(workspace["focused"], true);
    assert_eq!(workspace["floating_nodes"][0]["app_id"], "two");
    assert_eq!(workspace["floating_nodes"][0]["focused"], false);
}

/// Returning a focused floating split to tiling leaves the split focused
/// (`container_set_floating`, sway/tree/container.c:976-1011). Seeds 40172, 40370.
#[test]
fn focus_flag_unfloat_focused_floating_split_keeps_the_split_focused() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    map_test_window(&mut f, client, "nine");
    let tree = focus_flag_tree(
        &mut f,
        &["floating enable", "splith", "focus parent; floating toggle"],
    );
    let split = &focus_stack_workspace(&tree)["nodes"][0];
    assert_eq!(split["layout"], "splith");
    assert_eq!(split["focused"], true);
    assert_eq!(split["nodes"][0]["app_id"], "nine");
    assert_eq!(split["nodes"][0]["focused"], false);
}

/// Sending a fullscreen floating view to the scratchpad clears its fullscreen in place: the
/// view never leaves `ws->floating`, so the tiling focus stack is untouched and the view
/// under the split stays the focus-inactive view sway refocuses
/// (`root_scratchpad_add_container`, sway/tree/root.c:109-139). Seed 40350.
#[test]
fn focus_flag_scratchpad_fullscreen_floating_view_refocuses_the_tiled_view() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    map_test_window(&mut f, client, "four");
    map_test_window(&mut f, client, "five");
    let tree = focus_flag_tree(
        &mut f,
        &[
            "layout toggle split",
            "move scratchpad",
            "scratchpad show",
            "fullscreen toggle; move scratchpad",
        ],
    );
    let split = &focus_stack_workspace(&tree)["nodes"][0];
    assert_eq!(split["focused"], false);
    assert_eq!(split["nodes"][0]["app_id"], "four");
    assert_eq!(split["nodes"][0]["focused"], true);
}

// Family diff-fam-v3-focused-flag-residual-y1.

/// Unfloating a floating split whose view is focused tiles the split beside the
/// focus-inactive tiled view and keeps the view focused: `cmd_floating` walks up to the
/// split (sway/commands/floating.c:40-46) and `container_set_floating` touches focus only
/// when the split itself was focused (sway/tree/container.c:946-1011). Seed 32922.
#[test]
fn focus_flag_unfloat_floating_split_keeps_its_focused_view() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    map_test_window(&mut f, client, "one");
    map_test_window(&mut f, client, "two");
    let tree = focus_flag_tree(&mut f, &["floating toggle", "splith", "floating toggle"]);
    let workspace = focus_stack_workspace(&tree);
    assert_eq!(workspace["nodes"][0]["app_id"], "one");
    assert_eq!(workspace["nodes"][0]["focused"], false);
    assert_eq!(workspace["nodes"][1]["nodes"][0]["app_id"], "two");
    assert_eq!(workspace["nodes"][1]["nodes"][0]["focused"], true);
}

/// Moving a focused workspace wraps its children (sway/commands/move.c:430-436). The
/// wrapper joins the target workspace, and the view under it stays the most recent focus
/// there, so switching to that workspace focuses it rather than the target's own view
/// (`seat_get_focus_inactive`, sway/input/seat.c). Seed 32781.
#[test]
fn focus_flag_moved_workspace_wrapper_keeps_its_view_most_recent() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    map_test_window(&mut f, client, "one");
    focus_flag_tree(&mut f, &["move container to workspace 2"]);
    map_test_window(&mut f, client, "two");
    let tree = focus_flag_tree(
        &mut f,
        &[
            "focus parent",
            "move container to workspace next",
            "workspace 2",
        ],
    );
    let workspace = tree["nodes"][1]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|ws| ws["name"] == "2")
        .unwrap();
    assert_eq!(workspace["nodes"][0]["app_id"], "one");
    assert_eq!(workspace["nodes"][0]["focused"], false);
    assert_eq!(workspace["nodes"][1]["nodes"][0]["app_id"], "two");
    assert_eq!(workspace["nodes"][1]["nodes"][0]["focused"], true);
    // Focusing the view raises the wrapper above the workspace's own view.
    assert_eq!(focus_order_app_ids(workspace), ["con", "one"]);
}

/// A focused split moved to another workspace stays above its own view on the seat stack,
/// so switching there focuses the split (`workspace_switch`, sway/tree/workspace.c:731-743).
/// Seed 32781.
#[test]
fn focus_flag_moved_focused_split_is_refocused_on_switch() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    map_test_window(&mut f, client, "one");
    focus_flag_tree(&mut f, &["move container to workspace 2"]);
    map_test_window(&mut f, client, "two");
    map_test_window(&mut f, client, "three");
    let tree = focus_flag_tree(
        &mut f,
        &[
            "splitv",
            "focus parent",
            "move container to workspace next",
            "workspace 2",
        ],
    );
    let workspace = tree["nodes"][1]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|ws| ws["name"] == "2")
        .unwrap();
    let split = &workspace["nodes"][1];
    assert_eq!(split["nodes"][0]["app_id"], "three");
    assert_eq!(split["focused"], true);
    assert_eq!(split["nodes"][0]["focused"], false);
}
