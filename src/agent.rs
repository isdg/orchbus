//! What agent is running in a pane? Detected from tmux's `pane_current_command`.
//!
//! Today the cockpit only drives Claude Code, but we plan to scale to other
//! terminal agents — so the running agent is a first-class, extensible mapping
//! here rather than a hardcoded `== "claude"` check scattered through `scan`. To
//! support another agent, add an arm below; its tag is carried on every `Row`
//! and shown as a column in the cockpit so you can tell sessions apart at a
//! glance.

/// Short tag for the agent behind `pane_current_command`, or `None` if it isn't
/// an agent the cockpit knows how to drive.
pub fn detect(pane_current_command: &str) -> Option<&'static str> {
    match pane_current_command {
        "claude" => Some("CC"), // Claude Code
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_is_cc_others_unknown() {
        assert_eq!(detect("claude"), Some("CC"));
        assert_eq!(detect("bash"), None);
        assert_eq!(detect("vim"), None);
    }
}
