use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A codebase a department works in, with the worktrees agents may be placed in.
#[derive(CustomResource, Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[kube(
    group = "orchbus.io",
    version = "v1alpha1",
    kind = "Workspace",
    namespaced,
    shortname = "ws",
    category = "orchbus",
    printcolumn(name = "Repo", type_ = "string", json_path = ".spec.repo"),
    printcolumn(name = "Verify", type_ = "string", json_path = ".spec.verify"),
    printcolumn(name = "Age", type_ = "date", json_path = ".metadata.creationTimestamp")
)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSpec {
    /// Absolute path of the main checkout on the node.
    pub repo: String,
    /// Pre-provisioned worktrees to bind tasks to before creating new ones.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pool: Vec<String>,
    /// Shell commands run in a fresh worktree before the agent starts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bootstrap: Vec<String>,
    /// Command whose success is the Task's `Verified` condition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verify: Option<String>,
}
