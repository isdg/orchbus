//! The keystrokes that drive an agent's TUI, as a sequence of `tmux send-keys` calls.

/// One `tmux send-keys` call: literal text (`-l`) or a named key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Keys {
    Literal(String),
    Key(&'static str),
}

impl Keys {
    /// The arguments after `send-keys -t <pane>`.
    pub fn args(&self) -> Vec<&str> {
        match self {
            Keys::Literal(s) => vec!["-l", s],
            Keys::Key(k) => vec![k],
        }
    }
}

/// Accept the highlighted option, or pick menu option `choice` (1-9). The digit and the
/// Enter go as separate calls so tmux flushes between them, avoiding the TUI's debounce.
pub fn approve(choice: Option<u8>) -> Vec<Keys> {
    match choice {
        None => vec![Keys::Key("Enter")],
        Some(d) => vec![Keys::Literal(d.to_string()), Keys::Key("Enter")],
    }
}

/// Dismiss a prompt, or interrupt the running turn.
pub fn escape() -> Vec<Keys> {
    vec![Keys::Key("Escape")]
}

/// Type one line and submit it.
pub fn send(text: &str) -> Vec<Keys> {
    vec![Keys::Literal(text.to_string()), Keys::Key("Enter")]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approve_default_is_a_bare_enter() {
        assert_eq!(approve(None), [Keys::Key("Enter")]);
    }

    #[test]
    fn approve_choice_types_the_digit_then_enter_separately() {
        let k = approve(Some(2));
        assert_eq!(k, [Keys::Literal("2".into()), Keys::Key("Enter")]);
        assert_eq!(k[0].args(), ["-l", "2"]);
    }

    #[test]
    fn send_submits_the_line() {
        assert_eq!(send("fix it"), [Keys::Literal("fix it".into()), Keys::Key("Enter")]);
    }
}
