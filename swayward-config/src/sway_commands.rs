//! Checks of the sway command strings a configuration carries.
//!
//! A bind's `command` and a window rule's `sway-for-window-command` hold sway
//! commands as plain strings, which the KDL decoder cannot check. They are
//! parsed only when the key is pressed or the window maps, so a misspelt or
//! unimplemented command would otherwise stay unnoticed until then.
use std::fmt;

use swayward_ipc::command::{parse_with_variables, FocusedNode};

use crate::{Action, Binds, Config};

/// Why a sway command string would be refused when run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwayCommandProblemKind {
    Invalid,
    Unimplemented,
}

/// A sway command string that would be refused when run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwayCommandProblem {
    /// The config section holding the command, such as `binds` or `mode "resize"`.
    pub section: String,
    /// The command string as written.
    pub command: String,
    pub kind: SwayCommandProblemKind,
    /// The reply the command would get when run.
    pub error: String,
}

impl fmt::Display for SwayCommandProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            section,
            command,
            kind,
            error,
        } = self;
        match kind {
            SwayCommandProblemKind::Invalid => {
                write!(f, "{section}: invalid sway command {command:?}: {error}")
            }
            SwayCommandProblemKind::Unimplemented => write!(
                f,
                "{section}: sway command {command:?} is not implemented in swayward: {error}"
            ),
        }
    }
}

/// The first command in `command` that sway would refuse whatever is focused.
///
/// The parser rejects only arguments, which sway's handlers refuse as
/// CMD_INVALID or CMD_FAILURE wherever focus is. A handler that checks focus
/// first answers its own precondition error without a view, so the reply is
/// taken with one. A refusal that depends on the tree, such as a move with no
/// container to move, is reached only when the command runs, so it is not
/// reported. A command whose arguments use a variable is skipped: the value is
/// set at run time and may be what makes the arguments valid.
fn refusal(command: &str) -> Option<(SwayCommandProblemKind, String)> {
    parse_with_variables(command, &[])
        .into_iter()
        .filter_map(Result::err)
        .find(|failure| !failure.references_variable())
        .map(|failure| {
            let kind = if failure.is_unimplemented() {
                SwayCommandProblemKind::Unimplemented
            } else {
                SwayCommandProblemKind::Invalid
            };
            let outcome = failure.resolve(FocusedNode::View);
            (kind, outcome.error.unwrap_or_default())
        })
}

impl Config {
    /// Lists the sway command strings in binds, binding modes and window rules
    /// that would be refused when run, in config order.
    pub fn sway_command_problems(&self) -> Vec<SwayCommandProblem> {
        let mut problems = Vec::new();
        let mut check = |section: &str, command: &str| {
            if let Some((kind, error)) = refusal(command) {
                problems.push(SwayCommandProblem {
                    section: section.to_owned(),
                    command: command.to_owned(),
                    kind,
                    error,
                });
            }
        };
        let mut check_binds = |section: &str, binds: &Binds| {
            for bind in &binds.0 {
                if let Action::SwayCommand(command) = &bind.action {
                    check(section, command);
                }
            }
        };
        check_binds("binds", &self.binds);
        for mode in &self.binding_modes {
            check_binds(&format!("mode {:?}", mode.name), &mode.binds);
        }
        for rule in &self.window_rules {
            for command in &rule.sway_for_window_commands {
                check("window-rule", command);
            }
        }
        problems
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn problems(config: &str) -> Vec<SwayCommandProblem> {
        Config::parse_mem(config).unwrap().sway_command_problems()
    }

    fn invalid(config: &str) -> Vec<(String, String, String)> {
        problems(config)
            .into_iter()
            .map(|found| {
                assert_eq!(found.kind, SwayCommandProblemKind::Invalid, "{found}");
                (found.section, found.command, found.error)
            })
            .collect()
    }

    #[test]
    fn valid_command_strings_report_nothing() {
        assert_eq!(
            invalid(
                r#"
                binds {
                    Mod+Return { command "exec foot"; }
                    Mod+Q { command "kill"; }
                    Mod+H { command "focus left; nop chained, mark --add seen"; }
                    Mod+R { command "mode resize"; }
                    Mod+F { command "[app_id=\"foot\"] focus"; }
                    Mod+T { spawn "foot"; }
                    Mod+G { command "title_format \"\""; }
                    Mod+E { command "set $empty \"\""; }
                    Mod+I { command "input * XKB_SWITCH_LAYOUT next"; }
                    Mod+K { command "[con_mark=\"nothing\"] kill"; }
                }
                mode "resize" {
                    Escape { command "mode default"; }
                    L { command "resize grow width 10 px"; }
                }
                window-rule {
                    match app-id="^mpv$"
                    sway-for-window-command "inhibit_idle fullscreen"
                    sway-for-window-command "move scratchpad"
                }
                "#,
            ),
            []
        );
    }

    #[test]
    fn invalid_command_strings_are_listed_in_config_order() {
        let found = invalid(
            r#"
            binds {
                Mod+A { command "kil"; }
                Mod+B { command "focus left; focus sideways; kill"; }
                Mod+C { command "kill"; }
            }
            mode "resize" {
                L { command "resize grow sideways"; }
            }
            window-rule {
                match app-id="^mpv$"
                sway-for-window-command "inhibit_idle always"
            }
            "#,
        );
        let places = found
            .iter()
            .map(|(section, command, _)| (section.as_str(), command.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(
            places,
            [
                ("binds", "kil"),
                ("binds", "focus left; focus sideways; kill"),
                ("mode \"resize\"", "resize grow sideways"),
                ("window-rule", "inhibit_idle always"),
            ]
        );
        assert_eq!(found[0].2, "Unknown/invalid command 'kil'");
        assert_eq!(
            found[3].2,
            "Expected `inhibit_idle focus|fullscreen|open|none|visible`"
        );
        assert!(found.iter().all(|(_, _, error)| !error.is_empty()));
    }

    /// A variable is set at run time, so its command cannot be judged here.
    #[test]
    fn commands_using_variables_are_not_judged() {
        assert_eq!(
            invalid(
                r#"
                binds {
                    Mod+A { command "resize set $width px"; }
                    Mod+B { command "focus left; resize set \"$width\" px"; }
                    Mod+C { command "[app_id=\"^foot$\"] resize set $width px"; }
                }
                "#
            ),
            []
        );
    }

    /// Only a `$` that sway would substitute hides a command: criteria are
    /// split off before substitution, `$$` is a literal dollar even after an
    /// escaped backslash (`sway/sway/config.c:893-909`), and the command
    /// name, quoted or not, is never substituted.
    #[test]
    fn a_dollar_that_is_not_a_variable_is_still_judged() {
        let found = invalid(
            r#"
            binds {
                Mod+A { command "[app_id=\"^foot$\"] kil"; }
                Mod+B { command "mark $$; kil"; }
                Mod+C { command "kil $width"; }
                Mod+D { command "resize set $width px; kil"; }
                Mod+E { command "\"kil x\" $width"; }
                Mod+F { command "focus_follows_mouse \\\\$$width"; }
            }
            "#,
        );
        let errors = found
            .iter()
            .map(|(_, command, error)| (command.as_str(), error.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(
            errors,
            [
                ("[app_id=\"^foot$\"] kil", "Unknown/invalid command 'kil'"),
                ("mark $$; kil", "Unknown/invalid command 'kil'"),
                ("kil $width", "Unknown/invalid command 'kil'"),
                ("resize set $width px; kil", "Unknown/invalid command 'kil'"),
                ("\"kil x\" $width", "Unknown/invalid command '\"kil x\"'"),
                (
                    "focus_follows_mouse \\\\$$width",
                    "Expected 'focus_follows_mouse no|yes|always'"
                ),
            ]
        );
    }

    /// These handlers answer CMD_FAILURE for a bad value before they look at
    /// focus (`sway/sway/commands/focus_follows_mouse.c:17-18`,
    /// `sway/sway/commands/mouse_warping.c:17-18`,
    /// `sway/sway/commands/move.c:674-680`), so sway refuses them whatever
    /// is focused.
    #[test]
    fn argument_failures_are_reported() {
        let found = invalid(
            r#"
            binds {
                Mod+A { command "focus_follows_mouse sideways"; }
                Mod+B { command "mouse_warping sideways"; }
                Mod+C { command "move left 10lightyears"; }
            }
            "#,
        );
        let errors = found
            .iter()
            .map(|(_, command, error)| (command.as_str(), error.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(
            errors,
            [
                (
                    "focus_follows_mouse sideways",
                    "Expected 'focus_follows_mouse no|yes|always'"
                ),
                (
                    "mouse_warping sideways",
                    "Expected 'mouse_warping output|container|none'"
                ),
                ("move left 10lightyears", "Invalid distance specified"),
            ]
        );
    }

    /// Bare `seat` is short of the two arguments sway's handler takes
    /// (`sway/sway/commands/seat.c:55`), so it is a mistake, not a gap.
    #[test]
    fn seat_without_arguments_is_invalid() {
        assert_eq!(
            invalid(r#"binds { Mod+A { command "seat"; }; }"#),
            [(
                "binds".to_owned(),
                "seat".to_owned(),
                "Unknown/invalid command 'seat'".to_owned()
            )]
        );
    }

    /// A command sway accepts is reported as swayward's gap, not as a mistake
    /// in the config.
    #[test]
    fn commands_sway_accepts_are_reported_as_unimplemented() {
        let found = problems(
            r#"
            binds {
                Mod+A { command "seat seat0 hide_cursor 1000"; }
                Mod+B { command "focus left; input type:touchpad tap enabled"; }
            }
            window-rule {
                match app-id="^mpv$"
                sway-for-window-command "urgent allow"
            }
            "#,
        );
        let places = found
            .iter()
            .map(|found| (found.section.as_str(), found.command.as_str(), found.kind))
            .collect::<Vec<_>>();
        assert_eq!(
            places,
            [
                (
                    "binds",
                    "seat seat0 hide_cursor 1000",
                    SwayCommandProblemKind::Unimplemented
                ),
                (
                    "binds",
                    "focus left; input type:touchpad tap enabled",
                    SwayCommandProblemKind::Unimplemented
                ),
                (
                    "window-rule",
                    "urgent allow",
                    SwayCommandProblemKind::Unimplemented
                ),
            ]
        );
    }

    /// Sway requires a value for an X11-only criterion
    /// (`sway/criteria.c:635-642`), so one without is a mistake, while one
    /// with a value is swayward's gap.
    #[test]
    fn x11_criteria_without_a_value_are_invalid() {
        let found = problems(
            r#"
            binds {
                Mod+A { command "[class] kill"; }
                Mod+B { command "[class=\"foo\"] kill"; }
            }
            "#,
        );
        let kinds = found
            .iter()
            .map(|found| (found.command.as_str(), found.kind, found.error.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(
            kinds,
            [
                (
                    "[class] kill",
                    SwayCommandProblemKind::Invalid,
                    "X11-only criterion 'class' is unsupported"
                ),
                (
                    "[class=\"foo\"] kill",
                    SwayCommandProblemKind::Unimplemented,
                    "X11-only criterion 'class' is unsupported"
                ),
            ]
        );
    }

    #[test]
    fn display_names_the_section_command_and_reply() {
        let mut found = SwayCommandProblem {
            section: "binds".into(),
            command: "kil".into(),
            kind: SwayCommandProblemKind::Invalid,
            error: "Unknown/invalid command 'kil'".into(),
        };
        assert_eq!(
            found.to_string(),
            r#"binds: invalid sway command "kil": Unknown/invalid command 'kil'"#
        );
        found.command = "seat seat0 hide_cursor 1000".into();
        found.kind = SwayCommandProblemKind::Unimplemented;
        found.error = "Unknown/invalid command 'seat'".into();
        assert_eq!(
            found.to_string(),
            r#"binds: sway command "seat seat0 hide_cursor 1000" is not implemented in swayward: Unknown/invalid command 'seat'"#
        );
    }
}
