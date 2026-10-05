use super::*;

/// `bar_handlers` and `bar_config_handlers` (`sway/sway/commands/bar.c:10-46`),
/// matched case-insensitively as sway's `find_handler` does.
const BAR_SUBCOMMANDS: &[&str] = &[
    "bindcode",
    "binding_mode_indicator",
    "bindsym",
    "colors",
    "font",
    "gaps",
    "height",
    "hidden_state",
    "icon_theme",
    "id",
    "mode",
    "modifier",
    "output",
    "pango_markup",
    "position",
    "separator_symbol",
    "status_command",
    "status_edge_padding",
    "status_padding",
    "strip_workspace_name",
    "strip_workspace_numbers",
    "swaybar_command",
    "tray_bindcode",
    "tray_bindsym",
    "tray_output",
    "tray_padding",
    "unbindcode",
    "unbindsym",
    "workspace_buttons",
    "workspace_min_width",
    "wrap_scroll",
];

fn is_subcommand(name: &str) -> bool {
    BAR_SUBCOMMANDS
        .iter()
        .any(|command| command.eq_ignore_ascii_case(name))
}

/// Runtime `bar`, answered as sway answers it with no bar configured.
///
/// Swayward manages no bar, so `config->bars` is always empty and
/// `config->current_bar` is NULL at runtime. `bar mode` and
/// `bar hidden_state` then iterate over zero bars and succeed without effect
/// (`sway/sway/commands/bar.c:54-136`, `bar/mode.c:40-77`,
/// `bar/hidden_state.c:36-74`). `bar <id> <subcommand>` would create a bar
/// and launch swaybar, which swayward cannot do, so it is refused
/// (docs/KNOWN_DEVIATIONS.md#bars).
pub(super) fn parse_bar(rest: &[&str]) -> Result<Command, String> {
    checkarg(rest.len(), "bar", Expected::AtLeast(2))?;
    let [first, second, ..] = rest else {
        return Err(arity_error(rest.len(), "bar", Expected::AtLeast(2)));
    };
    if *first != "id" && is_subcommand(second) {
        return Err(
            "bar configuration is unsupported because swayward does not manage bars".into(),
        );
    }
    // These two comparisons are case-sensitive strcmp in sway.
    if *first != "mode" && *first != "hidden_state" {
        return Err(if is_subcommand(first) {
            "No bar defined.".into()
        } else {
            // Sway names argv[1] here, not the unknown argv[0].
            format!("Unknown/invalid command '{second}'")
        });
    }
    let args = rest.len() - 1;
    checkarg(args, first, Expected::AtMost(2))?;
    Ok(Command::Nop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_bar_matches_sway_with_no_bar_configured() {
        for accepted in [
            "bar mode hide",
            "bar mode bogus",
            "bar hidden_state show",
            "bar hidden_state toggle bar-0",
            "bar mode dock missing-id",
        ] {
            let words = accepted.split(' ').collect::<Vec<_>>();
            assert_eq!(parse_bar(&words[1..]), Ok(Command::Nop), "{accepted}");
        }
        for (input, error) in [
            (
                "bar mode",
                "Invalid bar command (expected at least 2 arguments, got 1)",
            ),
            (
                "bar mode hide bar-0 extra",
                "Invalid mode command (expected at most 2 arguments, got 3)",
            ),
            ("bar position top", "No bar defined."),
            ("bar MODE hide", "No bar defined."),
            ("bar id bar-0", "No bar defined."),
            ("bar oracle value", "Unknown/invalid command 'value'"),
            (
                "bar bar-0 mode hide",
                "bar configuration is unsupported because swayward does not manage bars",
            ),
            (
                "bar mode HIDDEN_STATE show",
                "bar configuration is unsupported because swayward does not manage bars",
            ),
        ] {
            let words = input.split(' ').collect::<Vec<_>>();
            assert_eq!(parse_bar(&words[1..]), Err(error.into()), "{input}");
        }
    }
}
