//! The `orchbus.io/v1alpha1` API: the custom resources an agent company is made of.
//! `deploy/crds/orchbus.yaml` is generated from these types: `UPDATE_CRDS=1 cargo test -p orchbus-api`.

pub mod approval;
pub mod role;
pub mod task;
pub mod workspace;

pub use approval::{Approval, ApprovalSpec, ApprovalStatus};
pub use role::{AgentRole, AgentRoleSpec};
pub use task::{Task, TaskSpec, TaskStatus};
pub use workspace::{Workspace, WorkspaceSpec};

use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use kube::CustomResourceExt;

pub const GROUP: &str = "orchbus.io";
pub const VERSION: &str = "v1alpha1";

/// Every CRD of the API, in a stable order.
pub fn crds() -> Vec<CustomResourceDefinition> {
    vec![AgentRole::crd(), Approval::crd(), Task::crd(), Workspace::crd()]
}
