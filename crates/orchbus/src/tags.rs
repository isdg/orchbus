//! Agent **tags** (roles) — named launch profiles that decide *how* an agent runs
//! for a given step of the loop: which agent, what permission mode, what
//! system-prompt role, interactive pane vs headless.
//!
//! Three built-ins cover the plan → implement → review flow and work with **no
//! config file**. A project can override a built-in or add its own tag in
//! `.orchbus/agents.toml`:
//!
//! ```toml
//! [implement]
//! role = "Prefer the smallest diff that satisfies the plan."
//! skip_perms = true
//! ```

use crate::git;
use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::BTreeMap;

/// A launch profile. Fields map onto `agent::Launch` at spawn/review time.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Tag {
    /// Agent command to run (matches `agent::Agent.command`, e.g. `claude`).
    #[serde(default = "default_agent")]
    pub agent: String,
    /// Extra system-prompt injected via `--append-system-prompt` (empty = none).
    #[serde(default)]
    pub role: String,
    /// Claude `--permission-mode` (e.g. `plan`); empty = default.
    #[serde(default)]
    pub permission_mode: String,
    /// Run headless (`-p --output-format json`) instead of an interactive pane.
    #[serde(default)]
    pub headless: bool,
    /// Bypass permission prompts — gated on isolation at spawn.
    #[serde(default)]
    pub skip_perms: bool,
}

fn default_agent() -> String {
    "claude".to_string()
}

/// The read-only reviewer's role: the "spec + correctness" contract, and the
/// `<review>` block the parser (Track B5) looks for.
const REVIEW_ROLE: &str = "\
You are a strict, read-only code reviewer. You are given the PLAN a change was \
supposed to follow and the DIFF that was produced. Do NOT edit any files. Assess \
two axes: (1) SPEC — does the diff implement what the plan describes, no more and \
no less? (2) CORRECTNESS — bugs, missed edge cases, safety or security issues. \
Report only real problems, most severe first, each as `- [spec|correctness] \
<file>: <one-line problem>`. Wrap the whole list in a <review>...</review> block; \
if there are no problems, emit an empty <review></review>.";

/// Built-in tag for `name`, or `None` if it isn't one of the defaults.
pub fn builtin(name: &str) -> Option<Tag> {
    let base = |permission_mode: &str, role: &str, headless: bool, skip_perms: bool| Tag {
        agent: default_agent(),
        role: role.to_string(),
        permission_mode: permission_mode.to_string(),
        headless,
        skip_perms,
    };
    match name {
        // Entry point: Claude's own plan mode, in a watched pane. No skip — plan
        // mode doesn't edit, and it would conflict with --permission-mode plan.
        "plan" => Some(base("plan", "", false, false)),
        // Reached by accepting the plan in-pane; isolated, so skip permissions.
        "implement" => Some(base("", "", false, true)),
        // Fresh, headless, read-only spec+correctness reviewer.
        "review" => Some(base("", REVIEW_ROLE, true, false)),
        _ => None,
    }
}

/// Resolve a tag by name: a `.orchbus/agents.toml` entry wins over the built-in;
/// otherwise the built-in; otherwise an error listing what's available.
pub fn resolve(name: &str) -> Result<Tag> {
    if let Some(t) = load_config()?.remove(name) {
        return Ok(t);
    }
    builtin(name).with_context(|| {
        format!("unknown tag '{name}' (built-ins: plan, implement, review; or define it in .orchbus/agents.toml)")
    })
}

/// Parse `.orchbus/agents.toml` into a name→Tag map (empty if the file is absent).
fn load_config() -> Result<BTreeMap<String, Tag>> {
    let path = git::orchbus_dir()?.join("agents.toml");
    match std::fs::read_to_string(&path) {
        Ok(body) => {
            toml::from_str(&body).with_context(|| format!("parsing {}", path.display()))
        }
        Err(_) => Ok(BTreeMap::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_cover_the_loop() {
        let plan = builtin("plan").unwrap();
        assert_eq!(plan.permission_mode, "plan");
        assert!(!plan.skip_perms && !plan.headless);

        let implement = builtin("implement").unwrap();
        assert!(implement.skip_perms && implement.permission_mode.is_empty());

        let review = builtin("review").unwrap();
        assert!(review.headless && !review.skip_perms);
        assert!(review.role.contains("<review>"));

        assert!(builtin("nope").is_none());
    }

    #[test]
    fn toml_overrides_parse() {
        let cfg: BTreeMap<String, Tag> =
            toml::from_str("[implement]\nrole = \"tiny diffs\"\nskip_perms = true\n").unwrap();
        let t = &cfg["implement"];
        assert_eq!(t.agent, "claude"); // defaulted
        assert_eq!(t.role, "tiny diffs");
        assert!(t.skip_perms);
    }
}
