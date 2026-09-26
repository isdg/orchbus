use orchbus_api::ApprovalSpec;
use serde_json::{json, Value};

fn spec(decision: &str) -> Value {
    json!({
        "kind": "permission",
        "question": "Allow Bash(rm -rf build)?",
        "options": ["1. Yes", "2. No"],
        "subject": {"pod": "worker-a-i1", "chain": ["system:serviceaccount:platform:worker-a"]},
        "decision": decision,
        "choice": 1
    })
}

#[test]
fn approving_a_pending_request_is_allowed() {
    assert!(ApprovalSpec::validate_cel(&spec("approved"), Some(&spec("pending"))).is_ok());
}

#[test]
fn a_decision_is_final() {
    assert!(ApprovalSpec::validate_cel(&spec("denied"), Some(&spec("approved"))).is_err());
    assert!(ApprovalSpec::validate_cel(&spec("pending"), Some(&spec("denied"))).is_err());
}

#[test]
fn subject_and_kind_are_immutable() {
    let mut rewritten = spec("pending");
    rewritten["subject"]["chain"] = json!([]);
    assert!(ApprovalSpec::validate_cel(&rewritten, Some(&spec("pending"))).is_err());
    let mut rekinded = spec("pending");
    rekinded["kind"] = json!("land");
    assert!(ApprovalSpec::validate_cel(&rekinded, Some(&spec("pending"))).is_err());
}

#[test]
fn approving_a_menu_needs_a_choice() {
    let mut no_choice = spec("approved");
    no_choice.as_object_mut().unwrap().remove("choice");
    assert!(ApprovalSpec::validate_cel(&no_choice, Some(&spec("pending"))).is_err());
}
