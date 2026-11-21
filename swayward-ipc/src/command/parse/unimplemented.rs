//! Which parser rejections refuse a command that sway accepts.
//!
//! Every rule here names what sway accepts, so a misspelt command is never
//! reported as merely unimplemented.

use std::ops::RangeInclusive;

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

/// Sway commands swayward refuses whatever their arguments, with the fewest
/// arguments sway's handler accepts: `sway/sway/commands/gesture.c:99-109`
/// and `sway/sway/commands/client.c:79-82`.
const COMMANDS: &[(&str, usize)] = &[
    ("bindgesture", 2),
    ("client.background", 0),
    ("client.placeholder", 0),
    ("unbindgesture", 1),
];

const ONE: RangeInclusive<usize> = 1..=1;
const AT_LEAST_ONE: RangeInclusive<usize> = 1..=usize::MAX;

/// The `seat` subcommands (`sway/sway/commands/seat.c:11-27`), with the
/// argument counts each handler's `checkarg` accepts
/// (`sway/sway/commands/seat/*.c`).
const SEAT_SUBCOMMANDS: &[(&str, RangeInclusive<usize>)] = &[
    ("attach", AT_LEAST_ONE),
    ("cursor", 2..=usize::MAX),
    ("fallback", ONE),
    ("hide_cursor", 1..=2),
    ("idle_inhibit", AT_LEAST_ONE),
    ("idle_wake", AT_LEAST_ONE),
    ("keyboard_grouping", ONE),
    ("pointer_constraint", ONE),
    ("shortcuts_inhibitor", ONE),
    ("xcursor_theme", 1..=2),
];

/// The other `input` subcommands sway runs outside its config file
/// (`sway/sway/commands/input.c:10-44`), with the argument counts each
/// handler's `checkarg` accepts (`sway/sway/commands/input/*.c`).
const INPUT_SUBCOMMANDS: &[(&str, RangeInclusive<usize>)] = &[
    ("accel_profile", AT_LEAST_ONE),
    ("calibration_matrix", 6..=6),
    ("click_method", AT_LEAST_ONE),
    ("clickfinger_button_map", AT_LEAST_ONE),
    ("drag", AT_LEAST_ONE),
    ("drag_lock", AT_LEAST_ONE),
    ("dwt", AT_LEAST_ONE),
    ("dwtp", AT_LEAST_ONE),
    ("events", AT_LEAST_ONE),
    ("left_handed", AT_LEAST_ONE),
    ("map_from_region", 2..=2),
    ("map_to_output", ONE),
    ("map_to_region", 4..=4),
    ("middle_emulation", AT_LEAST_ONE),
    ("natural_scroll", AT_LEAST_ONE),
    ("pointer_accel", AT_LEAST_ONE),
    ("repeat_delay", ONE),
    ("repeat_rate", ONE),
    ("rotation_angle", AT_LEAST_ONE),
    ("scroll_button", AT_LEAST_ONE),
    ("scroll_button_lock", AT_LEAST_ONE),
    ("scroll_factor", AT_LEAST_ONE),
    ("scroll_method", AT_LEAST_ONE),
    ("tap", AT_LEAST_ONE),
    ("tap_button_map", AT_LEAST_ONE),
    ("tool_mode", 2..=usize::MAX),
    ("xkb_file", ONE),
    ("xkb_layout", ONE),
    ("xkb_model", ONE),
    ("xkb_options", ONE),
    ("xkb_rules", ONE),
    ("xkb_variant", ONE),
];

/// Whether `args`, a subcommand and its arguments, names an entry of
/// `table`, matched case-insensitively as sway's `find_handler` does
/// (`sway/sway/commands.c:145-160`), with an argument count it accepts.
fn subcommand_accepts(table: &[(&str, RangeInclusive<usize>)], args: &[&str]) -> bool {
    let Some((name, values)) = args.split_first() else {
        return false;
    };
    table.iter().any(|(handler, counts)| {
        handler.eq_ignore_ascii_case(name) && counts.contains(&values.len())
    })
}

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
pub(super) fn sway_accepts(name: &str, rest: &[&str], message: &str, input: &str) -> bool {
    if let Some((_, min_args)) = COMMANDS.iter().find(|(command, _)| *command == name) {
        return rest.len() >= *min_args;
    }
    if message == GESTURE_BINDINGS {
        // A gesture binding nested in `mode`, after the mode name.
        return rest.iter().enumerate().skip(1).any(|(index, word)| {
            let word = word.to_ascii_lowercase();
            COMMANDS.iter().any(|(command, min_args)| {
                command.ends_with("gesture")
                    && *command == word
                    && rest.len().saturating_sub(index + 1) >= *min_args
            })
        });
    }
    if [
        URGENT_POLICY,
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
    if name == "seat" {
        return rest
            .get(1..)
            .is_some_and(|args| subcommand_accepts(SEAT_SUBCOMMANDS, args));
    }
    if name == "input" && message == INPUT_SUBCOMMAND {
        return rest
            .get(1..)
            .is_some_and(|args| subcommand_accepts(INPUT_SUBCOMMANDS, args));
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
    sway_accepts_criteria(message, input)
}

/// Whether `message` refuses an X11-only criterion in the first criteria
/// block of `input` that sway would accept.
pub(super) fn sway_accepts_criteria(message: &str, input: &str) -> bool {
    message.starts_with("X11-only criterion ")
        && input.find('[').is_some_and(|start| {
            input.get(start..).is_some_and(|criteria| {
                super::super::lexer::criteria_end(criteria)
                    .and_then(|end| criteria.get(..=end))
                    .is_some_and(crate::criteria::Criteria::sway_accepts_x11_only)
            })
        })
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
            "seat * hide_cursor when-typing enable",
            "seat - cursor move 10 10",
            // Forms of an implemented command.
            "urgent allow",
            "bar bar-0 position top",
            "bindswitch --reload lid:on nop",
            "bindsym --to-code Mod4+a nop",
            "bindsym --whole-window button1 nop",
            "unbindcode --border 38",
            "mode resize bindsym --to-code Mod4+a nop",
            "mode resize bindgesture swipe:3:left nop",
            "mode resize unbindgesture swipe:3:left",
            "input type:touchpad tap enabled",
            "input type:touchpad TAP enabled",
            "input type:touchpad calibration_matrix 1 0 0 0 1 0",
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
            "[class=\"Firefox\" app_id=\"foot\"] kill",
            "[id=42] kill",
        ] {
            assert!(unimplemented(input), "{input}");
        }
    }

    #[test]
    fn mistakes_are_not_marked_unimplemented() {
        for input in [
            "kil",
            // Fewer arguments than sway's handler takes.
            "seat",
            "seat seat0",
            "seat seat0 hide_cursorr 1000",
            "seat seat0 fallback",
            "seat seat0 hide_cursor when-typing enable 1000",
            "input * tap",
            "input * calibration_matrix 1 0 0",
            "bindgesture swipe:3:left",
            "unbindgesture",
            "mode resize bindgesture swipe:3:left",
            "input *",
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
            // Sway requires a value (`sway/criteria.c:635-642`) and checks
            // every token.
            "[class] kill",
            "for_window [window_role] floating enable",
            "[id=x] kill",
            "[class=\"Firefox\" bogus=\"x\"] kill",
            "inhibit_idle always",
        ] {
            assert!(!unimplemented(input), "{input}");
        }
    }
}
