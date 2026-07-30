use serde::{Deserialize, Serialize};

pub const DEFAULT_AGENT_PLAN_BETA: bool = true;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentPlanPolicy {
    pub mode: AgentPlanMode,
    #[serde(default = "default_agent_plan_beta")]
    pub beta: bool,
    #[serde(default)]
    pub nodes: Vec<AgentPlanNodePolicy>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AgentPlanMode {
    None,
    Free,
    Strict,
    Hybrid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentPlanNodePolicy {
    pub id: String,
    pub title: String,
    pub locked: bool,
}

fn default_agent_plan_beta() -> bool {
    DEFAULT_AGENT_PLAN_BETA
}
