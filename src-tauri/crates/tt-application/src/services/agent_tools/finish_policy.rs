//! How a stage ends the run. The runtime, the system prompt, continuation hints, result
//! text and Chat readiness all read this one table; opening it to Profiles or presets
//! (for example a plan condition) changes `for_stage` and `FinishCondition` here.

use serde::Serialize;

use tt_domain::models::agent::{AgentInvocationExitPolicy, AgentRunPresentation};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FinishPolicy {
    /// The stage never ends the run itself: it returns its task, hands off, or replies.
    /// A text-only turn is answered with a reminder.
    Correct,
    /// The stage can end the run: by a final commit, or by a text-only turn once every
    /// condition holds (the text stays in the transcript and is not published).
    Conditional(&'static [FinishCondition]),
}

/// A fact the run must already have before a text-only turn may end it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FinishCondition {
    /// The run has published to the chat, explicitly or automatically, and the chat message
    /// still shows that confirmed publication, with no streamed preview or rejected commit
    /// since. Otherwise a text-only turn is answered with a reminder to commit.
    Committed,
    // Planned: `PlanComplete`, once runs keep a plan whose items can all be done.
}

/// What a text-only turn does in a stage, for the text that tells the model how to end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextTurn {
    /// A text-only turn ends the run.
    EndsRun,
    /// A text-only turn ends the run once the chat message shows a confirmed publication
    /// (see [`FinishCondition::Committed`]). Model-facing text directs these stages to finish
    /// with a final commit; the text-turn ending is the fallback.
    EndsRunOnceCommitted,
    /// The stage never ends the run itself (see [`FinishPolicy::Correct`]).
    Continues,
}

impl FinishPolicy {
    /// The default for each kind of stage. `presentation` is the Run's, and
    /// `can_finish_run` comes from `stage_can_finish_run` over the stage's tools.
    pub(crate) fn for_stage(
        exit_policy: AgentInvocationExitPolicy,
        presentation: AgentRunPresentation,
        can_finish_run: bool,
    ) -> Self {
        match exit_policy {
            AgentInvocationExitPolicy::RunFinishAllowed if can_finish_run => match presentation {
                AgentRunPresentation::Foreground => {
                    Self::Conditional(&[FinishCondition::Committed])
                }
                AgentRunPresentation::Background => Self::Conditional(&[]),
            },
            // Return-mode children end with task.return and handoff-only stages with the
            // handoff; session replies end their turn before any policy applies.
            _ => Self::Correct,
        }
    }

    /// Whether a text-only turn ends the run, given whether [`FinishCondition::Committed`]
    /// holds.
    pub(crate) fn ends_run(self, committed: bool) -> bool {
        match self {
            Self::Correct => false,
            Self::Conditional(conditions) => conditions
                .iter()
                .all(|condition| condition.holds(committed)),
        }
    }

    pub(crate) fn text_turn(self) -> TextTurn {
        match self {
            Self::Correct => TextTurn::Continues,
            Self::Conditional([]) => TextTurn::EndsRun,
            Self::Conditional(_) => TextTurn::EndsRunOnceCommitted,
        }
    }

    /// Whether the stage must be able to commit: its text-only turns end the run only once
    /// the chat message shows a confirmed publication, which this stage may have to make.
    pub(crate) fn requires_commit(self) -> bool {
        matches!(self, Self::Conditional(conditions) if conditions.contains(&FinishCondition::Committed))
    }
}

impl FinishCondition {
    fn holds(self, committed: bool) -> bool {
        match self {
            Self::Committed => committed,
        }
    }
}
