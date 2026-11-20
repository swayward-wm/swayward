use smithay::backend::input::{ButtonState, InputTime};
use smithay::input::pointer::ButtonEvent;
use smithay::reexports::wayland_protocols::xdg::decoration::zv1::client::zxdg_toplevel_decoration_v1::Mode;
use smithay::reexports::wayland_server::Resource as _;
use smithay::utils::SERIAL_COUNTER;
use smithay::wayland::shell::xdg::XdgShellHandler as _;
use wayland_client::Proxy as _;

use super::client::ClientId;
use super::*;
use crate::input::move_grab::MoveGrab;
use crate::input::resize_grab::ResizeGrab;
use crate::layout::tiling_tree::IpcNode;
use crate::layout::LayoutElement as _;
use crate::swayward::State;

#[derive(Clone, Copy)]
enum WindowState {
    Tiled,
    Floating,
    Fullscreen,
}

fn set_up(state: WindowState) -> (Fixture, ClientId) {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1280, 720));

    let client = fixture.add_client();
    let window = fixture.client(client).create_window();
    window.commit();
    fixture.roundtrip(client);

    let window = fixture.client(client).state.windows.last_mut().unwrap();
    window.attach_new_buffer();
    window.set_size(400, 300);
    window.ack_last_and_commit();
    fixture.double_roundtrip(client);

    if matches!(state, WindowState::Tiled) {
        let window = fixture.client(client).create_window();
        window.commit();
        fixture.roundtrip(client);
        let window = fixture.client(client).state.windows.last_mut().unwrap();
        window.attach_new_buffer();
        window.set_size(400, 300);
        window.ack_last_and_commit();
        fixture.double_roundtrip(client);
    } else {
        fixture.swayward().layout.toggle_window_floating(None);
        fixture.double_roundtrip(client);
        fixture
            .client(client)
            .state
            .windows
            .last_mut()
            .unwrap()
            .ack_last_and_commit();
        fixture.double_roundtrip(client);
    }
    if matches!(state, WindowState::Fullscreen) {
        let window = fixture
            .swayward()
            .layout
            .windows()
            .next()
            .unwrap()
            .1
            .window
            .clone();
        fixture.swayward().layout.set_fullscreen(&window, true);
    }

    fixture.niri_state().move_cursor((200., 150.).into());
    (fixture, client)
}

#[test]
fn initial_decoration_mode_honours_open_floating_rule() {
    let config = swayward_config::Config::parse_mem(
        r#"
window-rule {
    open-floating true
}
"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    let client = fixture.add_client();
    fixture.client(client).create_window();
    fixture
        .client(client)
        .decorate_last_window(Mode::ClientSide);
    fixture
        .client(client)
        .state
        .windows
        .last()
        .unwrap()
        .commit();
    fixture.double_roundtrip(client);

    assert_eq!(
        fixture
            .client(client)
            .state
            .windows
            .last()
            .unwrap()
            .decoration_modes
            .last(),
        Some(&Mode::ClientSide),
        "a window predicted to float must receive its initial client-side mode"
    );
}

#[test]
fn decoration_mode_follows_sway_tiling_policy() {
    let mut fixture = Fixture::with_config(swayward_config::Config::parse_mem("").unwrap());
    fixture.add_output(1, (1280, 720));

    let client = fixture.add_client();
    assert!(fixture
        .client(client)
        .state
        .xdg_decoration_manager
        .is_some());
    let window = fixture.client(client).create_window();
    window.commit();
    fixture.roundtrip(client);
    fixture
        .client(client)
        .decorate_last_window(Mode::ClientSide);
    fixture.double_roundtrip(client);
    assert_eq!(
        fixture
            .client(client)
            .state
            .windows
            .last()
            .unwrap()
            .decoration_modes
            .last(),
        Some(&Mode::ServerSide),
        "tiled windows must use server-side decorations"
    );

    let window = fixture.client(client).state.windows.last_mut().unwrap();
    window.attach_new_buffer();
    window.set_size(400, 300);
    window.ack_last_and_commit();
    fixture.double_roundtrip(client);
    // Sway checks only that an xdg-decoration object exists
    // (sway/commands/border.c:77-80), so a tiled window negotiated to
    // server-side still accepts `border csd` and is told to draw its own
    // (view_set_csd_from_server, sway/tree/view.c:515-525).
    let outcome = crate::command::execute(fixture.niri_state(), "border csd");
    assert!(outcome[0].success, "{:?}", outcome[0].error);
    fixture.double_roundtrip(client);
    assert_eq!(
        fixture
            .client(client)
            .state
            .windows
            .last()
            .unwrap()
            .decoration_modes
            .last(),
        Some(&Mode::ClientSide),
        "border csd must switch the window to client-side decorations"
    );

    fixture.swayward().layout.toggle_window_floating(None);
    fixture.swayward().layout.refresh(true);
    fixture
        .client(client)
        .request_decoration_mode(Mode::ClientSide);
    fixture.double_roundtrip(client);
    assert_eq!(
        fixture
            .client(client)
            .state
            .windows
            .last()
            .unwrap()
            .decoration_modes
            .last(),
        Some(&Mode::ClientSide),
        "floating windows must receive the mode they request"
    );
}

fn start_pointer_grab(
    fixture: &mut Fixture,
) -> (
    smithay::input::pointer::PointerHandle<State>,
    smithay::utils::Serial,
) {
    let serial = SERIAL_COUNTER.next_serial();
    let pointer = fixture.swayward().seat.get_pointer().unwrap();
    pointer.button(
        fixture.niri_state(),
        &ButtonEvent {
            button: 0x110,
            state: ButtonState::Pressed,
            serial,
            time: InputTime::from_millis(1),
        },
    );
    (pointer, serial)
}

fn toplevel_and_seat(
    fixture: &mut Fixture,
) -> (
    smithay::wayland::shell::xdg::ToplevelSurface,
    smithay::reexports::wayland_server::protocol::wl_seat::WlSeat,
) {
    let toplevel = fixture
        .swayward()
        .layout
        .windows()
        .next()
        .unwrap()
        .1
        .toplevel()
        .clone();
    let wayland_client = fixture
        .swayward()
        .display_handle
        .get_client(toplevel.wl_surface().id())
        .unwrap();
    let seat = fixture
        .swayward()
        .seat
        .client_seats(&wayland_client)
        .remove(0);
    (toplevel, seat)
}

fn release_pointer(fixture: &mut Fixture, pointer: smithay::input::pointer::PointerHandle<State>) {
    pointer.button(
        fixture.niri_state(),
        &ButtonEvent {
            button: 0x110,
            state: ButtonState::Released,
            serial: SERIAL_COUNTER.next_serial(),
            time: InputTime::from_millis(2),
        },
    );
}

fn request_move(fixture: &mut Fixture) -> bool {
    let (pointer, serial) = start_pointer_grab(fixture);
    let (toplevel, seat) = toplevel_and_seat(fixture);
    fixture.niri_state().move_request(toplevel, seat, serial);

    let is_move = pointer
        .with_grab(|_, grab| grab.is::<MoveGrab>())
        .unwrap_or(false);
    release_pointer(fixture, pointer);
    is_move
}

fn request_resize(fixture: &mut Fixture) -> bool {
    let (pointer, serial) = start_pointer_grab(fixture);
    let (toplevel, seat) = toplevel_and_seat(fixture);
    fixture.niri_state().resize_request(
        toplevel,
        seat,
        serial,
        smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::ResizeEdge::BottomRight,
    );

    let is_resize = pointer
        .with_grab(|_, grab| grab.is::<ResizeGrab>())
        .unwrap_or(false);
    release_pointer(fixture, pointer);
    is_resize
}

fn tree_shape(node: &IpcNode<smithay::desktop::Window>) -> String {
    match node {
        IpcNode::Split {
            layout, children, ..
        } => format!(
            "{layout:?}[{}]",
            children
                .iter()
                .map(tree_shape)
                .collect::<Vec<_>>()
                .join(",")
        ),
        IpcNode::Leaf { window, .. } => window
            .toplevel()
            .unwrap()
            .wl_surface()
            .id()
            .protocol_id()
            .to_string(),
    }
}

#[test]
fn remap_replaces_the_tree_entry_and_focuses_the_same_toplevel_once() {
    let mut f = Fixture::new();
    f.add_output(1, (800, 600));
    let client = f.add_client();
    let _first = windows::map_window(&mut f, client, windows::WindowSpec::titled("first"));
    let second = windows::map_window(&mut f, client, windows::WindowSpec::titled("second"));

    f.client(client).window(&second).attach_null();
    f.client(client).window(&second).commit();
    f.double_roundtrip(client);
    assert_eq!(f.swayward().layout.windows().count(), 1);
    assert_eq!(f.swayward().layout.focus().unwrap().title(), "first");

    f.client(client).window(&second).commit();
    f.double_roundtrip(client);
    let window = f.client(client).window(&second);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    assert_eq!(f.swayward().layout.windows().count(), 2);
    assert!(f.swayward().unmapped_windows.is_empty());
    let remapped = f.swayward().layout.focus().unwrap().window.clone();
    assert_eq!(
        remapped.toplevel().unwrap().wl_surface().id().protocol_id(),
        second.id().protocol_id()
    );
    assert_eq!(
        f.swayward()
            .layout
            .windows()
            .filter(|(_, mapped)| mapped.window == remapped)
            .count(),
        1
    );
}

#[test]
fn fullscreen_round_trip_restores_the_original_nested_tree_position() {
    let mut f = Fixture::new();
    f.add_output(1, (800, 600));
    let client = f.add_client();
    let _first = windows::map_window(&mut f, client, windows::WindowSpec::titled("first"));
    let second = windows::map_window(&mut f, client, windows::WindowSpec::titled("second"));
    let _third = windows::map_window(&mut f, client, windows::WindowSpec::titled("third"));
    f.swayward().layout.nest_or_unnest_window_left(None);
    let before = tree_shape(
        &f.swayward()
            .layout
            .active_workspace()
            .unwrap()
            .ipc_tiling_tree(),
    );

    f.client(client).window(&second).set_fullscreen(None);
    f.double_roundtrip(client);
    assert!(f
        .swayward()
        .layout
        .windows()
        .find(|(_, mapped)| {
            mapped.toplevel().wl_surface().id().protocol_id() == second.id().protocol_id()
        })
        .unwrap()
        .1
        .pending_sizing_mode()
        .is_fullscreen());

    f.client(client).window(&second).unset_fullscreen();
    f.double_roundtrip(client);
    let mapped = f
        .swayward()
        .layout
        .windows()
        .find(|(_, mapped)| {
            mapped.toplevel().wl_surface().id().protocol_id() == second.id().protocol_id()
        })
        .unwrap()
        .1;
    assert!(mapped.pending_sizing_mode().is_normal());
    let after = tree_shape(
        &f.swayward()
            .layout
            .active_workspace()
            .unwrap()
            .ipc_tiling_tree(),
    );

    assert_eq!(after, before);
}

#[test]
#[should_panic(expected = "Protocol error 4 on object xdg_wm_base")]
fn stale_configure_ack_is_rejected_by_the_xdg_protocol() {
    let mut f = Fixture::new();
    f.add_output(1, (800, 600));
    let client = f.add_client();
    let surface = windows::map_window(&mut f, client, windows::WindowSpec::titled("window"));
    let stale_serial = f.client(client).window(&surface).configures_received[0].0;

    f.client(client).window(&surface).set_fullscreen(None);
    f.double_roundtrip(client);
    let window = f.client(client).window(&surface);
    window.ack_configure(stale_serial);
    window.commit();
    f.double_roundtrip(client);
}

#[test]
fn popup_reposition_after_parent_unmaps_is_safe() {
    let mut f = Fixture::new();
    f.add_output(1, (800, 600));
    let client = f.add_client();

    let window = f.client(client).create_window();
    window.commit();
    let parent_surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&parent_surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    let parent = f.client(client).window(&parent_surface).xdg_surface.clone();
    let popup = f.client(client).create_popup(&parent);
    let popup_surface = popup.surface.clone();
    popup.surface.commit();
    f.double_roundtrip(client);
    assert!(!f
        .client(client)
        .popup(&popup_surface)
        .configures_received
        .is_empty());

    let window = f.client(client).window(&parent_surface);
    window.attach_null();
    window.commit();
    f.double_roundtrip(client);

    let popup = f.client(client).popup(&popup_surface).xdg_popup.clone();
    f.client(client).state.reposition_popup(&popup, 42);
    f.client(client).connection.flush().unwrap();
    f.dispatch();
    f.client(client).dispatch_unchecked();

    assert!(f.client(client).connection.protocol_error().is_none());
    assert_eq!(f.client(client).popup(&popup_surface).repositioned, [42]);
}

#[test]
fn mutter_x11_interop_accepts_an_unmapped_surface() {
    let mut f = Fixture::new();
    f.add_output(1, (800, 600));
    let client = f.add_client();
    let surface = f.client(client).create_window().surface.clone();

    let interop = f.client(client).state.mutter_x11_interop.clone().unwrap();
    interop.set_x11_parent(&surface, 42);
    f.client(client).connection.flush().unwrap();
    f.dispatch();
    f.client(client).dispatch_unchecked();

    assert!(f.client(client).connection.protocol_error().is_none());
}

#[test]
fn client_interactive_move_requires_floating_and_non_fullscreen() {
    let (mut tiled, _) = set_up(WindowState::Tiled);
    assert!(!request_move(&mut tiled));

    let (mut floating, _) = set_up(WindowState::Floating);
    assert!(request_move(&mut floating));

    let (mut fullscreen, _) = set_up(WindowState::Fullscreen);
    assert!(!request_move(&mut fullscreen));
}

#[test]
fn client_interactive_resize_requires_floating() {
    let (mut tiled, _) = set_up(WindowState::Tiled);
    assert!(!request_resize(&mut tiled));

    let (mut floating, _) = set_up(WindowState::Floating);
    assert!(request_resize(&mut floating));

    let (mut fullscreen, _) = set_up(WindowState::Fullscreen);
    assert!(!request_resize(&mut fullscreen));
}

fn popup_position(
    f: &mut Fixture,
    client: ClientId,
    parent: &wayland_client::protocol::wl_surface::WlSurface,
    offset: (i32, i32),
) -> (i32, i32) {
    let parent = f.client(client).window(parent).xdg_surface.clone();
    let surface = f
        .client(client)
        .state
        .create_popup(Some(&parent), Some(offset))
        .surface
        .clone();
    surface.commit();
    f.double_roundtrip(client);
    let &(x, y, _, _) = f
        .client(client)
        .popup(&surface)
        .configures_received
        .last()
        .unwrap();
    (x, y)
}

/// Sway fits a window's popups to its output, expressed in the window's coordinates
/// (`popup_unconstrain`, sway/desktop/xdg_shell.c:44-66). A tiled window away from the
/// output origin keeps the popup where the client put it, and a popup may extend past the
/// window's own edge while it stays on the output.
#[test]
fn tiled_popups_are_unconstrained_against_the_output() {
    let mut f = Fixture::new();
    f.add_output(1, (800, 600));
    let client = f.add_client();
    let left = windows::map_window(&mut f, client, windows::WindowSpec::titled("left"));
    let right = windows::map_window(&mut f, client, windows::WindowSpec::titled("right"));

    let on_left = popup_position(&mut f, client, &left, (100, 100));
    assert_eq!(popup_position(&mut f, client, &right, (100, 100)), on_left);

    // The 100 px popup straddles the boundary between the two 400 px allocations. The
    // clients' own surfaces are far smaller, so it is also outside its parent.
    assert_eq!(
        popup_position(&mut f, client, &left, (420, 100)),
        (on_left.0 + 320, on_left.1)
    );
}

/// The leftmost and rightmost x, in the parent's coordinates, that a 100 px popup is slid to.
/// They differ by the width of the box the popup is fitted to, less the popup and the
/// 8 px margin kept at each end.
fn popup_x_limits(
    f: &mut Fixture,
    client: ClientId,
    parent: &wayland_client::protocol::wl_surface::WlSurface,
) -> (i32, i32) {
    (
        popup_position(f, client, parent, (-5000, 100)).0,
        popup_position(f, client, parent, (5000, 100)).0,
    )
}

/// Sway's popup box for a floating view is the output too, not the usable area, so the popup
/// may cover a bar (`popup_unconstrain`, sway/desktop/xdg_shell.c:44-66).
#[test]
fn floating_popups_are_unconstrained_against_the_output_not_the_usable_area() {
    use smithay::desktop::layer_map_for_output;
    use smithay::reexports::wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::Layer;
    use smithay::reexports::wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::Anchor;

    use super::client::LayerConfigureProps;

    let mut f = Fixture::new();
    f.add_output(1, (800, 600));
    let client = f.add_client();
    let window = windows::map_window(&mut f, client, windows::WindowSpec::sized(300, 200));
    f.swayward().layout.toggle_window_floating(None);
    f.double_roundtrip(client);
    f.client(client).window(&window).ack_last_and_commit();
    f.double_roundtrip(client);

    let bar = f.client(client).create_layer(None, Layer::Top, "bar");
    let bar_surface = bar.surface.clone();
    bar.set_configure_props(LayerConfigureProps {
        anchor: Some(Anchor::Top | Anchor::Left | Anchor::Right),
        size: Some((0, 50)),
        exclusive_zone: Some(50),
        ..Default::default()
    });
    bar.commit();
    f.double_roundtrip(client);
    let bar = f.client(client).layer(&bar_surface);
    bar.attach_new_buffer();
    bar.set_size(800, 50);
    bar.ack_last_and_commit();
    f.double_roundtrip(client);
    let output = f.niri_output(1);
    assert_eq!(layer_map_for_output(&output).non_exclusive_zone().loc.y, 50);

    // Slid as far up and as far down as the box allows: the output's 600 px, less the
    // 100 px popup and the 8 px margin at each end. The usable area is 50 px shorter.
    let top = popup_position(&mut f, client, &window, (100, -5000)).1;
    let bottom = popup_position(&mut f, client, &window, (100, 5000)).1;
    assert_eq!(bottom - top, 600 - 16 - 100);
}

/// The box is the window's own output, in output-local coordinates: an output placed away
/// from the layout origin does not shift it.
#[test]
fn popups_on_a_second_output_are_unconstrained_against_that_output() {
    let mut f = Fixture::new();
    f.add_output(1, (800, 600));
    f.add_output_at(2, (1000, 600), Some((800, 0)));
    let client = f.add_client();
    let first = windows::map_window(&mut f, client, windows::WindowSpec::titled("first"));
    let layout = &mut f.swayward().layout;
    let output = layout
        .outputs()
        .find(|output| output.name() == "headless-2");
    let output = output.unwrap().clone();
    layout.focus_output(&output);
    let second = windows::map_window(&mut f, client, windows::WindowSpec::titled("second"));

    let (first_left, first_right) = popup_x_limits(&mut f, client, &first);
    let (second_left, second_right) = popup_x_limits(&mut f, client, &second);
    assert_eq!(first_right - first_left, 800 - 16 - 100);
    assert_eq!(second_right - second_left, 1000 - 16 - 100);
    assert_eq!(second_left, first_left);

    assert_eq!(
        popup_position(&mut f, client, &second, (100, 100)),
        popup_position(&mut f, client, &first, (100, 100))
    );
}

/// wlroots truncates the output box: 2560 px at scale 1.5 is 1706 wide, where the layout's
/// view size is 1706.67 and rounds to 1707 (wlr_output_effective_resolution,
/// wlroots/types/output/output.c:472-477).
#[test]
fn popup_output_box_is_truncated_at_a_fractional_scale() {
    let mut f = Fixture::new();
    f.add_output(1, (2560, 1440));
    let output = f.niri_output(1);
    output.change_current_state(
        None,
        None,
        Some(smithay::output::Scale::Fractional(1.5)),
        None,
    );
    f.swayward().output_resized(&output);
    let client = f.add_client();
    let window = windows::map_window(&mut f, client, windows::WindowSpec::titled("only"));

    let (left, right) = popup_x_limits(&mut f, client, &window);
    assert_eq!(right - left, 1706 - 16 - 100);
}

/// Popup positions and the box are both in window-geometry coordinates, so a client whose
/// geometry does not start at its surface origin gets the same answers.
#[test]
fn popup_box_follows_the_window_geometry_offset() {
    let mut f = Fixture::new();
    f.add_output(1, (800, 600));
    let client = f.add_client();
    windows::map_window(&mut f, client, windows::WindowSpec::titled("left"));
    let window = windows::map_window(&mut f, client, windows::WindowSpec::sized(300, 200));

    let mut measure = |geometry: (i32, i32)| {
        let parent = f.client(client).window(&window);
        parent
            .xdg_surface
            .set_window_geometry(geometry.0, geometry.1, 200, 100);
        parent.commit();
        f.double_roundtrip(client);
        (
            popup_x_limits(&mut f, client, &window),
            popup_position(&mut f, client, &window, (100, 100)),
        )
    };

    let at_origin = measure((0, 0));
    assert_eq!(measure((10, 20)), at_origin);
}
