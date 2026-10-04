//! Parser rejections that carry the reply kind sway's handler gives them.
//!
//! Sway's handlers do not all report bad arguments the same way. Most return
//! CMD_INVALID, which stops a command list, but some return CMD_FAILURE, and a
//! few check for a focused container before they look at their arguments.
//! The parser knows which handler it is standing in for, so it records that
//! here instead of leaving dispatch to recognise the message text.

use super::FocusedNode;
use crate::CommandOutcome;

/// The reply kind of a sway handler result: CMD_INVALID or CMD_FAILURE.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ErrorKind {
    Invalid,
    Failure,
}

/// A parser rejection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ParseError {
    message: String,
    kind: ErrorKind,
    /// What sway's handler answers instead when the focused node is less
    /// than it needs, for handlers that check focus before their arguments.
    without_view: Option<(FocusedNode, &'static str, ErrorKind)>,
}

impl ParseError {
    /// Report this as CMD_FAILURE, which does not stop the rest of a
    /// command list.
    pub(super) fn into_failure(mut self) -> Self {
        self.kind = ErrorKind::Failure;
        self
    }

    #[cfg(test)]
    pub(super) fn into_message(self) -> String {
        self.message
    }

    pub(super) fn message_is(&self, message: &str) -> bool {
        self.message == message
    }

    /// Answer `message` of `kind` instead when no view is focused, because
    /// sway's handler checks `container->view` before it parses arguments.
    pub(super) fn unless_view(mut self, message: &'static str, kind: ErrorKind) -> Self {
        self.without_view = Some((FocusedNode::View, message, kind));
        self
    }

    /// Answer `message` of `kind` instead when no container is focused,
    /// because sway's handler checks for one before it parses arguments.
    pub(super) fn unless_container(mut self, message: &'static str, kind: ErrorKind) -> Self {
        self.without_view = Some((FocusedNode::Container, message, kind));
        self
    }

    pub(super) fn into_outcome(self, focused: FocusedNode) -> CommandOutcome {
        let (message, kind) = match self.without_view {
            Some((needed, message, kind)) if focused < needed => (message.to_owned(), kind),
            _ => (self.message, self.kind),
        };
        CommandOutcome {
            success: false,
            error: Some(message),
            parse_error: Some(kind == ErrorKind::Invalid),
        }
    }
}

impl From<String> for ParseError {
    fn from(message: String) -> Self {
        Self {
            message,
            kind: ErrorKind::Invalid,
            without_view: None,
        }
    }
}

impl From<&str> for ParseError {
    fn from(message: &str) -> Self {
        Self::from(message.to_owned())
    }
}
