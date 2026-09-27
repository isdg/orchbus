use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A job description: how an agent in this role is launched and what authority it acts with.
#[derive(CustomResource, Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[kube(
    group = "orchbus.io",
    version = "v1alpha1",
    kind = "AgentRole",
    namespaced,
    shortname = "ar",
    category = "orchbus",
    printcolumn(name = "Runtime", type_ = "string", json_path = ".spec.runtime"),
    printcolumn(name = "Model", type_ = "string", json_path = ".spec.model"),
    printcolumn(name = "Authority", type_ = "string", json_path = ".spec.authority"),
    printcolumn(name = "Age", type_ = "date", json_path = ".metadata.creationTimestamp")
)]
#[serde(rename_all = "camelCase")]
pub struct AgentRoleSpec {
    /// Agent CLI the node launches, e.g. `claude`.
    #[serde(default = "default_runtime")]
    pub runtime: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Appended to the agent's system prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Tools::is_empty")]
    pub tools: Tools,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<String>,
    #[serde(default)]
    pub authority: Authority,
}

fn default_runtime() -> String {
    "claude".into()
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, JsonSchema)]
pub struct Tools {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allow: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deny: Vec<String>,
}

impl Tools {
    fn is_empty(&self) -> bool {
        self.allow.is_empty() && self.deny.is_empty()
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Authority {
    #[default]
    Worker,
    Manager,
}
