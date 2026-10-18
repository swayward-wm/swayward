//! Containers in the floating layer (floating groups).

use super::*;

#[test]
fn sticky_floating_tree_follows_workspace_focus() {
    let mut layout = Layout::default();
    Op::AddOutput(1).apply(&mut layout);
    for id in 1..=2 {
        Op::AddWindow {
            params: TestWindowParams::new(id),
        }
        .apply(&mut layout);
    }
    let source = layout.active_workspace().unwrap().id();
    let workspace = layout.active_workspace_mut().unwrap();
    workspace.tiling_mut().focus_root();
    let root = workspace.tiling().focus().unwrap();
    let focused = workspace.tiling().node_for_window(&1).unwrap();
    workspace.tiling_mut().set_focus(focused);
    let (subtree, old_parent) = workspace.detach_tiling_subtree(root).unwrap();
    workspace.tiling_mut().finish_subtree_detach(old_parent);
    let (root, _) = workspace.floating_mut().add_tree(
        subtree,
        Rectangle::new((100., 120.).into(), (600., 450.).into()),
    );
    workspace.floating_mut().set_tree_sticky(root, true);

    layout
        .activate_sway_workspace(crate::command::WorkspaceTarget::Name("target".into()))
        .unwrap();

    let target = layout.active_workspace().unwrap();
    assert_eq!(target.floating_tree_root_for_window(&1), Some(root));
    assert_eq!(target.floating_tree_root_for_window(&2), Some(root));
    assert_eq!(target.floating().tree(root).unwrap().focus(), Some(focused));
    assert_ne!(target.id(), source);
}

/// A 1280x720 output named "output", ready to host a workspace.
pub(super) fn test_output() -> Output {
    let output = Output::new(
        "output".into(),
        PhysicalProperties {
            size: Size::from((1280, 720)),
            subpixel: Subpixel::Unknown,
            make: String::new(),
            model: String::new(),
            serial_number: String::new(),
        },
    );
    output.change_current_state(
        Some(Mode {
            size: Size::from((1280, 720)),
            refresh: 60000,
        }),
        None,
        None,
        None,
    );
    output.user_data().insert_if_missing(|| OutputName {
        connector: "output".into(),
        make: None,
        model: None,
        serial: None,
    });
    output
}

/// A workspace on `output` holding tiled windows `1..=count`, the last one
/// focused.
pub(super) fn workspace_with_tiled(output: Output, count: usize) -> Workspace<TestWindow> {
    let mut workspace = Workspace::new(
        output,
        Clock::with_time(Duration::ZERO),
        Rc::new(Options::default()),
    );
    for id in 1..=count {
        let tile = workspace.make_tile(TestWindow::new(TestWindowParams::new(id)));
        workspace.add_tile(
            tile,
            WorkspaceAddWindowTarget::Auto,
            super::workspace::AddTileOptions {
                activate: ActivateWindow::Yes,
                is_floating: false,
            },
        );
    }
    workspace
}

#[test]
fn floating_tree_entry_routes_geometry_focus_hit_testing_and_lifecycle() {
    let mut workspace = workspace_with_tiled(test_output(), 2);
    let first = workspace.tiling().node_for_window(&1).unwrap();
    workspace
        .tiling_mut()
        .set_layout(first, tiling_tree::Layout::SplitV);
    workspace.tiling_mut().focus_root();
    let tiling_root = workspace.tiling().focus().unwrap();
    workspace.tiling_mut().set_focus(first);
    let (subtree, old_parent) = workspace.detach_tiling_subtree(tiling_root).unwrap();
    workspace.tiling_mut().finish_subtree_detach(old_parent);
    let rect = Rectangle::new((100., 120.).into(), (600., 450.).into());

    let (root, remapped) = workspace.floating_mut().add_tree(subtree, rect);
    assert!(remapped.is_empty());
    // The emptied workspace root keeps its ID; the floated split gets its own.
    assert_ne!(root, tiling_root);
    assert!(workspace.tiling().is_root(tiling_root));
    assert_eq!(workspace.floating().tree(root).unwrap().parent_area(), rect);
    assert_eq!(
        workspace.floating().tree(root).unwrap().geometry(first),
        Some(Rectangle::new((100., 120.).into(), (300., 450.).into()))
    );
    assert!(workspace
        .floating_mut()
        .tree_mut(root)
        .unwrap()
        .focus_parent());
    let parent = workspace.floating().tree(root).unwrap().focus().unwrap();
    assert_ne!(parent, first);
    assert!(workspace
        .floating()
        .tree(root)
        .unwrap()
        .contains_node(root, parent));

    let detached = workspace.floating_mut().remove_tree(root).unwrap();
    let (restored, remapped) = workspace.attach_tiling_subtree(detached);
    assert_eq!(restored, tiling_root);
    assert_eq!(remapped, vec![(root, tiling_root)]);
    assert_eq!(workspace.tiling().windows().count(), 2);
    workspace.verify_invariants(None);
}

#[test]
fn moving_the_only_child_of_a_floating_group_keeps_the_root_position() {
    let mut workspace = workspace_with_tiled(test_output(), 2);
    let second = workspace.tiling().node_for_window(&2).unwrap();
    workspace
        .tiling_mut()
        .split(second, tiling_tree::Layout::SplitV);
    workspace.tiling_mut().focus_parent();
    let group = workspace.tiling().focus().unwrap();
    workspace.set_container_floating(group, true).unwrap();
    workspace.focus_child();

    assert!(!workspace.is_floating(&2));
    let root = workspace.floating_tree_root_for_window(&2).unwrap();
    let before = workspace.floating().tree_rect(root).unwrap();
    assert!(workspace
        .floating()
        .focused_leaf_is_only_child_of_tree_root());

    workspace.move_window_in_direction(&2, tiling_tree::Direction::Right, 10.);

    assert_eq!(workspace.floating().tree_rect(root), Some(before));
    assert_eq!(workspace.tiling().windows().count(), 1);
    workspace.verify_invariants(None);
}

#[test]
fn directional_move_reorders_a_floating_group_child() {
    let mut workspace = workspace_with_tiled(test_output(), 3);
    let second = workspace.tiling().node_for_window(&2).unwrap();
    workspace.tiling_mut().set_focus(second);
    workspace.tiling_mut().focus_root();
    let group = workspace.tiling().focus().unwrap();
    workspace.tiling_mut().set_focus(second);
    let root = workspace.set_container_floating(group, true).unwrap();
    let before = workspace.floating().tree_rect(root).unwrap();

    assert!(workspace.move_window_in_direction(&2, tiling_tree::Direction::Left, 10.));

    let tree = workspace.floating().tree(root).unwrap();
    let first = tree.node_for_window(&1).unwrap();
    let second = tree.node_for_window(&2).unwrap();
    assert!(tree.geometry(second).unwrap().loc.x < tree.geometry(first).unwrap().loc.x);
    assert_eq!(workspace.floating().tree_rect(root), Some(before));
    workspace.verify_invariants(None);
}

#[test]
fn removing_a_floating_tree_leaf_uses_the_resident_tree() {
    let mut workspace = workspace_with_tiled(test_output(), 2);
    workspace.tiling_mut().focus_root();
    let root = workspace.tiling().focus().unwrap();
    let (subtree, old_parent) = workspace.detach_tiling_subtree(root).unwrap();
    workspace.tiling_mut().finish_subtree_detach(old_parent);
    workspace.floating_mut().add_tree(
        subtree,
        Rectangle::new((100., 120.).into(), (600., 450.).into()),
    );

    let removed = workspace.remove_tile(&1, Transaction::new());

    assert_eq!(removed.tile.window().id(), &1);
    assert!(workspace.floating().has_window(&2));
    assert!(!workspace.floating().has_window(&1));
    workspace.verify_invariants(None);
}

#[test]
fn floating_tree_root_keeps_its_position_when_the_output_resizes() {
    let output = test_output();
    let mut workspace = workspace_with_tiled(output.clone(), 2);
    workspace.tiling_mut().focus_root();
    let root = workspace.tiling().focus().unwrap();
    let (subtree, old_parent) = workspace.detach_tiling_subtree(root).unwrap();
    workspace.tiling_mut().finish_subtree_detach(old_parent);
    let (root, _) = workspace.floating_mut().add_tree(
        subtree,
        Rectangle::new((100., 120.).into(), (600., 450.).into()),
    );

    // Sway's arrange_workspace moves floaters only when the workspace origin
    // moves (sway/sway/tree/arrange.c:277-304).
    let area = Rectangle::from_size((2560., 1440.).into());
    workspace.floating_mut().update_config(
        (2560., 1440.).into(),
        area,
        area,
        Some((0., 0.).into()),
        1.,
        Rc::new(Options::default()),
    );
    assert_eq!(
        workspace.floating().tree_rect(root),
        Some(Rectangle::new((100., 120.).into(), (600., 450.).into()))
    );

    // A moved origin keeps the center at the same fraction of the box
    // (floating_fix_coordinates, sway/sway/tree/container.c:818-831).
    let moved = Rectangle::new((0., 40.).into(), (2560., 1400.).into());
    workspace.floating_mut().update_config(
        (2560., 1440.).into(),
        moved,
        moved,
        Some((0., 0.).into()),
        1.,
        Rc::new(Options::default()),
    );
    let center_y = 40. + (120. + 225.) * 1400. / 1440.;
    assert_eq!(
        workspace.floating().tree_rect(root),
        Some(Rectangle::new(
            (100., center_y - 225.).into(),
            (600., 450.).into()
        ))
    );
    workspace.floating().verify_invariants();
}

pub(super) fn floating_group_workspace() -> (Workspace<TestWindow>, tiling_tree::NodeId) {
    let mut workspace = workspace_with_tiled(test_output(), 2);
    workspace.tiling_mut().focus_root();
    let root = workspace.tiling().focus().unwrap();
    let (subtree, old_parent) = workspace.detach_tiling_subtree(root).unwrap();
    workspace.tiling_mut().finish_subtree_detach(old_parent);
    let (root, _) = workspace.floating_mut().add_tree(
        subtree,
        Rectangle::new((100., 120.).into(), (600., 450.).into()),
    );
    (workspace, root)
}

#[test]
fn sticky_targets_only_the_floating_group_child() {
    let (mut workspace, root) = floating_group_workspace();

    assert!(workspace.set_window_sticky(&2, true));

    assert!(workspace.is_window_sticky(&2));
    assert!(!workspace.is_window_sticky(&1));
    assert!(!workspace.floating().tree_is_sticky(root));
    assert!(workspace.take_sticky_tiles().is_empty());
    assert_eq!(workspace.floating_tree_root_for_window(&2), Some(root));
}

#[test]
fn swap_targets_children_inside_the_same_floating_group() {
    let (mut workspace, root) = floating_group_workspace();
    let tree = workspace.floating().tree(root).unwrap();
    let first = tree.node_for_window(&1).unwrap();
    let second = tree.node_for_window(&2).unwrap();

    workspace.swap_tiling_nodes(first, second).unwrap();

    let children = &workspace.floating().tree(root).unwrap().ipc_tree();
    let tiling_tree::IpcNode::Split { children, .. } = children else {
        panic!("floating group root must remain a split");
    };
    assert!(matches!(
        &children[..],
        [
            tiling_tree::IpcNode::Leaf { window: 2, .. },
            tiling_tree::IpcNode::Leaf { window: 1, .. }
        ]
    ));
}

#[test]
fn fullscreen_targets_a_node_inside_a_floating_tree() {
    let mut workspace = workspace_with_tiled(test_output(), 2);
    workspace.tiling_mut().focus_root();
    let root = workspace.tiling().focus().unwrap();
    let (subtree, old_parent) = workspace.detach_tiling_subtree(root).unwrap();
    workspace.tiling_mut().finish_subtree_detach(old_parent);
    let (root, _) = workspace.floating_mut().add_tree(
        subtree,
        Rectangle::new((100., 120.).into(), (600., 450.).into()),
    );
    workspace.activate_window(&1);
    workspace.floating_mut().focus_parent();

    assert!(workspace.set_focused_fullscreen(Some(tiling_tree::FullscreenMode::Workspace)));
    assert_eq!(
        workspace.floating().tree(root).unwrap().fullscreen_node(),
        Some(root)
    );
    assert_eq!(
        workspace.fullscreen_mode(),
        Some(tiling_tree::FullscreenMode::Workspace)
    );
    assert!(workspace.fullscreen_contains_window(&1));
    assert_eq!(workspace.fullscreen_window(), Some(&1));

    assert!(workspace.set_focused_fullscreen(Some(tiling_tree::FullscreenMode::Global)));
    assert_eq!(
        workspace.fullscreen_mode(),
        Some(tiling_tree::FullscreenMode::Global)
    );
    workspace.disable_fullscreen();
    assert_eq!(workspace.fullscreen_mode(), None);

    assert!(workspace.set_focused_fullscreen(Some(tiling_tree::FullscreenMode::Workspace)));
    assert!(workspace.set_focused_fullscreen(None));
    workspace.floating_mut().focus_child();
    assert!(workspace.set_focused_fullscreen(Some(tiling_tree::FullscreenMode::Workspace)));
    assert_eq!(
        workspace.floating().tree(root).unwrap().fullscreen_node(),
        workspace.floating().tree(root).unwrap().node_for_window(&1)
    );
    assert_eq!(
        workspace
            .floating()
            .tree(root)
            .unwrap()
            .tiles_with_render_positions()
            .map(|(tile, _, visible)| (*tile.window().id(), visible))
            .collect::<Vec<_>>(),
        vec![(1, true), (2, false)]
    );
}

#[test]
fn floating_tree_root_survives_workspace_and_output_moves() {
    let output = test_output();
    let mut source = workspace_with_tiled(output.clone(), 2);
    let first = source.tiling().node_for_window(&1).unwrap();
    source.tiling_mut().focus_root();
    let root = source.tiling().focus().unwrap();
    source.tiling_mut().set_focus(first);
    let (subtree, old_parent) = source.detach_tiling_subtree(root).unwrap();
    source.tiling_mut().finish_subtree_detach(old_parent);
    let old_rect = Rectangle::new((100., 120.).into(), (600., 450.).into());
    let (root, _) = source.floating_mut().add_tree(subtree, old_rect);
    source
        .floating_mut()
        .tree_mut(root)
        .unwrap()
        .set_fullscreen(&1, true);
    source.floating_mut().set_tree_sticky(root, true);
    let node_ids = source
        .floating()
        .tree(root)
        .unwrap()
        .iter_depth_first()
        .map(|(id, _)| id)
        .collect::<Vec<_>>();

    let removed = source.remove_floating_tree(root).unwrap();
    assert!(source.floating().is_empty());

    let target_output = Output::new(
        "target".into(),
        PhysicalProperties {
            size: Size::from((2560, 1440)),
            subpixel: Subpixel::Unknown,
            make: String::new(),
            model: String::new(),
            serial_number: String::new(),
        },
    );
    target_output.change_current_state(
        Some(Mode {
            size: Size::from((2560, 1440)),
            refresh: 60000,
        }),
        None,
        None,
        None,
    );
    target_output.user_data().insert_if_missing(|| OutputName {
        connector: "target".into(),
        make: None,
        model: None,
        serial: None,
    });
    let mut target = Workspace::new(
        target_output,
        Clock::with_time(Duration::ZERO),
        Rc::new(Options::default()),
    );
    let restored = target.add_floating_tree(removed, true);
    let tree = target.floating().tree(restored).unwrap();

    assert_eq!(restored, root);
    assert_eq!(tree.focus(), Some(first));
    assert_eq!(
        tree.iter_depth_first()
            .map(|(id, _)| id)
            .collect::<Vec<_>>(),
        node_ids
    );
    assert_eq!(tree.fullscreen_node(), tree.node_for_window(&1));
    assert!(target.floating().tree_is_sticky(restored));
    assert_eq!(
        target.floating().tree_rect(restored),
        Some(Rectangle::new((500., 465.).into(), (600., 450.).into()))
    );
    target.verify_invariants(None);
}

#[test]
fn directional_focus_descends_into_a_floating_tree() {
    let mut layout = Layout::default();
    Op::AddOutput(1).apply(&mut layout);
    for id in 1..=2 {
        Op::AddWindow {
            params: TestWindowParams::new(id),
        }
        .apply(&mut layout);
    }
    let workspace = layout.active_workspace_mut().unwrap();
    workspace.tiling_mut().focus_root();
    let root = workspace.tiling().focus().unwrap();
    let first = workspace.tiling().node_for_window(&1).unwrap();
    workspace.tiling_mut().set_focus(first);
    let (subtree, old_parent) = workspace.detach_tiling_subtree(root).unwrap();
    workspace.tiling_mut().finish_subtree_detach(old_parent);
    workspace.floating_mut().add_tree(
        subtree,
        Rectangle::new((100., 120.).into(), (600., 450.).into()),
    );

    assert!(workspace.floating_mut().focus_right());
    assert_eq!(workspace.floating().active_window().unwrap().id(), &2);
}

#[test]
fn floating_tree_scratchpad_moves_the_whole_root() {
    let mut layout = Layout::default();
    Op::AddOutput(1).apply(&mut layout);
    for id in 1..=2 {
        Op::AddWindow {
            params: TestWindowParams::new(id),
        }
        .apply(&mut layout);
    }
    let workspace = layout.active_workspace_mut().unwrap();
    workspace.tiling_mut().focus_root();
    let root = workspace.tiling().focus().unwrap();
    let focused = workspace.tiling().node_for_window(&1).unwrap();
    workspace.tiling_mut().set_focus(focused);
    let (subtree, old_parent) = workspace.detach_tiling_subtree(root).unwrap();
    workspace.tiling_mut().finish_subtree_detach(old_parent);
    let rect = Rectangle::new((100., 120.).into(), (600., 450.).into());
    let (root, _) = workspace.floating_mut().add_tree(subtree, rect);

    layout.move_to_scratchpad(Some(&1));
    assert!(layout.is_scratchpad_hidden(&1));
    assert!(layout.is_scratchpad_hidden(&2));
    assert_eq!(layout.scratchpad_windows().count(), 2);

    assert_eq!(layout.show_scratchpad(Some(&2)), Some(1));
    let workspace = layout.active_workspace().unwrap();
    assert_eq!(workspace.floating_tree_root_for_window(&1), Some(root));
    assert_eq!(workspace.floating_tree_root_for_window(&2), Some(root));
    assert_eq!(
        workspace.floating().tree(root).unwrap().focus(),
        Some(focused)
    );
    layout.verify_invariants();
}

#[test]
fn mixed_layer_selection_filters_one_global_focus_order() {
    let output = test_output();
    let mut workspace = Workspace::new(
        output,
        Clock::with_time(Duration::ZERO),
        Rc::new(Options::default()),
    );
    for (id, timestamp) in [(1, 4), (2, 3), (3, 2), (4, 1)] {
        let window = TestWindow::new(TestWindowParams::new(id));
        window
            .0
            .focus_timestamp
            .set(Some(Duration::from_secs(timestamp)));
        let tile = workspace.make_tile(window);
        workspace.add_tile(
            tile,
            WorkspaceAddWindowTarget::Auto,
            super::workspace::AddTileOptions {
                activate: ActivateWindow::Yes,
                is_floating: false,
            },
        );
        if id >= 3 {
            workspace.toggle_window_floating(Some(&id));
        }
    }

    workspace.activate_window(&1);
    for expected in [1, 2, 3, 4] {
        assert_eq!(workspace.active_window().unwrap().id(), &expected);
        workspace.remove_tile(&expected, Transaction::new());
    }
}

#[test]
fn moving_a_floating_singleton_after_child_focus_keeps_its_resident_root() {
    let layout = check_ops([
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::FocusParent,
        Op::ToggleFocusedContainerFloating,
        Op::FocusChild,
        Op::MoveWindowDownOrToWorkspaceDown,
    ]);
    let (_, workspace) = layout
        .workspaces()
        .map(|(_, _, ws)| ws)
        .enumerate()
        .find(|(_, ws)| ws.has_window(&1))
        .expect("window 1 is still laid out");
    // Inside a floating container, so not a floating root leaf.
    assert!(workspace.floating().has_window(&1) && !workspace.is_floating(&1));
    assert_eq!(workspace.floating().tree_roots().count(), 1);
}

#[test]
fn layout_changes_do_not_flatten_a_floating_group_resident_root() {
    let layout = check_ops([
        Op::AddOutput(1),
        Op::AddOutput(2),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::SplitFocused(tiling_tree::Layout::SplitV),
        Op::FocusParent,
        Op::ToggleFocusedContainerFloating,
        Op::FocusChild,
        Op::FocusParent,
        Op::FocusParent,
        Op::SetFocusedLayout(tiling_tree::Layout::Tabbed),
        Op::SetFocusedLayout(tiling_tree::Layout::SplitH),
        Op::FocusWindow(2),
        Op::SetFocusedLayout(tiling_tree::Layout::SplitH),
    ]);
    // The split wrapped window 2 alone, so the floated group holds only it;
    // the layout changes must leave that group a floating tree root.
    let workspace = layout.active_workspace().unwrap();
    assert!(workspace.floating().has_window(&2) && !workspace.is_floating(&2));
    assert!(!workspace.floating().has_window(&1));
    assert_eq!(workspace.floating().tree_roots().count(), 1);
}

#[test]
fn closing_a_hidden_scratchpad_floating_group_child_removes_it() {
    let layout = check_ops([
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::SplitFocused(tiling_tree::Layout::SplitV),
        Op::FocusParent,
        Op::ToggleFocusedContainerFloating,
        Op::FocusChild,
        Op::AddWindow {
            params: TestWindowParams::new(5),
        },
        Op::ToggleFocusedContainerFloating,
        Op::MoveFocusedToScratchpad,
        Op::CloseWindow(5),
    ]);

    verify_layout_windows_reachable_once(&layout);
    assert!(!layout.has_window(&5));
}

#[test]
fn focus_parent_with_only_a_floating_window_preserves_tree_invariants() {
    let layout = check_ops([
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams {
                is_floating: true,
                ..TestWindowParams::new(1)
            },
        },
        Op::FocusParent,
    ]);
    // A floating root's parent is the workspace, so sway focuses the workspace and no view
    // (`focus_parent`, sway/commands/focus.c:339-351). Oracle random seed 230 step 19.
    assert!(layout.focus().is_none());
    assert!(layout
        .active_workspace()
        .is_some_and(|workspace| workspace.is_workspace_focused()));
}

#[test]
fn unfloat_container_after_changing_its_layout_does_not_panic() {
    check_ops([
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::ToggleFocusedContainerFloating,
        Op::FocusParent,
        Op::SetFocusedLayout(tiling_tree::Layout::Stacked),
        Op::SetFocusedLayout(tiling_tree::Layout::SplitH),
        Op::ToggleWindowFloating { id: None },
    ]);
}

#[test]
fn adding_a_floating_window_next_to_a_floating_container_does_not_panic() {
    let mut floating = TestWindowParams::new(3);
    floating.is_floating = true;
    check_ops([
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::AddOutput(1),
        Op::ToggleFocusedContainerFloating,
        Op::AddWindowNextTo {
            params: floating,
            next_to_id: 2,
        },
    ]);
}

#[test]
fn splitting_a_floating_window_cancels_interactive_resize() {
    let mut floating = TestWindowParams::new(3);
    floating.is_floating = true;
    check_ops([
        Op::AddWindow { params: floating },
        Op::AddOutput(1),
        Op::InteractiveResizeBegin {
            window: 3,
            edges: ResizeEdge::RIGHT,
        },
        Op::SplitFocused(tiling_tree::Layout::SplitH),
        Op::MoveWindowToWorkspaceDown(false),
    ]);
}

#[test]
fn interactive_resize_on_a_floating_container_does_not_panic() {
    check_ops([
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(3),
        },
        Op::ToggleFocusedContainerFloating,
        Op::InteractiveResizeBegin {
            window: 3,
            edges: ResizeEdge::RIGHT,
        },
    ]);
}

#[test]
fn moving_a_floating_workspace_between_fractional_scales_does_not_panic() {
    check_ops([
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::AddScaledOutput {
            id: 2,
            scale: 2.,
            layout_config: None,
        },
        Op::ToggleFocusedContainerFloating,
        Op::MoveWindowUpOrToWorkspaceUp,
        Op::AddScaledOutput {
            id: 1,
            scale: 1.5,
            layout_config: None,
        },
        Op::MoveWorkspaceToOutput(1),
    ]);
}

#[test]
fn hiding_the_active_floating_container_focuses_the_remaining_leaf() {
    let mut floating = TestWindowParams::new(1);
    floating.is_floating = true;
    let layout = check_ops([
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::ToggleFocusedContainerFloating,
        Op::AddWindow { params: floating },
        Op::FocusWindowDown,
        Op::MoveFocusedToScratchpad,
    ]);
    let hidden = if layout.is_scratchpad_window(&1) {
        1
    } else {
        2
    };
    let remaining = 3 - hidden;
    assert!(layout.is_scratchpad_window(&hidden));
    assert_eq!(layout.focus().map(|window| *window.id()), Some(remaining));
}

#[test]
fn centering_a_floating_container_does_not_panic() {
    check_ops([
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::ToggleFocusedContainerFloating,
        Op::CenterWindow { id: None },
    ]);
}

#[test]
fn moving_the_last_floating_leaf_keeps_a_resident_tree_active() {
    let mut floating = TestWindowParams::new(1);
    floating.is_floating = true;
    let layout = check_ops([
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(3),
        },
        Op::ToggleFocusedContainerFloating,
        Op::AddWindow { params: floating },
        Op::MoveWindowToWorkspaceDown(false),
    ]);
    let workspace = layout.active_workspace().unwrap();
    assert!(!workspace.has_window(&1));
    assert!(workspace.floating().has_window(&3) && !workspace.is_floating(&3));
    assert_eq!(workspace.floating().tree_roots().count(), 1);
    assert!(workspace.floating_is_active());
    assert_eq!(layout.focus().map(|window| *window.id()), Some(3));
}

#[test]
fn resizing_a_window_in_a_floating_container_does_not_panic() {
    check_ops([
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::ToggleFocusedContainerFloating,
        Op::SetFocusedWidth(SizeChange::SetFixed(0)),
    ]);
}

#[test]
fn directional_focus_with_one_floating_container_does_not_panic() {
    check_ops([
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::ToggleFocusedContainerFloating,
        Op::FocusLeft,
    ]);
}

#[test]
fn toggling_a_window_in_a_floating_container_unfloats_the_container() {
    let layout = check_ops([
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::ToggleFocusedContainerFloating,
        Op::ToggleWindowFloating { id: None },
    ]);
    let workspace = layout.active_workspace().unwrap();
    assert!(!workspace.floating().has_window(&1));
    assert!(workspace.floating().is_empty());
    assert!(!workspace.floating_is_active());
}

#[test]
fn preset_width_on_floating_container_does_not_panic() {
    // The floating-group operation generator found this command dispatching to
    // the leaf-only floating list for a resident tree (cc 7508638e).
    check_ops([
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::ToggleFocusedContainerFloating,
        Op::SwitchPresetTiledWidth,
    ]);
}

#[test]
fn unfloat_last_group_after_focusing_parent_deactivates_floating() {
    // The deep proptest soak shrank this to a floating group whose parent had
    // focus while its last member returned to tiling (cc e3487ca6).
    let mut options = Options::default();
    options.layout.default_orientation = swayward_config::DefaultOrientation::Vertical;
    let mut layout = check_ops_with_options(
        options,
        [
            Op::AddOutput(1),
            Op::AddWindow {
                params: TestWindowParams::new(3),
            },
            Op::AddWindow {
                params: TestWindowParams::new(1),
            },
            Op::MoveFocusedToWorkspaceUp(false),
            Op::UpdateConfig {
                layout_config: Box::default(),
            },
            Op::FocusWindowTop,
            Op::ToggleWindowFloating { id: None },
            Op::FocusParent,
        ],
    );
    let workspace = layout
        .workspaces()
        .map(|(_, _, ws)| ws)
        .find(|ws| ws.has_window(&3))
        .unwrap();
    assert!(workspace.floating().has_window(&3));
    check_ops_on_layout(
        &mut layout,
        [Op::ToggleWindowFloating { id: Some(3) }, Op::FocusChild],
    );
    let workspace = layout
        .workspaces()
        .map(|(_, _, ws)| ws)
        .find(|ws| ws.has_window(&3))
        .unwrap();
    assert!(!workspace.floating().has_window(&3));
    assert!(workspace.floating().is_empty());
    assert!(!workspace.floating_is_active());
}

/// Sway keeps one stacking list for every floating container, whether a view
/// or a split: a newly floated container goes on top (workspace_add_floating
/// appends, sway/tree/workspace.c:961-971) and activation raises it
/// (container_raise_floating, sway/tree/container.c:1625-1637). A single
/// floating window and a floating group therefore stack against each other.
#[test]
fn floating_windows_and_groups_share_one_stacking_order() {
    let (mut workspace, root) = floating_group_workspace();
    let tile = workspace.make_tile(TestWindow::new(TestWindowParams::new(3)));
    workspace.add_tile(
        tile,
        WorkspaceAddWindowTarget::Auto,
        super::super::workspace::AddTileOptions {
            activate: ActivateWindow::Yes,
            is_floating: true,
        },
    );

    // The newly floated single window is on top of the older group, and
    // activating a group child raises the whole group above it.
    assert_eq!(workspace.floating().stacking_order(), [Some(root), None]);
    workspace.activate_window(&1);
    assert_eq!(workspace.floating().stacking_order(), [None, Some(root)]);
    workspace.activate_window(&3);
    assert_eq!(workspace.floating().stacking_order(), [Some(root), None]);
}

/// A move inside a floating group stops at the group's root: the root is the
/// floating container, and sway's ancestor walk returns there instead of
/// escaping it (`container_is_floating`, sway/commands/move.c:326-330).
/// Shrunk from proptest cc d17cdea7.
#[test]
fn moves_inside_a_floating_group_keep_the_group_root() {
    let mut layout = check_ops([
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(3),
        },
        Op::SplitFocused(tiling_tree::Layout::SplitV),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::FocusParent,
        Op::ToggleFocusedContainerFloating,
        Op::FocusWindow(1),
    ]);
    check_ops_on_layout(
        &mut layout,
        [
            Op::MoveLeft,
            Op::MoveWindowDownOrToWorkspaceDown,
            Op::MoveLeft,
        ],
    );
    assert!(layout.has_window(&1) && layout.has_window(&3));
}

#[test]
fn layout_change_after_fullscreening_a_floating_group_child_keeps_the_group_root() {
    // Shrunk from floating_group_lifecycle_operations_preserve_reachability. A second
    // `focus parent` from the group root used to focus the tree's internal root node, which is
    // not a sway container; the layout command then flattened the group root away.
    let mut layout = Layout::default();
    for op in [
        Op::AddOutput(1),
        Op::AddOutput(2),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::SplitFocused(tiling_tree::Layout::SplitV),
        Op::FocusParent,
        Op::ToggleFocusedContainerFloating,
        Op::FocusChild,
        Op::FocusParent,
        Op::FocusParent,
        Op::SplitFocused(tiling_tree::Layout::SplitH),
        Op::FullscreenWindow(2),
        Op::SetFocusedLayout(tiling_tree::Layout::SplitH),
    ] {
        op.apply(&mut layout);
        layout.verify_invariants();
        verify_layout_windows_reachable_once(&layout);
    }
}

/// `focus parent; floating enable` on a horizontal split of views 1 and 2,
/// with view 1 focused, and the group root.
fn floated_split() -> (Layout<TestWindow>, tiling_tree::NodeId) {
    floated_split_with_options(Options::default())
}

fn floated_split_with_options(options: Options) -> (Layout<TestWindow>, tiling_tree::NodeId) {
    let layout = check_ops_with_options(
        options,
        [
            Op::AddOutput(1),
            Op::AddWindow {
                params: TestWindowParams::new(1),
            },
            Op::AddWindow {
                params: TestWindowParams::new(2),
            },
            Op::FocusWindow(1),
            Op::FocusParent,
            Op::ToggleFocusedContainerFloating,
        ],
    );
    let root = layout
        .active_workspace()
        .unwrap()
        .floating_tree_root_for_window(&1)
        .unwrap();
    assert_eq!(
        layout
            .active_workspace()
            .unwrap()
            .floating_tree_root_for_window(&2),
        Some(root)
    );
    layout.verify_invariants();
    (layout, root)
}

fn group_rect(layout: &Layout<TestWindow>, root: tiling_tree::NodeId) -> Rectangle<f64, Logical> {
    layout
        .active_workspace()
        .unwrap()
        .floating()
        .tree_rect(root)
        .unwrap()
}

fn group_child_rect(
    layout: &Layout<TestWindow>,
    root: tiling_tree::NodeId,
    window: usize,
) -> Rectangle<f64, Logical> {
    let tree = layout
        .active_workspace()
        .unwrap()
        .floating()
        .tree(root)
        .unwrap();
    tree.geometry(tree.node_for_window(&window).unwrap())
        .unwrap()
}

/// Sway moves the toplevel floating ancestor of the view under the cursor and keeps the
/// cursor's offset into it (sway/input/seatop_default.c:459-463,
/// sway/input/seatop_move_floating.c:37-42,58-70).
#[test]
fn pointer_move_on_a_floating_group_child_moves_the_group() {
    let (mut layout, root) = floated_split();
    let output = layout.outputs().next().unwrap().clone();
    let before = group_rect(&layout, root);
    let second_before = group_child_rect(&layout, root, 2);
    let start = before.loc + Point::from((10., 10.));

    // The drag starts on view 2, the right child.
    assert!(layout.interactive_move_begin(2, &output, start));
    assert!(layout.interactive_move_update(
        &2,
        Point::from((50., 30.)),
        output.clone(),
        start + Point::from((50., 30.)),
    ));
    assert!(layout.interactive_move_update(
        &2,
        Point::from((20., 5.)),
        output,
        start + Point::from((70., 35.)),
    ));
    layout.verify_invariants();
    layout.interactive_move_end(&2);
    layout.verify_invariants();

    let after = group_rect(&layout, root);
    assert_eq!(after.loc, before.loc + Point::from((70., 35.)));
    assert_eq!(after.size, before.size);
    let second_after = group_child_rect(&layout, root, 2);
    assert_eq!(
        second_after.loc,
        second_before.loc + Point::from((70., 35.))
    );
    assert_eq!(second_after.size, second_before.size);
    // The group stays one floating container holding both views.
    let workspace = layout.active_workspace().unwrap();
    assert_eq!(workspace.floating_tree_root_for_window(&1), Some(root));
    assert_eq!(workspace.floating_tree_root_for_window(&2), Some(root));
    assert!(layout.interactive_move.is_none());
    assert!(layout.floating_group_move.is_none());
}

/// A single floating view keeps the niri interactive move, which detaches its tile.
#[test]
fn pointer_move_on_a_single_floating_window_keeps_the_detaching_move() {
    let mut params = TestWindowParams::new(1);
    params.is_floating = true;
    let mut layout = check_ops([Op::AddOutput(1), Op::AddWindow { params }]);
    let output = layout.outputs().next().unwrap().clone();

    assert!(layout.interactive_move_begin(1, &output, Point::from((100., 100.))));
    assert!(layout.floating_group_move.is_none());
    assert!(layout.interactive_move_update(
        &1,
        Point::from((50., 0.)),
        output,
        Point::from((150., 100.)),
    ));
    assert!(matches!(
        layout.interactive_move,
        Some(InteractiveMoveState::Moving(_))
    ));
    layout.interactive_move_end(&1);
    layout.verify_invariants();
}

/// Sway's modifier resize resizes the toplevel ancestor from the edge the cursor's
/// quadrant picks (sway/input/seatop_default.c:476-487), and `arrange_container` lays the
/// children out at their split ratios (sway/tree/arrange.c:249-262).
#[test]
fn modifier_resize_on_a_floating_group_child_resizes_the_root() {
    let (mut layout, root) = floated_split();
    let before = group_rect(&layout, root);

    assert!(layout.floating_group_resize_begin(1, ResizeEdge::BOTTOM_RIGHT));
    assert!(layout.interactive_resize_update(&1, Point::from((100., 50.))));
    layout.verify_invariants();
    let after = group_rect(&layout, root);
    assert_eq!(after.loc, before.loc);
    assert_eq!(after.size, before.size + Size::from((100., 50.)));
    // Both children keep half the width each.
    let first = group_child_rect(&layout, root, 1);
    let second = group_child_rect(&layout, root, 2);
    assert_eq!(first.size.w, second.size.w);
    assert_eq!(first.size.h, after.size.h);
    layout.interactive_resize_end(&1);

    // From the top-left the opposite corner stays put.
    let before = after;
    assert!(layout.floating_group_resize_begin(2, ResizeEdge::TOP_LEFT));
    assert!(layout.interactive_resize_update(&2, Point::from((40., 20.))));
    layout.interactive_resize_end(&2);
    layout.verify_invariants();
    let after = group_rect(&layout, root);
    assert_eq!(after.loc, before.loc + Point::from((40., 20.)));
    assert_eq!(after.size, before.size - Size::from((40., 20.)));
}

/// Options with a 4 px border, so a test window, which has no input region, is hit on its
/// border.
fn bordered() -> Options {
    let mut options = Options::default();
    options.layout.border.off = false;
    options.layout.border.width = 4.;
    options
}

/// The rendered position and size of view `window`'s tile in group `root`.
fn group_tile(
    layout: &Layout<TestWindow>,
    root: tiling_tree::NodeId,
    window: usize,
) -> Rectangle<f64, Logical> {
    let tree = layout
        .active_workspace()
        .unwrap()
        .floating()
        .tree(root)
        .unwrap();
    tree.tiles_with_render_positions()
        .find(|(tile, _, _)| tile.window().id() == &window)
        .map(|(tile, pos, _)| Rectangle::new(pos, tile.tile_size()))
        .unwrap()
}

/// The modifier-resize edge comes from the group root's quadrant, not the view's
/// (sway/input/seatop_default.c:479-481).
#[test]
fn modifier_resize_edge_on_a_floating_group_uses_the_root_quadrant() {
    let (layout, root) = floated_split_with_options(bordered());
    let second = group_tile(&layout, root, 2);
    let workspace = layout.active_workspace().unwrap();
    // The top-left corner of the right child is in the top-right quadrant of the group.
    let pos = second.loc + Point::from((1., 1.));
    assert_eq!(workspace.window_under(pos).unwrap().0.id(), &2);
    assert_eq!(
        workspace.resize_edges_under(pos),
        Some(ResizeEdge::TOP_RIGHT)
    );
}

/// A view inside a floating group is not floating (`container_is_floating`,
/// sway/tree/container.c:1041-1050), so a border drag on it is a tiled resize against
/// its sibling (sway/input/seatop_default.c:396-411, sway/input/seatop_resize_tiling.c),
/// and an edge with no sibling past it resizes nothing (`edge_is_external`,
/// sway/input/seatop_default.c:39-75,111-118).
#[test]
fn border_resize_on_a_floating_group_child_resizes_inside_the_group() {
    let (mut layout, root) = floated_split();
    let before = group_rect(&layout, root);
    let first_before = group_child_rect(&layout, root, 1);
    let second_before = group_child_rect(&layout, root, 2);

    assert!(!layout.interactive_resize_begin(1, ResizeEdge::LEFT));
    assert!(!layout.interactive_resize_begin(1, ResizeEdge::TOP));

    assert!(layout.interactive_resize_begin(1, ResizeEdge::RIGHT));
    assert!(layout.interactive_resize_update(&1, Point::from((60., 0.))));
    layout.interactive_resize_end(&1);
    layout.verify_invariants();

    assert_eq!(group_rect(&layout, root), before);
    let first = group_child_rect(&layout, root, 1);
    let second = group_child_rect(&layout, root, 2);
    assert_eq!(first.size.w, first_before.size.w + 60.);
    assert_eq!(second.size.w, second_before.size.w - 60.);
    assert_eq!(second.loc.x, second_before.loc.x + 60.);
}

/// Only a group view's internal border is a resize edge; the group's outer edges are
/// external to the view and the group container has no border of its own
/// (sway/tree/container.c:88-94, sway/input/seatop_default.c:111-118).
#[test]
fn border_resize_edges_of_a_floating_group_are_the_internal_ones() {
    let (layout, root) = floated_split_with_options(bordered());
    let first = group_tile(&layout, root, 1);
    let workspace = layout.active_workspace().unwrap();
    let mid_y = first.loc.y + first.size.h / 2.;

    // The right border of the left view, shared with the right view.
    let (window, edges) = workspace
        .border_resize_edges_under(Point::from((first.loc.x + first.size.w - 1., mid_y)))
        .unwrap();
    assert_eq!(window.id(), &1);
    assert_eq!(edges, ResizeEdge::RIGHT);
    // The left border of the left view is the group's outer edge.
    let outer = Point::from((first.loc.x + 1., mid_y));
    assert_eq!(workspace.window_under(outer).unwrap().0.id(), &1);
    assert!(workspace.border_resize_edges_under(outer).is_none());
}
