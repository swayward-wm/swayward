//! The version string swayward and swaywardmsg report.
//!
//! The compositor prints it for `--version` and in `GET_VERSION`'s
//! `human_readable`; swaywardmsg prints it for `--version`. Both binaries take
//! it from here, so they agree on the release name and the build commit.

use git_version::git_version;

/// Swayward's human release name. Cargo carries the same release as ordered SemVer
/// (beta2 is `26.10.0-beta.2`).
pub const SWAYWARD_VERSION: &str = "beta2";

/// The release name followed by the commit this binary was built from.
///
/// `SWAYWARD_BUILD_VERSION_STRING` at build time replaces the whole string.
/// Otherwise the commit is `SWAYWARD_BUILD_COMMIT`, which packagers set when
/// the build has no Git repository, then `git describe`, then
/// `unknown commit`. An empty variable counts as unset.
pub fn version() -> String {
    format_version(
        option_env!("SWAYWARD_BUILD_VERSION_STRING"),
        option_env!("SWAYWARD_BUILD_COMMIT"),
        git_version!(fallback = "unknown commit"),
    )
}

fn format_version(override_string: Option<&str>, commit: Option<&str>, described: &str) -> String {
    if let Some(string) = override_string.filter(|s| !s.is_empty()) {
        return string.to_owned();
    }
    let commit = commit.filter(|s| !s.is_empty()).unwrap_or(described);
    format!("{SWAYWARD_VERSION} ({commit})")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_override_replaces_the_whole_string() {
        assert_eq!(
            format_version(Some("beta2 (packaged)"), Some("1a2b3c4d"), "described"),
            "beta2 (packaged)"
        );
    }

    #[test]
    fn a_packager_commit_beats_git_describe() {
        assert_eq!(
            format_version(None, Some("1a2b3c4d"), "described"),
            format!("{SWAYWARD_VERSION} (1a2b3c4d)")
        );
    }

    #[test]
    fn without_a_packager_commit_git_describe_or_its_fallback_is_used() {
        assert_eq!(
            format_version(None, None, "swayward-v26.10.0-beta.2-12-g1a2b3c4d"),
            format!("{SWAYWARD_VERSION} (swayward-v26.10.0-beta.2-12-g1a2b3c4d)")
        );
        assert_eq!(
            format_version(None, None, "unknown commit"),
            format!("{SWAYWARD_VERSION} (unknown commit)")
        );
    }

    #[test]
    fn empty_variables_count_as_unset() {
        assert_eq!(
            format_version(Some(""), Some(""), "unknown commit"),
            format!("{SWAYWARD_VERSION} (unknown commit)")
        );
    }
}
