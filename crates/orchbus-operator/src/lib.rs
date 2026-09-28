//! The orchbus operator: watches Tasks and drives them toward their target.

use futures::StreamExt;
use kube::api::{Api, Patch, PatchParams};
use kube::runtime::controller::{Action, Controller};
use kube::runtime::watcher;
use kube::{Client, ResourceExt};
use orchbus_api::task::Phase;
use orchbus_api::{Task, TaskStatus};
use std::sync::Arc;
use std::time::Duration;

/// Field manager for every server-side apply the operator makes.
pub const MANAGER: &str = "orchbus-operator";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("kube: {0}")]
    Kube(#[from] kube::Error),
    #[error("task has no namespace")]
    NoNamespace,
}

pub struct Context {
    pub client: Client,
}

/// The status a Task should have next, or `None` when it already has it.
pub fn next_status(task: &Task) -> Option<TaskStatus> {
    let status = task.status.clone().unwrap_or_default();
    if status.phase.is_some() {
        return None;
    }
    Some(TaskStatus { phase: Some(Phase::Pending), ..status })
}

async fn reconcile(task: Arc<Task>, ctx: Arc<Context>) -> Result<Action, Error> {
    let Some(status) = next_status(&task) else {
        return Ok(Action::await_change());
    };
    let ns = task.namespace().ok_or(Error::NoNamespace)?;
    let api: Api<Task> = Api::namespaced(ctx.client.clone(), &ns);
    let patch = serde_json::json!({
        "apiVersion": "orchbus.io/v1alpha1",
        "kind": "Task",
        "status": status,
    });
    api.patch_status(&task.name_any(), &PatchParams::apply(MANAGER).force(), &Patch::Apply(&patch)).await?;
    tracing::info!(task = %task.name_any(), namespace = %ns, "observed, phase Pending");
    Ok(Action::await_change())
}

fn error_policy(task: Arc<Task>, err: &Error, _: Arc<Context>) -> Action {
    tracing::warn!(task = %task.name_any(), error = %err, "reconcile failed");
    Action::requeue(Duration::from_secs(15))
}

/// Run the Task controller until `shutdown` resolves.
pub async fn run(client: Client, shutdown: impl std::future::Future<Output = ()> + Send + Sync + 'static) {
    let tasks: Api<Task> = Api::all(client.clone());
    Controller::new(tasks, watcher::Config::default())
        .graceful_shutdown_on(shutdown)
        .run(reconcile, error_policy, Arc::new(Context { client }))
        .for_each(|res| async move {
            if let Err(e) = res {
                tracing::debug!(error = %e, "controller event");
            }
        })
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use orchbus_api::TaskSpec;

    fn task() -> Task {
        Task::new("t", TaskSpec { prompt: "p".into(), role: "r".into(), workspace: None, target: Default::default(), priority: None })
    }

    #[test]
    fn a_new_task_becomes_pending() {
        assert_eq!(next_status(&task()).and_then(|s| s.phase), Some(Phase::Pending));
    }

    #[test]
    fn a_task_with_a_phase_is_left_alone() {
        let mut t = task();
        t.status = Some(TaskStatus { phase: Some(Phase::Implementing), iteration: 2, ..Default::default() });
        assert_eq!(next_status(&t), None);
    }

    #[test]
    fn existing_status_fields_are_kept() {
        let mut t = task();
        t.status = Some(TaskStatus { iteration: 3, ..Default::default() });
        assert_eq!(next_status(&t).map(|s| s.iteration), Some(3));
    }
}
