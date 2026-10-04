//! Sway's argument-count check, shared by every handler that uses it.

/// The comparison a sway handler asks `checkarg` for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Expected {
    AtLeast(usize),
    AtMost(usize),
    EqualTo(usize),
}

impl Expected {
    fn holds(self, argc: usize) -> bool {
        match self {
            Self::AtLeast(val) => argc >= val,
            Self::AtMost(val) => argc <= val,
            Self::EqualTo(val) => argc == val,
        }
    }
}

/// Sway's `checkarg`: `Ok` when `argc` satisfies `expected`, otherwise the
/// CMD_INVALID text it builds (`sway/sway/commands.c:18-41`).
pub(super) fn checkarg(argc: usize, name: &str, expected: Expected) -> Result<(), String> {
    if expected.holds(argc) {
        Ok(())
    } else {
        Err(arity_error(argc, name, expected))
    }
}

/// The text `checkarg` reports when `argc` fails `expected`, for callers that
/// have already matched the argument slice.
pub(super) fn arity_error(argc: usize, name: &str, expected: Expected) -> String {
    let (qualifier, val) = match expected {
        Expected::AtLeast(val) => ("at least ", val),
        Expected::AtMost(val) => ("at most ", val),
        Expected::EqualTo(val) => ("", val),
    };
    format!(
        "Invalid {name} command (expected {qualifier}{val} argument{}, got {argc})",
        if val == 1 { "" } else { "s" }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkarg_reproduces_sways_wording() {
        assert_eq!(checkarg(1, "split", Expected::EqualTo(1)), Ok(()));
        assert_eq!(checkarg(2, "assign", Expected::AtLeast(2)), Ok(()));
        assert_eq!(checkarg(5, "client.focused", Expected::AtMost(5)), Ok(()));
        for (argc, name, expected, message) in [
            (
                0,
                "exec",
                Expected::AtLeast(1),
                "Invalid exec command (expected at least 1 argument, got 0)",
            ),
            (
                1,
                "assign",
                Expected::AtLeast(2),
                "Invalid assign command (expected at least 2 arguments, got 1)",
            ),
            (
                2,
                "split",
                Expected::EqualTo(1),
                "Invalid split command (expected 1 argument, got 2)",
            ),
            (
                1,
                "exit",
                Expected::EqualTo(0),
                "Invalid exit command (expected 0 arguments, got 1)",
            ),
            (
                6,
                "client.focused",
                Expected::AtMost(5),
                "Invalid client.focused command (expected at most 5 arguments, got 6)",
            ),
        ] {
            assert_eq!(checkarg(argc, name, expected), Err(message.to_owned()));
        }
    }
}
