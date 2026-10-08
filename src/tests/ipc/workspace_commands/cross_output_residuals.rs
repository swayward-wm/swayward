// Family diff-fam-v3-cross-output-residuals (random-v3 seeds 31139 31259
// 31279 31294 32249 32420). Each case replays the oracle state row named in
// its comment and checks the facts that row captured from sway 1.12.

/// Runs `steps` on a fresh fixture with one portrait output (the oracle's
/// single-output size) or two side-by-side 1280x720 outputs. `@id` maps a
/// view with that app id, `!command` is a command that must fail, and
/// anything else is a command that must succeed.
fn cross_output_tree(two_outputs: bool, steps: &[&str]) -> serde_json::Value {
    let (mut f, _) = ipc_fixture();
    if two_outputs {
        f.add_output(1, (1280, 720));
        f.add_output(2, (1280, 720));
    } else {
        f.add_output(1, (1270, 1408));
    }
    let client = f.add_client();
    for step in steps {
        if let Some(app_id) = step.strip_prefix('@') {
            windows::map_window(
                &mut f,
                client,
                windows::WindowSpec {
                    app_id: Some(app_id),
                    ..Default::default()
                },
            );
        } else if let Some(command) = step.strip_prefix('!') {
            let reply = crate::command::execute(f.niri_state(), command);
            assert!(!reply[0].success, "{command}: {reply:?}");
        } else {
            let reply = crate::command::execute(f.niri_state(), step);
            assert!(
                reply.iter().all(|outcome| outcome.success),
                "{step}: {reply:?}"
            );
        }
    }
    f.double_roundtrip(client);
    f.swayward().layout.verify_invariants();
    get_tree(&mut f)
}

fn cross_output_find<'a>(
    node: &'a serde_json::Value,
    matches: &dyn Fn(&serde_json::Value) -> bool,
) -> Option<&'a serde_json::Value> {
    if matches(node) {
        return Some(node);
    }
    ["nodes", "floating_nodes"]
        .iter()
        .flat_map(|key| node[*key].as_array().into_iter().flatten())
        .find_map(|child| cross_output_find(child, matches))
}

fn cross_output_view<'a>(tree: &'a serde_json::Value, app_id: &str) -> &'a serde_json::Value {
    cross_output_find(tree, &|node| node["app_id"] == app_id)
        .unwrap_or_else(|| panic!("no view {app_id}"))
}

fn cross_output_workspace<'a>(tree: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
    cross_output_find(tree, &|node| {
        node["type"] == "workspace" && node["name"] == name
    })
    .unwrap_or_else(|| panic!("no workspace {name}"))
}

/// The app ids of `node`'s tiling children, in order.
fn cross_output_child_apps(node: &serde_json::Value) -> Vec<String> {
    node["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|child| child["app_id"].as_str().unwrap_or("split").to_owned())
        .collect()
}

/// Seed 31139; oracle row map_into_failed_move_wrapper_keeps_empty_box. The
/// failed `move container to output left` wraps the workspace without
/// arranging it. The next view maps into that wrapper and
/// `arrange_container(parent)` lays it out at the wrapper's empty box
/// (sway/tree/view.c:936-937). A workspace fullscreen view then arranges
/// only itself (sway/tree/arrange.c:310-316), so the wrapper keeps its empty
/// box either way.
#[test]
fn view_mapped_into_a_failed_move_wrapper_keeps_its_empty_box() {
    for fullscreen in [false, true] {
        let mut steps = vec![
            "@one",
            "focus parent",
            "!move container to output left",
            "@two",
        ];
        if fullscreen {
            steps.push("fullscreen toggle");
        }
        let tree = cross_output_tree(false, &steps);
        let workspace = cross_output_workspace(&tree, "1");
        let wrapper = &workspace["nodes"][0];
        assert_eq!(
            cross_output_child_apps(wrapper),
            ["one", "two"],
            "{wrapper}"
        );
        assert_eq!(wrapper["percent"], 0.0, "fullscreen {fullscreen}");
        assert_eq!(wrapper["rect"]["width"], 0, "fullscreen {fullscreen}");
        for view in wrapper["nodes"].as_array().unwrap() {
            assert!(view["percent"].is_null(), "fullscreen {fullscreen}: {view}");
        }
    }
}

/// Seed 32249; oracle row directional_move_beside_fullscreen_keeps_focus. A
/// view moved right onto a workspace with a fullscreen view keeps the seat
/// focus and its border: `container_move_to_workspace_from_direction` never
/// calls `workspace_focus_fullscreen` (sway/commands/move.c:168-196). A
/// later `focus right` to a hidden sibling is refused
/// (sway/input/seat.c:1148-1151).
#[test]
fn view_moved_beside_a_fullscreen_view_keeps_focus() {
    let steps = [
        "@one",
        "focus output right",
        "@two",
        "for_window [app_id=\"three\"] fullscreen enable",
        "@three",
        "focus output left",
        "move right",
    ];
    for extra in [None, Some("focus right")] {
        let mut steps = steps.to_vec();
        steps.extend(extra);
        let tree = cross_output_tree(true, &steps);
        let workspace = cross_output_workspace(&tree, "2");
        assert_eq!(cross_output_child_apps(workspace), ["one", "two", "three"]);
        let one = cross_output_view(&tree, "one");
        assert_eq!(one["focused"], true, "{extra:?}");
        assert_eq!(one["border"], "normal");
        assert_eq!(one["percent"], 0.0);
        assert_eq!(cross_output_view(&tree, "three")["fullscreen_mode"], 1);
    }
}

/// Seeds 32420 and 31259; oracle rows directional_move_of_split_keeps_focus
/// and directional_move_of_split_onto_empty_workspace. A focused split moved
/// off the workspace edge onto another output stays focused there, and an
/// empty destination takes the split itself rather than its children
/// (`container_move_to_workspace_from_direction`, sway/commands/move.c:168-196).
#[test]
fn split_moved_to_another_output_keeps_focus_and_shape() {
    let tree = cross_output_tree(
        true,
        &[
            "@one",
            "focus output right",
            "@two",
            "focus parent; fullscreen toggle",
            "workspace 1",
            "focus output left",
            "split h",
            "move left",
        ],
    );
    let workspace = cross_output_workspace(&tree, "1");
    assert_eq!(cross_output_child_apps(workspace), ["one", "split"]);
    let split = &workspace["nodes"][1];
    assert_eq!(split["focused"], true);
    assert_eq!(split["layout"], "splith");
    assert_eq!(cross_output_child_apps(split), ["two"]);

    let tree = cross_output_tree(
        true,
        &[
            "focus output right",
            "@one",
            "@two",
            "move workspace to output right",
            "layout toggle splitv tabbed",
            "floating toggle",
            "move scratchpad",
            "move right",
        ],
    );
    let workspace = cross_output_workspace(&tree, "3");
    assert_eq!(workspace["layout"], "splith");
    assert_eq!(workspace["representation"], "H[V[one]]");
    let split = &workspace["nodes"][0];
    assert_eq!(split["focused"], true);
    assert_eq!(split["layout"], "splitv");
    assert_eq!(cross_output_child_apps(split), ["one"]);
}

/// Seed 31279; oracle row move_to_workspace_joins_focus_inactive_split. A
/// container moved to another output's workspace goes inside that
/// workspace's focus-inactive split (`seat_get_focus_inactive_tiling`,
/// `container_move_to_container`, sway/commands/move.c:516-517, 241-262).
#[test]
fn container_moved_to_a_workspace_joins_its_focus_inactive_split() {
    let tree = cross_output_tree(
        true,
        &[
            "@one",
            "focus output right",
            "@two",
            "mark --toggle m",
            "sticky enable; focus output right",
            "layout toggle",
            "[con_mark=\"m\"] focus",
            "focus parent",
            "move container to workspace 1",
        ],
    );
    let workspace = cross_output_workspace(&tree, "1");
    assert_eq!(workspace["representation"], "H[V[one H[two]]]");
}

/// Seeds 31294 and 32420; oracle rows float_workspace_view_keeps_focus_order,
/// float_wrapped_view_refocuses_view and
/// criteria_move_beside_floating_focus_keeps_order. Floating a
/// workspace-level view raises nothing (sway/tree/container.c:969-973), so a
/// later criteria move of the tiled views to their own workspace keeps the
/// split's order, and the focus stays on the view, never its split.
#[test]
fn floating_a_workspace_level_view_keeps_the_tiled_focus_order() {
    let tree = cross_output_tree(
        false,
        &[
            "@one",
            "@two",
            "layout tabbed; layout toggle",
            "@three",
            "move right",
            "floating enable",
            "[workspace=__focused__] move container to workspace 1",
        ],
    );
    let workspace = cross_output_workspace(&tree, "1");
    let inner = &workspace["nodes"][0]["nodes"][0];
    assert_eq!(cross_output_child_apps(inner), ["one", "two"]);
    let ids: Vec<_> = inner["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["id"].clone())
        .collect();
    assert_eq!(inner["focus"], serde_json::json!([ids[1], ids[0]]));
    assert_eq!(cross_output_view(&tree, "three")["focused"], true);

    let tree = cross_output_tree(
        false,
        &[
            "@one",
            "layout tabbed; layout toggle",
            "@two",
            "splith",
            "floating enable",
            "move container to workspace number 3",
        ],
    );
    assert_eq!(cross_output_view(&tree, "one")["focused"], true);
}
