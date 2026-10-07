// Family diff-fam-focus-singletons: where focus lands, and the focus order GET_TREE reports,
// after commands that each touch sway's seat focus stack in their own way.

fn focus_stack_tree(f: &mut Fixture) -> serde_json::Value {
    f.niri_state().ipc_refresh_layout();
    let swayward = f.swayward();
    serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap()
}

fn focus_stack_workspace(tree: &serde_json::Value) -> &serde_json::Value {
    &tree["nodes"][1]["nodes"][0]
}

/// App ids of `node`'s children in the order its `focus` list names them.
fn focus_order_app_ids(node: &serde_json::Value) -> Vec<String> {
    let children = node["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .chain(node["floating_nodes"].as_array().unwrap());
    let children = children.collect::<Vec<_>>();
    node["focus"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| {
            let child = children.iter().find(|child| child["id"] == *id).unwrap();
            child["app_id"].as_str().unwrap_or("con").to_owned()
        })
        .collect()
}

fn map_focus_window(
    f: &mut Fixture,
    client: super::client::ClientId,
    app_id: &str,
) -> wayland_client::protocol::wl_surface::WlSurface {
    windows::map_window(
        f,
        client,
        windows::WindowSpec {
            app_id: Some(app_id),
            ..Default::default()
        },
    )
}

/// Closes a window the way a client honouring `kill` does: it unmaps.
fn close_focus_window(
    f: &mut Fixture,
    client: super::client::ClientId,
    surface: &wayland_client::protocol::wl_surface::WlSurface,
) {
    let window = f.client(client).window(surface);
    window.attach_null();
    window.commit();
    f.double_roundtrip(client);
}

fn run_focus_commands(f: &mut Fixture, commands: &[&str]) {
    for command in commands {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(outcome[0].success, "{command}: {outcome:?}");
    }
}

/// A view that does not take focus joins the tail of sway's focus stack
/// (`seat_node_from_node`, sway/input/seat.c:349), behind every focused view.
/// Differential seeds 1066, 1451, 3225, 4822, 5595, 7235.
#[test]
fn focus_stack_no_focus_view_joins_the_tail_of_the_focus_stack() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    map_test_window(&mut f, client, "one");
    let two = map_focus_window(&mut f, client, "two");
    run_focus_commands(&mut f, &[r#"no_focus [app_id="three"]"#]);
    map_test_window(&mut f, client, "three");

    let tree = focus_stack_tree(&mut f);
    let workspace = focus_stack_workspace(&tree);
    assert_eq!(focus_order_app_ids(workspace), ["two", "one", "three"]);

    // Closing the focused view refocuses the next view on the stack, not the unfocused one.
    close_focus_window(&mut f, client, &two);
    let tree = focus_stack_tree(&mut f);
    assert_eq!(find_json_node(&tree, "con", true).unwrap()["app_id"], "one");
}

/// `focus tiling` leaves a fullscreen view fullscreen (sway/commands/focus.c:261-292).
/// Differential seed 1117.
#[test]
fn focus_stack_focus_tiling_keeps_fullscreen() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    map_test_window(&mut f, client, "one");
    run_focus_commands(&mut f, &["fullscreen enable", "focus tiling"]);

    let tree = focus_stack_tree(&mut f);
    let view = find_json_node(&tree, "con", true).unwrap();
    assert_eq!(view["app_id"], "one");
    assert_eq!(view["fullscreen_mode"], 1);
}

/// `focus next sibling` from a fullscreen view stops at the fullscreen container
/// (`node_get_in_direction_tiling`, sway/commands/focus.c:143-155). Seed 1840.
#[test]
fn focus_stack_focus_sibling_does_not_leave_a_fullscreen_view() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    map_test_window(&mut f, client, "one");
    run_focus_commands(&mut f, &["fullscreen enable"]);
    map_test_window(&mut f, client, "two");
    run_focus_commands(&mut f, &["focus next sibling"]);

    let tree = focus_stack_tree(&mut f);
    assert_eq!(find_json_node(&tree, "con", true).unwrap()["app_id"], "one");
}

/// `focus right` off the edge of a fullscreen split does not wrap inside it: reaching the
/// fullscreen container returns NULL with no other output, before the wrap candidate is used
/// (`node_get_in_direction_tiling`, sway/commands/focus.c:143-155). Family
/// diff-fam-focus-edge-in-fullscreen-split, seeds 15587 and 16845.
#[test]
fn focus_stack_focus_direction_does_not_wrap_inside_a_fullscreen_split() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    map_test_window(&mut f, client, "one");
    map_test_window(&mut f, client, "two");
    run_focus_commands(&mut f, &["fullscreen enable", "split h"]);
    map_test_window(&mut f, client, "three");
    run_focus_commands(&mut f, &["focus right"]);

    let tree = focus_stack_tree(&mut f);
    assert_eq!(
        find_json_node(&tree, "con", true).unwrap()["app_id"],
        "three"
    );
}

/// A sibling wrap candidate still descends to its focus-inactive view
/// (sway/commands/focus.c:216-220). Seed 2730.
#[test]
fn focus_stack_focus_sibling_wrap_descends_to_a_view() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    map_test_window(&mut f, client, "one");
    map_test_window(&mut f, client, "two");
    run_focus_commands(&mut f, &["move down 30 px", "focus next sibling"]);

    let tree = focus_stack_tree(&mut f);
    assert_eq!(find_json_node(&tree, "con", true).unwrap()["app_id"], "one");
}

/// `workspace` focuses the target's focus-inactive view even when the workspace itself was
/// focused (`workspace_switch`, sway/tree/workspace.c:731-743). Seed 1037.
#[test]
fn focus_stack_workspace_switch_focuses_the_focus_inactive_view() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    map_test_window(&mut f, client, "one");
    run_focus_commands(&mut f, &["focus parent", "workspace prev"]);

    let tree = focus_stack_tree(&mut f);
    assert_eq!(find_json_node(&tree, "con", true).unwrap()["app_id"], "one");
}

/// `floating disable` with the workspace focused wraps its children in a container, focuses
/// the wrapper and leaves it tiled (sway/commands/floating.c:28-33). Seed 1678.
#[test]
fn focus_stack_floating_disable_on_a_focused_workspace_wraps_its_children() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    run_focus_commands(&mut f, &["split v"]);
    map_test_window(&mut f, client, "one");
    run_focus_commands(&mut f, &["focus parent", "floating disable"]);

    // The wrapper keeps the workspace's vertical layout; the workspace turns horizontal.
    let tree = focus_stack_tree(&mut f);
    let workspace = focus_stack_workspace(&tree);
    assert_eq!(workspace["layout"], "splith");
    assert_eq!(workspace["representation"], "V[V[one]]");
    let wrapper = &workspace["nodes"][0];
    assert_eq!(wrapper["focused"], true);
    assert_eq!(wrapper["layout"], "splitv");
    assert_eq!(wrapper["nodes"][0]["app_id"], "one");
}

/// Closing a global fullscreen view leaves focus on the workspace: the seat refuses the
/// siblings it still obstructs while the destroy signal runs
/// (sway/tree/container.c:488-501; sway/input/seat.c:1148-1151). Seed 5002.
#[test]
fn focus_stack_closing_a_global_fullscreen_view_focuses_the_workspace() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    map_test_window(&mut f, client, "one");
    let two = map_focus_window(&mut f, client, "two");
    run_focus_commands(&mut f, &["fullscreen toggle global"]);
    close_focus_window(&mut f, client, &two);

    let tree = focus_stack_tree(&mut f);
    assert_eq!(focus_stack_workspace(&tree)["focused"], true);
}

/// A view assigned to a hidden workspace joins the tail of the focus stack, so switching
/// there focuses the view that was focused before it. Seed 11654.
#[test]
fn focus_stack_assigned_view_does_not_take_focus_on_a_hidden_workspace() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    map_test_window(&mut f, client, "two");
    run_focus_commands(
        &mut f,
        &[
            "move container to workspace 2",
            r#"assign [app_id="four"] workspace 2"#,
        ],
    );
    map_test_window(&mut f, client, "four");
    run_focus_commands(&mut f, &["workspace next"]);

    let tree = focus_stack_tree(&mut f);
    assert_eq!(find_json_node(&tree, "con", true).unwrap()["app_id"], "two");
}

/// Criteria `focus` focuses each match in turn, so every match rises on the focus stack in
/// match order (sway/commands.c:305-326). Seed 1844.
#[test]
fn focus_stack_criteria_focus_raises_every_match() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    map_test_window(&mut f, client, "two");
    run_focus_commands(&mut f, &["mark --add oracle-tiled"]);
    map_test_window(&mut f, client, "four");
    run_focus_commands(&mut f, &["mark --add oracle-2"]);
    map_test_window(&mut f, client, "six");
    run_focus_commands(&mut f, &[r#"[con_mark="oracle"] focus"#]);

    let tree = focus_stack_tree(&mut f);
    let workspace = focus_stack_workspace(&tree);
    assert_eq!(focus_order_app_ids(workspace), ["four", "two", "six"]);
}

/// Sending the focused floating view to the scratchpad refocuses the workspace's
/// focus-inactive node, the split container that the floated view left behind
/// (sway/tree/root.c:128-140; sway/tree/container.c:969-975). Seed 2137.
#[test]
fn focus_stack_scratchpad_refocuses_the_container_a_floated_view_left() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    map_test_window(&mut f, client, "one");
    map_test_window(&mut f, client, "two");
    run_focus_commands(&mut f, &["split v"]);
    map_test_window(&mut f, client, "four");
    run_focus_commands(&mut f, &["floating toggle", "move scratchpad"]);

    let tree = focus_stack_tree(&mut f);
    let focused = find_json_node(&tree, "con", true).unwrap();
    assert_eq!(focused["layout"], "splitv");
    assert_eq!(focused["nodes"][0]["app_id"], "two");
}

/// With a floating view focused, the other floating views keep their focus-stack order:
/// a floated view that was focused longer ago ranks behind a tiled one focused since
/// (sway/ipc-json.c:786-807). Seed 1415.
#[test]
fn focus_stack_floating_layer_follows_the_focus_stack() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    map_test_window(&mut f, client, "one");
    run_focus_commands(&mut f, &["floating enable"]);
    map_test_window(&mut f, client, "two");
    map_test_window(&mut f, client, "three");
    run_focus_commands(&mut f, &["floating enable"]);

    let tree = focus_stack_tree(&mut f);
    let workspace = focus_stack_workspace(&tree);
    assert_eq!(focus_order_app_ids(workspace), ["three", "two", "one"]);
}

/// An unfocused view mapped while only floating views exist leaves the workspace focused and
/// joins the tail of the focus stack (sway/tree/view.c:697-731, 944-957). Seed 5595.
#[test]
fn focus_stack_no_focus_view_keeps_a_focused_workspace() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    map_test_window(&mut f, client, "two");
    run_focus_commands(
        &mut f,
        &[
            "floating toggle",
            "focus parent",
            r#"no_focus [app_id="three"]"#,
        ],
    );
    map_test_window(&mut f, client, "three");

    let tree = focus_stack_tree(&mut f);
    let workspace = focus_stack_workspace(&tree);
    assert_eq!(workspace["focused"], true);
    assert_eq!(focus_order_app_ids(workspace), ["two", "three"]);
}

/// `layout tabbed` over a fullscreen view wraps the workspace children without focusing the
/// wrapper (`workspace_wrap_children`, sway/commands/layout.c:176-181). The wrapper is a new
/// node, so it joins the tail of the seat focus stack behind the floating view that was
/// mapped under the fullscreen view before it (`seat_node_from_node`,
/// sway/input/seat.c:349). Seed 4176.
#[test]
fn focus_stack_fresh_wrapper_ranks_behind_an_older_unfocused_view() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    map_test_window(&mut f, client, "one");
    run_focus_commands(
        &mut f,
        &[
            "fullscreen enable",
            r#"for_window [app_id="two"] floating enable"#,
        ],
    );
    map_test_window(&mut f, client, "two");
    run_focus_commands(&mut f, &["layout tabbed"]);

    let tree = focus_stack_tree(&mut f);
    let workspace = focus_stack_workspace(&tree);
    assert_eq!(focus_order_app_ids(workspace), ["two", "con"]);
}

/// Two outputs side by side, as the differential runner lays them out.
fn two_output_fixture() -> (Fixture, super::client::ClientId) {
    let mut f = Fixture::new();
    f.add_output_at(1, (1280, 720), Some((0, 0)));
    f.add_output_at(2, (1280, 720), Some((1280, 0)));
    let client = f.add_client();
    (f, client)
}

fn focused_workspace_name(tree: &serde_json::Value) -> String {
    fn walk(node: &serde_json::Value, workspace: Option<&str>) -> Option<String> {
        let workspace = if node["type"] == "workspace" {
            node["name"].as_str()
        } else {
            workspace
        };
        if node["focused"] == true {
            return workspace.map(str::to_owned);
        }
        node["nodes"]
            .as_array()
            .into_iter()
            .chain(node["floating_nodes"].as_array())
            .flatten()
            .find_map(|child| walk(child, workspace))
    }
    walk(tree, None).unwrap()
}

/// A view moved to another output without focus becomes that workspace's focus-inactive
/// view when it was focused more recently, so switching there focuses it
/// (sway/commands/move.c:583-608; sway/input/seat.c:1357-1372). Seeds 40110, 40160.
#[test]
fn focus_stack_view_moved_to_another_output_ranks_by_its_last_focus() {
    let (mut f, client) = two_output_fixture();
    map_test_window(&mut f, client, "one");
    run_focus_commands(&mut f, &["focus output right"]);
    map_test_window(&mut f, client, "two");
    run_focus_commands(
        &mut f,
        &["move container to output left", "focus output left"],
    );

    let tree = focus_stack_tree(&mut f);
    assert_eq!(find_json_node(&tree, "con", true).unwrap()["app_id"], "two");

    // A floating view ranks the same way against the tiled views it joins. Seed 40283.
    let (mut f, client) = two_output_fixture();
    run_focus_commands(&mut f, &["focus output right"]);
    map_test_window(&mut f, client, "two");
    run_focus_commands(&mut f, &["focus output left"]);
    map_test_window(&mut f, client, "one");
    run_focus_commands(
        &mut f,
        &[
            "floating enable",
            "move container to output right",
            "focus output right",
        ],
    );
    let tree = focus_stack_tree(&mut f);
    assert_eq!(
        find_json_node(&tree, "floating_con", true).unwrap()["app_id"],
        "one"
    );
}

/// Focus crossing into an output whose workspace holds only floating views focuses the
/// workspace, not a floater (`get_node_in_output_direction`, sway/commands/focus.c:93-135),
/// and a floating view never crosses outputs (sway/commands/focus.c:226-258, 457-460).
/// Seeds 40040, 40077.
#[test]
fn focus_stack_directional_focus_skips_floating_views_across_outputs() {
    let (mut f, client) = two_output_fixture();
    map_test_window(&mut f, client, "one");
    run_focus_commands(&mut f, &["floating enable", "focus right"]);
    let tree = focus_stack_tree(&mut f);
    assert_eq!(
        find_json_node(&tree, "floating_con", true).unwrap()["app_id"],
        "one"
    );

    run_focus_commands(&mut f, &["focus output right", "focus left"]);
    let tree = focus_stack_tree(&mut f);
    assert_eq!(focused_workspace_name(&tree), "1");
    assert!(find_json_node(&tree, "floating_con", true).is_none());
}

/// A `for_window` rule moves a mapping view before `should_focus` runs, so the view has never
/// had seat focus and sits at the tail of the stack, behind the workspace node it lands on
/// (sway/tree/view.c:943-957; `seat_node_from_node`, sway/input/seat.c:349). Focusing that
/// output lands on the workspace, not the view (`focus_output`, sway/commands/focus.c:330-333).
/// Seed 40077.
#[test]
fn focus_stack_view_moved_by_for_window_leaves_the_empty_destination_focused() {
    let (mut f, client) = two_output_fixture();
    run_focus_commands(
        &mut f,
        &[r#"for_window [app_id="moved"] move container to workspace 2"#],
    );
    map_test_window(&mut f, client, "moved");
    let tree = focus_stack_tree(&mut f);
    assert_eq!(focused_workspace_name(&tree), "1");

    run_focus_commands(&mut f, &["focus output left"]);
    let tree = focus_stack_tree(&mut f);
    let workspace = find_json_node(&tree, "workspace", true).unwrap();
    assert_eq!(workspace["name"], "2");

    // `workspace 2` still descends to the view (`workspace_switch`, sway/tree/workspace.c:736).
    run_focus_commands(&mut f, &["workspace 2"]);
    let tree = focus_stack_tree(&mut f);
    assert_eq!(
        find_json_node(&tree, "con", true).unwrap()["app_id"],
        "moved"
    );
}

/// A directional move across outputs leaves the seat's focus alone (sway/commands/move.c:
/// 277-298, 715-744). Focusing the view had raised its workspace right below it
/// (sway/input/seat.c:1178-1190), so once it leaves, the workspace node heads that
/// workspace's focus stack and `focus output` lands on it, not on the remaining view. A
/// `workspace` switch still descends to the view. Seed 40077.
#[test]
fn focus_stack_directional_move_to_another_output_leaves_the_source_workspace_focused() {
    let (mut f, client) = two_output_fixture();
    run_focus_commands(&mut f, &["focus output right"]);
    map_test_window(&mut f, client, "stays");
    map_test_window(&mut f, client, "moves");
    run_focus_commands(&mut f, &["move left", "move left", "focus output right"]);
    let tree = focus_stack_tree(&mut f);
    let workspace = find_json_node(&tree, "workspace", true).unwrap();
    assert_eq!(workspace["name"], "2");

    run_focus_commands(&mut f, &["workspace 1", "workspace 2"]);
    let tree = focus_stack_tree(&mut f);
    assert_eq!(
        find_json_node(&tree, "con", true).unwrap()["app_id"],
        "stays"
    );
}

/// `workspace back_and_forth` returns to the seat's previous workspace, which may be on
/// another output (sway/input/seat.c:1098-1113; sway/commands/workspace.c:215-222).
/// Seeds 40039, 40067, 40318, 40474.
#[test]
fn focus_stack_back_and_forth_follows_the_seat_across_outputs() {
    let (mut f, _) = two_output_fixture();
    run_focus_commands(&mut f, &["focus output right", "workspace back_and_forth"]);
    let tree = focus_stack_tree(&mut f);
    assert_eq!(focused_workspace_name(&tree), "1");

    // Moving the only workspace away leaves the seat on it, so there is no history yet.
    let (mut f, _) = two_output_fixture();
    run_focus_commands(&mut f, &["move workspace to output right"]);
    let outcome = crate::command::execute(f.niri_state(), "workspace back_and_forth");
    assert!(!outcome[0].success);
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("There is no previous workspace")
    );
}

/// A native workspace binding records the seat's previous workspace too, so auto
/// back-and-forth returns from it (`set_workspace`, sway/input/seat.c:1098-1113;
/// `workspace_auto_back_and_forth`, sway/tree/workspace.c:709-729).
#[test]
fn focus_stack_native_focus_workspace_auto_back_and_forth_returns() {
    use swayward_config::{Action, WorkspaceReference};

    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    map_test_window(&mut f, client, "one");
    run_focus_commands(&mut f, &["workspace 2"]);
    map_test_window(&mut f, client, "two");
    run_focus_commands(&mut f, &["workspace 1"]);
    f.swayward()
        .config
        .borrow_mut()
        .input
        .workspace_auto_back_and_forth = true;

    let mut names = Vec::new();
    for _ in 0..3 {
        f.niri_state()
            .do_action(Action::FocusWorkspace(WorkspaceReference::Index(2)), false);
        names.push(focused_workspace_name(&focus_stack_tree(&mut f)));
    }
    assert_eq!(names, ["2", "1", "2"]);
}

/// `splith` on the wrapper a workspace split left focused hits `container_split`'s singleton
/// branch: the workspace takes the layout and its representation is rebuilt, dropping the
/// stale one `workspace_split` left (sway/tree/container.c:1512-1527). Differential seed 30404.
#[test]
fn focus_stack_split_on_split_focused_workspace_refreshes_the_representation() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    map_test_window(&mut f, client, "one");
    run_focus_commands(&mut f, &["split v", "focus parent", "split h"]);
    let tree = focus_stack_tree(&mut f);
    assert_eq!(focus_stack_workspace(&tree)["representation"], "V[V[one]]");

    run_focus_commands(&mut f, &["splith"]);
    let tree = focus_stack_tree(&mut f);
    let workspace = focus_stack_workspace(&tree);
    assert_eq!(workspace["layout"], "splith");
    assert_eq!(workspace["representation"], "H[V[one]]");
    assert_eq!(workspace["nodes"][0]["focused"], true);
}
