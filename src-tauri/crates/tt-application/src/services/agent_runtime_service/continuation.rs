use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::PreparedInvocation;
use super::commit_ledger::RunCommitLedger;
use super::guidance::AgentGuidanceItem;
use super::loop_runner::AgentLoopExit;
use crate::services::agent_tools::AgentToolSession;
use crate::services::tool_request_gate::ToolRequestGate;
use tt_domain::models::agent::WorkspacePersistentChangeSet;
use tt_domain::models::tool::ToolInvocation;
use tt_ports::repositories::workspace_repository::WorkspaceFile;

/// The live execution state is also the checkpoint payload. No journal replay is needed.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RunExecutionState {
    pub foreground: Option<InvocationFrame>,
    pub children: Vec<InvocationFrame>,
    pub commits: RunCommitLedger,
    pub guidance: Vec<AgentGuidanceItem>,
    pub published_state: Option<WorkspacePersistentChangeSet>,
    /// The preceding revision's version can be reused when persistent files did not change.
    #[serde(default)]
    pub previous_published_state_id: Option<String>,
    pub blocked_reason: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct InvocationFrame {
    pub prepared: PreparedInvocation,
    pub progress: InvocationProgress,
}

impl InvocationFrame {
    pub fn new(prepared: PreparedInvocation) -> Self {
        let mut session = AgentToolSession::new(prepared.effective_skills.clone());
        session.frozen_macros = prepared.frozen_macros.clone();
        let max_rounds = prepared.profile.tools.max_rounds;
        Self {
            prepared,
            progress: InvocationProgress {
                round: 1,
                max_rounds,
                step: InvocationStep::Model,
                session,
                gate: ToolRequestGate::default(),
                seen_child_results: HashSet::new(),
                drift_attempts: 0,
                blocked_reason: None,
            },
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct InvocationProgress {
    pub round: usize,
    pub max_rounds: usize,
    pub step: InvocationStep,
    pub session: AgentToolSession,
    pub gate: ToolRequestGate,
    pub seen_child_results: HashSet<String>,
    pub drift_attempts: usize,
    /// Set across operations whose failure can leave an unconfirmed effect or missing record.
    pub blocked_reason: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum InvocationStep {
    Model,
    Tools(PendingToolTurn),
    Exited(AgentLoopExit),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PendingToolTurn {
    pub calls: Vec<ToolInvocation>,
    pub next_call: usize,
    pub exit: Option<AgentLoopExit>,
    pub auto_commit: Option<(String, WorkspaceFile)>,
}
