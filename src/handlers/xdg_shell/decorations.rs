use super::*;

impl XdgToplevelTagHandler for State {
    fn set_tag(&mut self, toplevel: xdg_toplevel::XdgToplevel, _tag: String) {
        let Some(toplevel) = self.swayward.xdg_shell_state.get_toplevel(&toplevel) else {
            return;
        };
        self.update_window_rules(&toplevel);
        let id = self
            .swayward
            .layout
            .find_window_and_output(toplevel.wl_surface())
            .map(|(mapped, _)| mapped.id());
        if let Some(id) = id {
            crate::command::run_for_window(self, id);
        }
    }
}

/// Queries for the toplevel's live xdg-decoration object, sway's
/// `view->xdg_decoration`.
///
/// Smithay reports a new object but not its destruction and keeps the object
/// private, while sway clears `view->xdg_decoration` on destroy
/// (sway/xdg_decoration.c:9-20). Scanning the client's live objects answers
/// both cases. It runs only for `border` commands.
pub struct XdgDecorationObject;

impl XdgDecorationObject {
    /// Whether the toplevel currently has a live xdg-decoration object.
    pub fn is_present(toplevel: &ToplevelSurface) -> bool {
        use smithay::reexports::wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::ZxdgToplevelDecorationV1;

        let surface = toplevel.wl_surface();
        let (Some(client), Some(handle)) = (surface.client(), surface.handle().upgrade()) else {
            return false;
        };
        let mut ids = Vec::new();
        if handle
            .with_all_objects_for(client.id(), |id| ids.push(id))
            .is_err()
        {
            return false;
        }
        let dh = wayland_server::DisplayHandle::from(handle);
        ids.into_iter()
            .filter(|id| id.interface().name == ZxdgToplevelDecorationV1::interface().name)
            .filter_map(|id| ZxdgToplevelDecorationV1::from_id(&dh, id).ok())
            .any(|object| object.data::<ToplevelSurface>() == Some(toplevel))
    }
}

/// The mode the client last asked for on its xdg-decoration object, wlroots'
/// `requested_mode`. Smithay does not keep it.
#[derive(Default)]
struct XdgDecorationRequest(Cell<Option<zxdg_toplevel_decoration_v1::Mode>>);

fn record_requested_mode(
    toplevel: &ToplevelSurface,
    mode: Option<zxdg_toplevel_decoration_v1::Mode>,
) {
    with_states(toplevel.wl_surface(), |states| {
        states
            .data_map
            .get_or_insert(XdgDecorationRequest::default)
            .0
            .set(mode);
    });
}

/// Whether sway maps this toplevel as using client-side decorations
/// (`handle_map`, sway/desktop/xdg_shell.c:484-500): with an xdg-decoration
/// object, when the client requested client-side; otherwise unless a KDE server
/// decoration negotiated server-side. A client that binds neither protocol
/// draws its own.
pub fn maps_with_client_decorations(toplevel: &ToplevelSurface) -> bool {
    if XdgDecorationObject::is_present(toplevel) {
        return with_states(toplevel.wl_surface(), |states| {
            states
                .data_map
                .get::<XdgDecorationRequest>()
                .and_then(|request| request.0.get())
                == Some(zxdg_toplevel_decoration_v1::Mode::ClientSide)
        });
    }
    with_states(toplevel.wl_surface(), |states| {
        states
            .data_map
            .get::<KdeDecorationsModeState>()
            .is_none_or(|state| !state.is_server())
    })
}

impl XdgDecorationHandler for State {
    fn new_decoration(&mut self, toplevel: ToplevelSurface) {
        // A fresh wlroots decoration object starts with requested_mode NONE
        // (wlr_xdg_decoration_v1.c), so a request on a destroyed object no
        // longer counts.
        record_requested_mode(&toplevel, None);
        toplevel.with_pending_state(|state| {
            state.decoration_mode = Some(zxdg_toplevel_decoration_v1::Mode::ServerSide);
        });
    }

    fn request_mode(&mut self, toplevel: ToplevelSurface, mode: zxdg_toplevel_decoration_v1::Mode) {
        record_requested_mode(&toplevel, Some(mode));
        // Match sway: tiled windows always use server-side decorations, while floating windows
        // honour the client's requested mode (sway/xdg_decoration.c:64-90).
        let mode = if self.window_is_or_will_be_floating(&toplevel) {
            mode
        } else {
            zxdg_toplevel_decoration_v1::Mode::ServerSide
        };
        toplevel.with_pending_state(|state| {
            state.decoration_mode = Some(mode);
        });

        // A configure is required in response to this event. However, if an initial configure
        // wasn't sent, then we will send this as part of the initial configure later.
        if toplevel.is_initial_configure_sent() {
            // If this is a mapped window, flag it as needs configure to avoid duplicate configures.
            let surface = toplevel.wl_surface();
            if let Some((mapped, _)) = self.swayward.layout.find_window_and_output_mut(surface) {
                mapped.set_needs_configure();
            } else {
                toplevel.send_configure();
            }
        }
    }

    fn unset_mode(&mut self, toplevel: ToplevelSurface) {
        record_requested_mode(&toplevel, None);
        toplevel.with_pending_state(|state| {
            state.decoration_mode = Some(zxdg_toplevel_decoration_v1::Mode::ServerSide);
        });

        // A configure is required in response to this event. However, if an initial configure
        // wasn't sent, then we will send this as part of the initial configure later.
        if toplevel.is_initial_configure_sent() {
            // If this is a mapped window, flag it as needs configure to avoid duplicate configures.
            let surface = toplevel.wl_surface();
            if let Some((mapped, _)) = self.swayward.layout.find_window_and_output_mut(surface) {
                mapped.set_needs_configure();
            } else {
                toplevel.send_configure();
            }
        }
    }
}

/// Whether KDE server decorations are in use.
#[derive(Default, Clone)]
pub struct KdeDecorationsModeState {
    server: Cell<bool>,
}

impl KdeDecorationsModeState {
    pub fn is_server(&self) -> bool {
        self.server.get()
    }
}

impl KdeDecorationHandler for State {
    fn kde_decoration_state(&self) -> &KdeDecorationState {
        &self.swayward.kde_decoration_state
    }

    fn request_mode(
        &mut self,
        surface: &WlSurface,
        decoration: &org_kde_kwin_server_decoration::OrgKdeKwinServerDecoration,
        mode: wayland_server::WEnum<org_kde_kwin_server_decoration::Mode>,
    ) {
        let WEnum::Value(mode) = mode else {
            return;
        };

        decoration.mode(mode);

        with_states(surface, |states| {
            let state = states
                .data_map
                .get_or_insert(KdeDecorationsModeState::default);
            state
                .server
                .set(mode == org_kde_kwin_server_decoration::Mode::Server);
        });
    }
}

impl XdgForeignHandler for State {
    fn xdg_foreign_state(&mut self) -> &mut XdgForeignState {
        &mut self.swayward.xdg_foreign_state
    }
}
