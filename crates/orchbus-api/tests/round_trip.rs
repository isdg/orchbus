use orchbus_api::{Task, TaskSpec};

#[test]
fn a_manifest_parses_with_defaults_and_serializes_back_unchanged() {
    let manifest = serde_json::json!({
        "apiVersion": "orchbus.io/v1alpha1",
        "kind": "Task",
        "metadata": {"name": "fix-flaky-retry", "namespace": "platform"},
        "spec": {"prompt": "Fix the flaky retry test", "role": "implementer"}
    });
    let task: Task = serde_json::from_value(manifest).unwrap();
    assert_eq!(task.spec.target, orchbus_api::task::Target::Ready);
    assert_eq!(task.spec.workspace, None);

    let back = serde_json::to_value(&task).unwrap();
    assert_eq!(back["spec"], serde_json::json!({"prompt": "Fix the flaky retry test", "role": "implementer", "target": "ready"}));
    let again: Task = serde_json::from_value(back).unwrap();
    assert_eq!(again.spec, task.spec);
}

#[test]
fn new_builds_an_object_with_the_right_type_meta() {
    let t = Task::new("x", TaskSpec { prompt: "p".into(), role: "r".into(), workspace: None, target: Default::default(), priority: None });
    let v = serde_json::to_value(&t).unwrap();
    assert_eq!(v["apiVersion"], "orchbus.io/v1alpha1");
    assert_eq!(v["kind"], "Task");
}
