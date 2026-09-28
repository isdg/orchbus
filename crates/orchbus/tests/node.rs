//! `orchbus node` against a real, private tmux server (skipped when tmux is absent).

use serde_json::{json, Value};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

struct Tmux {
    dir: PathBuf,
}

impl Tmux {
    fn new() -> Option<Tmux> {
        Command::new("tmux").arg("-V").output().ok()?;
        let dir = std::env::temp_dir().join(format!("orchbus-node-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).ok()?;
        Some(Tmux { dir })
    }

    fn node(&self, verb: &str, req: Value) -> (bool, Value) {
        let mut child = Command::new(env!("CARGO_BIN_EXE_orchbus"))
            .args(["node", verb])
            .env("TMUX_TMPDIR", &self.dir)
            .env_remove("TMUX")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(req.to_string().as_bytes()).unwrap();
        let out = child.wait_with_output().unwrap();
        (out.status.success(), serde_json::from_slice(&out.stdout).expect("stdout is one JSON value"))
    }
}

impl Drop for Tmux {
    fn drop(&mut self) {
        let _ = Command::new("tmux").arg("kill-server").env("TMUX_TMPDIR", &self.dir).env_remove("TMUX").output();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn agent(uid: &str, script: &str) -> Value {
    json!({
        "uid": uid, "session": "demo", "name": format!("agent-{uid}"), "cwd": "/",
        "argv": ["sh", "-c", script], "env": {"ORCHBUS_POD_UID": uid}
    })
}

#[test]
fn pod_lifecycle_verbs_drive_a_real_tmux_server() {
    let Some(t) = Tmux::new() else { return };

    assert_eq!(t.node("version", json!({})), (true, json!({"contract": "v0"})));
    assert_eq!(t.node("list-panes", json!({})), (true, json!([])), "no server yet is an empty list");

    let menu = r#"printf 'Bash(rm -rf build)\n Do you want to proceed?\n ❯ 1. Yes\n   2. No\n'; echo "uid=$ORCHBUS_POD_UID"; sleep 30"#;
    let (ok, r) = t.node("spawn", agent("u1", menu));
    assert!(ok, "{r}");
    let pane = r["pane"].as_str().unwrap().to_string();
    assert!(pane.starts_with('%'));
    assert_eq!(t.node("spawn", agent("u1", menu)).1["pane"], pane, "spawn is idempotent per uid");

    let (ok, second) = t.node("spawn", agent("u2", "sleep 30"));
    assert!(ok, "second agent joins the existing session: {second}");
    assert_ne!(second["pane"], pane);

    let deadline = Instant::now() + Duration::from_secs(5);
    let st = loop {
        let (_, st) = t.node("state", json!({"uid": "u1"}));
        if st["state"] == "approve" || Instant::now() > deadline {
            break st;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    assert_eq!(st, json!({"pane": pane, "state": "approve", "question": "Do you want to proceed?"}));

    let (_, panes) = t.node("list-panes", json!({}));
    let uids: Vec<&str> = panes.as_array().unwrap().iter().map(|p| p["uid"].as_str().unwrap()).collect();
    assert_eq!(uids, ["u1", "u2"]);

    assert_eq!(t.node("kill", json!({"uid": "u1"})), (true, json!({"killed": true})));
    assert_eq!(t.node("kill", json!({"uid": "u1"})), (true, json!({"killed": false})));

    let (ok, err) = t.node("state", json!({"uid": "u1"}));
    assert!(!ok && err["error"].as_str().unwrap().contains("no pane for uid u1"));
    let (ok, err) = t.node("spawn", json!({"uid": "u9"}));
    assert!(!ok && err["error"].as_str().unwrap().contains("parsing request"));
}
