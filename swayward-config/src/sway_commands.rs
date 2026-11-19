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

/// The first command in `command` that would be refused as invalid.
///
/// Only CMD_INVALID counts. A CMD_FAILURE depends on what is focused when the
/// command runs, so it says nothing about the config. A command that uses a
/// variable is skipped: its value is set at run time and may be what makes the
/// arguments valid.
fn refusal(command: &str) -> Option<(SwayCommandProblemKind, String)> {
    if command.contains('$') {
        return None;
    }
    parse_with_variables(command, &[])
        .into_iter()
        .filter_map(Result::err)
        .find_map(|failure| {
            let kind = if failure.is_unimplemented() {
                SwayCommandProblemKind::Unimplemented
            } else {
                SwayCommandProblemKind::Invalid
            };
            // A bind's command runs with whatever is focused; a view is the
            // case where sway's handlers get as far as their arguments.
            let outcome = failure.resolve(FocusedNode::View);
            (outcome.parse_error == Some(true)).then(|| (kind, outcome.error.unwrap_or_default()))
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
            invalid(r#"binds { Mod+A { command "resize set $width px"; }; }"#),
            []
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
                Mod+C { command "seat"; }
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
                ("binds", "seat", SwayCommandProblemKind::Unimplemented),
                (
                    "window-rule",
                    "urgent allow",
                    SwayCommandProblemKind::Unimplemented
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
