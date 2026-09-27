use k8s_openapi::apimachinery::pkg::apis::meta::v1::Condition;
use kube::{CustomResource, KubeSchema};
use serde::{Deserialize, Serialize};

/// A decision an agent is blocked on. Who may decide is enforced by admission policy;
/// the schema rules here hold for everyone.
#[derive(CustomResource, KubeSchema, Serialize, Deserialize, Clone, Debug, PartialEq)]
#[kube(
    group = "orchbus.io",
    version = "v1alpha1",
    kind = "Approval",
    namespaced,
    status = "ApprovalStatus",
    shortname = "ap",
    category = "orchbus",
    printcolumn(name = "Kind", type_ = "string", json_path = ".spec.kind"),
    printcolumn(name = "Subject", type_ = "string", json_path = ".spec.subject.pod"),
    printcolumn(name = "Question", type_ = "string", json_path = ".spec.question"),
    printcolumn(name = "Decision", type_ = "string", json_path = ".spec.decision"),
    printcolumn(name = "Age", type_ = "date", json_path = ".metadata.creationTimestamp")
)]
#[x_kube(
    cel,
    validation = Rule::new("self.kind == oldSelf.kind && self.subject == oldSelf.subject").message("kind and subject are immutable"),
    validation = Rule::new("oldSelf.decision == 'pending' || self.decision == oldSelf.decision").message("a decision is final"),
    validation = Rule::new("self.decision != 'approved' || size(self.options) == 0 || has(self.choice)").message("approving a menu needs a choice")
)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalSpec {
    pub kind: ApprovalKind,
    pub question: String,
    /// Menu options as shown by the agent, in order.
    #[serde(default)]
    pub options: Vec<String>,
    pub subject: Subject,
    #[serde(default)]
    pub decision: Decision,
    /// 1-based menu option to select when approved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = 9))]
    pub choice: Option<i32>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ApprovalKind {
    Permission,
    Plan,
    Land,
    Budget,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Subject {
    pub pod: String,
    /// Service accounts of the subject and its ancestors; none of them may decide.
    #[serde(default)]
    pub chain: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Decision {
    #[default]
    Pending,
    Approved,
    Denied,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalStatus {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<Condition>,
}
