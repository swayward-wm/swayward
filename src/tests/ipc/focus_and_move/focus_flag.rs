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
