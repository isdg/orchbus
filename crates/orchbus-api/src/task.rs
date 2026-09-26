use k8s_openapi::apimachinery::pkg::apis::meta::v1::Condition;
use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A unit of work: an agent in a Role works a prompt in a Workspace until `target` is reached.
#[derive(CustomResource, Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[kube(
    group = "orchbus.io",
    version = "v1alpha1",
    kind = "Task",
    namespaced,
    status = "TaskStatus",
    shortname = "tk",
    category = "orchbus",
    printcolumn(name = "Phase", type_ = "string", json_path = ".status.phase"),
    printcolumn(name = "Role", type_ = "string", json_path = ".spec.role"),
    printcolumn(name = "Workspace", type_ = "string", json_path = ".spec.workspace"),
    printcolumn(name = "USD", type_ = "number", json_path = ".status.cost.usd", priority = "1"),
    printcolumn(name = "Age", type_ = "date", json_path = ".metadata.creationTimestamp")
)]
#[serde(rename_all = "camelCase")]
pub struct TaskSpec {
    pub prompt: String,
    /// Name of the AgentRole the working agent is started with.
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    #[serde(default)]
    pub target: Target,
    /// Higher runs first when the namespace is at its agent quota.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<i32>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Target {
    Planned,
    #[default]
    Ready,
    Landed,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TaskStatus {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<Phase>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<Condition>,
    /// Bumped on every revise; forks and rollbacks refer to iterations.
    #[serde(default)]
    pub iteration: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<Cost>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, JsonSchema)]
pub enum Phase {
    Pending,
    Planned,
    Implementing,
    Verifying,
    Reviewing,
    Ready,
    Landing,
    Landed,
    Suspended,
    Failed,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Cost {
    pub usd: f64,
    #[serde(default)]
    pub input_tokens: i64,
    #[serde(default)]
    pub output_tokens: i64,
}
