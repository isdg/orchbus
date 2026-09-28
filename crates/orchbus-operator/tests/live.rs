//! Against a real cluster: `ORCHBUS_TEST_KUBECONFIG=<path> cargo test -p orchbus-operator --test live`.
//! Skipped when the variable is unset. It installs the CRDs and uses a throwaway namespace.

use k8s_openapi::api::core::v1::Namespace;
use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use kube::api::{Api, DeleteParams, Patch, PatchParams, PostParams};
use kube::config::{KubeConfigOptions, Kubeconfig};
use kube::runtime::wait::{await_condition, conditions};
use orchbus_api::task::Phase;
use orchbus_api::{Task, TaskSpec};
use std::time::Duration;

async fn client() -> Option<kube::Client> {
    let path = std::env::var_os("ORCHBUS_TEST_KUBECONFIG")?;
    let kc = Kubeconfig::read_from(path).expect("kubeconfig");
    let cfg = kube::Config::from_custom_kubeconfig(kc, &KubeConfigOptions::default()).await.expect("config");
    Some(kube::Client::try_from(cfg).expect("client"))
}

#[tokio::test]
async fn a_new_task_is_observed_as_pending() {
    let Some(client) = client().await else { return };

    let crds: Api<CustomResourceDefinition> = Api::all(client.clone());
    for crd in orchbus_api::crds() {
        let name = crd.metadata.name.clone().unwrap();
        crds.patch(&name, &PatchParams::apply("orchbus-test").force(), &Patch::Apply(&crd)).await.unwrap();
        tokio::time::timeout(Duration::from_secs(20), await_condition(crds.clone(), &name, conditions::is_crd_established()))
            .await
            .expect("CRD established in time")
            .unwrap();
    }

    let ns_name = format!("op-test-{}", std::process::id());
    let namespaces: Api<Namespace> = Api::all(client.clone());
    let ns: Namespace = serde_json::from_value(serde_json::json!({"metadata": {"name": ns_name}})).unwrap();
    namespaces.create(&PostParams::default(), &ns).await.unwrap();

    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let operator = tokio::spawn(orchbus_operator::run(client.clone(), async move {
        let _ = stopped.await;
    }));

    let tasks: Api<Task> = Api::namespaced(client.clone(), &ns_name);
    let spec = TaskSpec { prompt: "fix the flaky test".into(), role: "implementer".into(), workspace: None, target: Default::default(), priority: None };
    tasks.create(&PostParams::default(), &Task::new("fix-flaky", spec)).await.unwrap();

    let pending = |t: Option<&Task>| t.and_then(|t| t.status.as_ref()).and_then(|s| s.phase) == Some(Phase::Pending);
    let seen = tokio::time::timeout(Duration::from_secs(20), await_condition(tasks.clone(), "fix-flaky", pending)).await;

    let managers: Vec<String> = tasks
        .get("fix-flaky")
        .await
        .map(|t| t.metadata.managed_fields.unwrap_or_default())
        .unwrap_or_default()
        .into_iter()
        .filter(|m| m.subresource.as_deref() == Some("status"))
        .filter_map(|m| m.manager)
        .collect();

    let _ = stop.send(());
    let _ = operator.await;
    let _ = namespaces.delete(&ns_name, &DeleteParams::default()).await;
    assert!(seen.is_ok(), "the operator did not set phase Pending within 20s");
    assert_eq!(managers, [orchbus_operator::MANAGER], "status must be written by the operator's field manager");
}
