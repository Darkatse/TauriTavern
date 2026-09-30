//! How a stage treats a model turn that has text but no tool calls.

use serde::Serialize;

use super::commit_ledger::RunCommitLedger;
use super::loop_runner::turn_can_finish_run;
use tt_domain::models::agent::{AgentInvocationExitPolicy, AgentRunPresentation, AgentRunTarget};
use tt_domain::models::tool::ToolTurnContract;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum FinishPolicy {
    /// Remind the model to continue through Agent tools.
    Correct,
    /// End the run once every condition holds, and remind the model otherwise. The text
    /// stays in the transcript and is not published.
    Conditional(&'static [FinishCondition]),
}

/// A fact the run must already have before a text-only turn may end it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum FinishCondition {
    /// The run has published to the chat at least once, explicitly or automatically.
    Committed,
    // Planned: `PlanComplete`, once runs keep a plan whose items can all be done.
}

impl FinishPolicy {
    /// The default for each kind of stage; the one place that decides how text-only turns end.
    pub(super) fn for_stage(
        exit_policy: AgentInvocationExitPolicy,
        target: &AgentRunTarget,
        turn: &ToolTurnContract,
    ) -> Self {
        match (exit_policy, target) {
            (AgentInvocationExitPolicy::RunFinishAllowed, AgentRunTarget::Chat(chat))
                if turn_can_finish_run(turn) =>
            {
                match chat.presentation {
                    AgentRunPresentation::Foreground => {
                        Self::Conditional(&[FinishCondition::Committed])
                    }
                    AgentRunPresentation::Background => Self::Conditional(&[]),
                }
            }
            // Return-mode children end with task.return and handoff-only stages with the
            // handoff; session replies end their turn before any policy applies.
            _ => Self::Correct,
        }
    }

    pub(super) fn ends_run(self, commits: &RunCommitLedger) -> bool {
        match self {
            Self::Correct => false,
            Self::Conditional(conditions) => {
                conditions.iter().all(|condition| condition.holds(commits))
            }
        }
    }
}

impl FinishCondition {
    fn holds(self, commits: &RunCommitLedger) -> bool {
        match self {
            Self::Committed => !commits.is_empty(),
        }
    }
}
