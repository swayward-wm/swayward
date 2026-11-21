use std::ffi::OsString;
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use clap_complete::Shell;

use crate::utils::{long_version, version};

#[derive(Parser)]
#[command(author, version = version(), long_version = long_version(), about, long_about = None)]
#[command(args_conflicts_with_subcommands = true)]
#[command(subcommand_value_name = "SUBCOMMAND")]
#[command(subcommand_help_heading = "Subcommands")]
pub struct Cli {
    /// Path to config file (default: `$XDG_CONFIG_HOME/swayward/config.kdl`).
    ///
    /// This can also be set with the `SWAYWARD_CONFIG` environment variable. If both are set, the
    /// command line argument takes precedence.
    #[arg(short, long)]
    pub config: Option<PathBuf>,
    /// Import environment globally to systemd and D-Bus, run D-Bus services.
    ///
    /// Set this flag in a systemd service started by your display manager, or when running
    /// manually as your main compositor instance. Do not set when running as a nested window, or
    /// on a TTY as your non-main compositor instance, to avoid messing up the global environment.
    #[arg(long)]
    pub session: bool,
    /// Command to run upon compositor startup.
    #[arg(last = true)]
    pub command: Vec<OsString>,

    #[command(subcommand)]
    pub subcommand: Option<Sub>,
}

#[cfg(test)]
mod tests {
    use clap::error::ErrorKind;

    use super::*;

    #[test]
    fn version_flag_reports_the_swayward_version_then_the_niri_base() {
        let Err(error) = Cli::try_parse_from(["swayward", "--version"]) else {
            panic!("--version should exit through clap")
        };

        assert_eq!(error.kind(), ErrorKind::DisplayVersion);
        assert_eq!(
            error.to_string(),
            format!(
                "swayward {}\nniri base: {}\n",
                crate::utils::version(),
                crate::utils::NIRI_BASE
            )
        );
        assert!(
            crate::utils::version().starts_with(&format!("{} (", crate::utils::SWAYWARD_VERSION))
        );
    }

    #[test]
    fn short_version_flag_prints_only_the_version_line() {
        let Err(error) = Cli::try_parse_from(["swayward", "-V"]) else {
            panic!("-V should exit through clap")
        };

        assert_eq!(error.kind(), ErrorKind::DisplayVersion);
        assert_eq!(
            error.to_string(),
            format!("swayward {}\n", crate::utils::version())
        );
    }

    #[test]
    fn niri_base_matches_the_fork_base_record() {
        let record = include_str!("../docs/FORK-BASE.md");
        let commit = record
            .split("commit ")
            .nth(1)
            .and_then(|rest| rest.get(..40))
            .expect("docs/FORK-BASE.md names the full base commit");
        let short = crate::utils::NIRI_BASE.split_whitespace().next().unwrap();

        assert!(
            short.len() >= 8 && commit.starts_with(short),
            "NIRI_BASE {:?} does not match docs/FORK-BASE.md (commit {commit})",
            crate::utils::NIRI_BASE
        );
    }
}

#[derive(Subcommand)]
pub enum Sub {
    /// The legacy msg client was removed; use swaymsg.
    #[command(hide = true)]
    Msg {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// Validate the config file.
    Validate {
        /// Path to config file (default: `$XDG_CONFIG_HOME/swayward/config.kdl`).
        ///
        /// This can also be set with the `SWAYWARD_CONFIG` environment variable. If both are set,
        /// the command line argument takes precedence.
        #[arg(short, long)]
        config: Option<PathBuf>,
        /// Fail, instead of warning, when a bind or window rule holds a sway command that would be
        /// refused.
        #[arg(long)]
        strict: bool,
    },
    /// Cause a panic to check if the backtraces are good.
    Panic,
    /// Generate shell completions.
    Completions { shell: CompletionShell },
}

#[derive(Clone, Debug, clap::ValueEnum)]
pub enum CompletionShell {
    Bash,
    Elvish,
    Fish,
    PowerShell,
    Zsh,
    Nushell,
}

impl TryFrom<CompletionShell> for Shell {
    type Error = &'static str;

    fn try_from(shell: CompletionShell) -> Result<Self, Self::Error> {
        match shell {
            CompletionShell::Bash => Ok(Shell::Bash),
            CompletionShell::Elvish => Ok(Shell::Elvish),
            CompletionShell::Fish => Ok(Shell::Fish),
            CompletionShell::PowerShell => Ok(Shell::PowerShell),
            CompletionShell::Zsh => Ok(Shell::Zsh),
            CompletionShell::Nushell => Err("Nushell should be handled separately"),
        }
    }
}
