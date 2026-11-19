//! Which parser rejections refuse a command that sway accepts.
//!
//! Every rule here names what sway accepts, so a misspelt command is never
//! reported as merely unimplemented.

pub(super) const URGENT_POLICY: &str =
    "urgent allow|deny requires client urgency-request policy support";
pub(super) const GESTURE_BINDINGS: &str = "gesture events have no sway command-binding model";
pub(super) const BAR_CONFIGURATION: &str =
    "bar configuration is unsupported because swayward does not manage bars";
pub(super) const BINDSWITCH_RELOAD: &str =
    "bindswitch --reload requires tracked per-device switch state during reload";
pub(super) const INPUT_SUBCOMMAND: &str =
    "only input <identifier> xkb_switch_layout <next|prev|index> is supported";
pub(super) const ADAPTIVE_SYNC_TOGGLE: &str = "adaptive_sync toggle is not implemented";
pub(super) const CLIENT_COLOURS: &str =
    "client colour commands are unsupported because sway window-border colours are not fully rendered";
pub(super) const HIDE_EDGE_BORDERS_I3: &str =
    "hide_edge_borders --i3 is unsupported because swayward cannot hide a lone tab's title bar";
pub(super) const SWAP_ID: &str =
    "swap container with id is unsupported because X11 window IDs are unavailable";

/// Sway commands swayward refuses whatever their arguments.
const COMMANDS: &[&str] = &[
    "bindgesture",
    "client.background",
    "client.placeholder",
    "seat",
    "unbindgesture",
];

/// The other `input` subcommands sway runs outside its config file
/// (`sway/sway/commands/input.c:10-44`).
const INPUT_SUBCOMMANDS: &[&str] = &[
    "accel_profile",
    "calibration_matrix",
    "click_method",
    "clickfinger_button_map",
    "drag",
    "drag_lock",
    "dwt",
    "dwtp",
    "events",
    "left_handed",
    "map_from_region",
    "map_to_output",
    "map_to_region",
    "middle_emulation",
    "natural_scroll",
    "pointer_accel",
    "repeat_delay",
    "repeat_rate",
    "rotation_angle",
    "scroll_button",
    "scroll_button_lock",
    "scroll_factor",
    "scroll_method",
    "tap",
    "tap_button_map",
    "tool_mode",
    "xkb_file",
    "xkb_layout",
    "xkb_model",
    "xkb_options",
    "xkb_rules",
    "xkb_variant",
];

/// The other `output` subcommands sway has
/// (`sway/sway/commands/output.c:9-34`).
const OUTPUT_SUBCOMMANDS: &[&str] = &[
    "allow_tearing",
    "background",
    "bg",
    "color_profile",
    "hdr",
    "max_render_time",
    "scale_filter",
    "subpixel",
    "unplug",
];

/// The other options sway's key bindings take
/// (`sway/sway/commands/bind.c:354-377`).
const BIND_OPTIONS: &[&str] = &[
    "--border",
    "--exclude-titlebar",
    "--to-code",
    "--whole-window",
];

/// Whether the rejection `message` of the command `name` with arguments
/// `rest` refuses something sway accepts. `name` is lowercase.
pub(super) fn sway_accepts(name: &str, rest: &[&str], message: &str) -> bool {
    if COMMANDS.contains(&name) {
        return true;
    }
    if [
        URGENT_POLICY,
        GESTURE_BINDINGS,
        BAR_CONFIGURATION,
        BINDSWITCH_RELOAD,
        ADAPTIVE_SYNC_TOGGLE,
        CLIENT_COLOURS,
        HIDE_EDGE_BORDERS_I3,
        SWAP_ID,
    ]
    .contains(&message)
    {
        return true;
    }
    // Sway looks both subcommand tables up case-insensitively
    // (`sway/sway/commands.c:145-149`).
    if name == "input" && message == INPUT_SUBCOMMAND {
        return rest.get(1).is_some_and(|subcommand| {
            INPUT_SUBCOMMANDS.contains(&subcommand.to_ascii_lowercase().as_str())
        });
    }
    if let Some(subcommand) = message
        .strip_prefix("Invalid output subcommand: ")
        .and_then(|rest| rest.strip_suffix('.'))
    {
        return name == "output"
            && OUTPUT_SUBCOMMANDS.contains(&subcommand.to_ascii_lowercase().as_str());
    }
    // The binding parsers name the option they stopped at; this also covers
    // a binding nested in `mode`.
    if let Some((command, option)) = message
        .strip_prefix("unsupported ")
        .and_then(|rest| rest.split_once(" option "))
    {
        return matches!(command, "bindsym" | "bindcode" | "unbindsym" | "unbindcode")
            && BIND_OPTIONS.contains(&option);
    }
    sway_accepts_criteria(message)
}

pub(super) fn sway_accepts_criteria(message: &str) -> bool {
    message.starts_with("X11-only criterion ")
}

#[cfg(test)]
mod tests {
    use crate::command::{parse_with_variables, FocusedNode};

    /// Whether the first rejection in `input` is marked unimplemented.
    /// Every input here must be refused as CMD_INVALID
    fn unimplemented(input: &str) -> bool {
        let failure = parse_with_variables(input, &[])
            .into_iter()
            .find_map(Result::err)
            .unwrap_or_else(|| panic!("{input} is accepted"));
        let unimplemented = failure.is_unimplemented();
        let outcome = failure.resolve(FocusedNode::View);
        assert_eq!(outcome.parse_error, Some(true), "{input}");
        unimplemented
    }

    #[test]
    fn commands_sway_accepts_are_marked_unimplemented() {
        for input in [
            // The probes tests/sway/compatibility.toml records as rejected.
            "bindgesture swipe:3:left nop",
            "unbindgesture swipe:3:left",
            "client.background #000000",
            "client.placeholder #000000 #0c0c0c #ffffff",
            "client.focused #4c7899 #285577 #ffffff #2e9ef4 #285577",
            "client.focused_inactive #333333 #5f676a #ffffff #484e50 #5f676a",
            "client.unfocused #333333 #222222 #888888 #292d2e #222222",
            "client.urgent #2f343a #900000 #ffffff #900000 #900000",
            "seat seat0 hide_cursor 1000",
            // Forms of an implemented command.
            "urgent allow",
            "bar bar-0 position top",
            "bindswitch --reload lid:on nop",
            "bindsym --to-code Mod4+a nop",
            "bindsym --whole-window button1 nop",
            "unbindcode --border 38",
            "mode resize bindsym --to-code Mod4+a nop",
            "mode resize bindgesture swipe:3:left nop",
            "input type:touchpad tap enabled",
            "input * XKB_Layout us",
            "output eDP-1 bg #000000 solid_color",
            "output eDP-1 scale 2 Subpixel rgb",
            "output eDP-1 adaptive_sync toggle",
            "hide_edge_borders --i3 smart",
            "swap container with id 1",
            // X11 criteria, wherever a criteria list is read.
            "[class=\"Firefox\"] kill",
            "for_window [window_role=\"pop-up\"] floating enable",
            "assign [instance=\"term\"] workspace 2",
            "no_focus [window_type=\"dialog\"]",
        ] {
            assert!(unimplemented(input), "{input}");
        }
    }

    #[test]
    fn mistakes_are_not_marked_unimplemented() {
        for input in [
            "kil",
            "resize rgrow width 10px",
            "focus sideways",
            "urgent",
            "bindsym --to-cod Mod4+a nop",
            "bindswitch --to-code lid:on nop",
            "input type:touchpad tapp enabled",
            "input type:touchpad",
            "output eDP-1 backgroundd #000000 solid_color",
            "output * adaptive_sync toggle",
            "hide_edge_borders --i3 sideways",
            "swap container with pid 1",
            "[klass=\"Firefox\"] kill",
            "inhibit_idle always",
        ] {
            assert!(!unimplemented(input), "{input}");
        }
    }
}
