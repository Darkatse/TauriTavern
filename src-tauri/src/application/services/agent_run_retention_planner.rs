use std::collections::BTreeSet;

use crate::application::errors::ApplicationError;
use crate::domain::models::agent::AgentRun;
use crate::domain::models::settings::AgentRunRetentionSettings;
use crate::domain::repositories::agent_run_repository::{
    AgentRunRepository, AgentRunStorageEntryStats,
};

pub const MAX_AGENT_RUN_PRUNE_DETAIL_LIMIT: usize = 1_000;

pub struct AgentRunRetentionPlanInput {
    pub retention: AgentRunRetentionSettings,
    pub detail_mode: AgentRunRetentionPlanDetailMode,
    pub active_run_ids: BTreeSet<String>,
}

#[derive(Debug)]
pub struct AgentRunRetentionPlan {
    pub retention: AgentRunRetentionSettings,
    pub terminal_run_count: usize,
    pub non_terminal_run_count: usize,
    pub blocked_run_count: usize,
    pub full_retained_run_count: usize,
    pub core_retained_run_count: usize,
    pub slim_candidate_count: usize,
    pub delete_candidate_count: usize,
    pub total_slim: AgentRunStorageEntryStats,
    pub total_delete: AgentRunStorageEntryStats,
    pub total_candidate: AgentRunStorageEntryStats,
    pub candidate_details_truncated: bool,
    pub candidates: Vec<AgentRunPruneCandidate>,
    pub blocked_details_truncated: bool,
    pub blocked_runs: Vec<AgentRunPruneBlockedRun>,
    detail_mode: AgentRunRetentionPlanDetailMode,
}

#[derive(Debug)]
pub struct AgentRunPruneCandidate {
    pub run: AgentRun,
    pub action: AgentRunPruneAction,
    pub reason: AgentRunPruneReason,
    pub stats: AgentRunStorageEntryStats,
}

#[derive(Debug)]
pub struct AgentRunPruneBlockedRun {
    pub run: AgentRun,
    pub action: AgentRunPruneAction,
    pub reason: AgentRunPruneReason,
    pub block_reason: AgentRunPruneBlockReason,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentRunPruneAction {
    SlimHeavyArtifacts,
    DeleteRun,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentRunPruneReason {
    OutsideFullRetentionWindow,
    OutsideHistoryRetentionWindow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentRunPruneBlockReason {
    ActiveRun,
    MissingTerminalEvent,
    InvalidJournal,
    InvalidStorage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentRunRetentionPlanDetailMode {
    Preview { detail_limit: usize },
    Execution,
}

pub struct AgentRunRetentionPlanner<'a> {
    run_repository: &'a dyn AgentRunRepository,
}

impl<'a> AgentRunRetentionPlanner<'a> {
    pub fn new(run_repository: &'a dyn AgentRunRepository) -> Self {
        Self { run_repository }
    }

    pub async fn plan(
        &self,
        input: AgentRunRetentionPlanInput,
    ) -> Result<AgentRunRetentionPlan, ApplicationError> {
        input
            .retention
            .validate()
            .map_err(|error| ApplicationError::ValidationError(error.message()))?;

        let keep_recent_terminal_runs = input.retention.keep_recent_terminal_runs as usize;
        let keep_full_recent_runs = input.retention.keep_full_recent_runs as usize;
        let runs = self.run_repository.list_all_runs().await?;

        let mut terminal_runs = Vec::new();
        let mut plan = AgentRunRetentionPlan::new(input.retention, input.detail_mode);
        for run in runs {
            if run.status.is_terminal() {
                terminal_runs.push(run);
            } else {
                plan.non_terminal_run_count += 1;
            }
        }
        plan.terminal_run_count = terminal_runs.len();

        for (rank, run) in terminal_runs.into_iter().enumerate() {
            if rank < keep_full_recent_runs {
                plan.full_retained_run_count += 1;
                continue;
            }

            if rank < keep_recent_terminal_runs {
                plan.core_retained_run_count += 1;
                self.plan_slim_candidate(&mut plan, &input.active_run_ids, run)
                    .await?;
                continue;
            }

            self.plan_delete_candidate(&mut plan, &input.active_run_ids, run)
                .await?;
        }

        plan.total_candidate = AgentRunStorageEntryStats {
            file_count: checked_usize_sum(
                plan.total_slim.file_count,
                plan.total_delete.file_count,
            )?,
            byte_count: checked_u64_sum(plan.total_slim.byte_count, plan.total_delete.byte_count)?,
        };

        Ok(plan)
    }

    async fn plan_slim_candidate(
        &self,
        plan: &mut AgentRunRetentionPlan,
        active_run_ids: &BTreeSet<String>,
        run: AgentRun,
    ) -> Result<(), ApplicationError> {
        let action = AgentRunPruneAction::SlimHeavyArtifacts;
        let reason = AgentRunPruneReason::OutsideFullRetentionWindow;
        if self.blocked_by_active_run(plan, active_run_ids, &run, action, reason) {
            return Ok(());
        }
        if self
            .blocked_by_missing_terminal_event(plan, &run, action, reason)
            .await
        {
            return Ok(());
        }

        let storage = match self.run_repository.inspect_run_storage(&run).await {
            Ok(storage) => storage,
            Err(error) => {
                plan.push_blocked(AgentRunPruneBlockedRun {
                    run,
                    action,
                    reason,
                    block_reason: AgentRunPruneBlockReason::InvalidStorage,
                    message: Some(error.to_string()),
                });
                return Ok(());
            }
        };
        if storage.heavy_artifacts.file_count == 0 {
            return Ok(());
        }

        plan.push_candidate(AgentRunPruneCandidate {
            run,
            action,
            reason,
            stats: storage.heavy_artifacts,
        })
    }

    async fn plan_delete_candidate(
        &self,
        plan: &mut AgentRunRetentionPlan,
        active_run_ids: &BTreeSet<String>,
        run: AgentRun,
    ) -> Result<(), ApplicationError> {
        let action = AgentRunPruneAction::DeleteRun;
        let reason = AgentRunPruneReason::OutsideHistoryRetentionWindow;
        if self.blocked_by_active_run(plan, active_run_ids, &run, action, reason) {
            return Ok(());
        }
        if self
            .blocked_by_missing_terminal_event(plan, &run, action, reason)
            .await
        {
            return Ok(());
        }

        let storage = match self.run_repository.inspect_run_storage(&run).await {
            Ok(storage) => storage,
            Err(error) => {
                plan.push_blocked(AgentRunPruneBlockedRun {
                    run,
                    action,
                    reason,
                    block_reason: AgentRunPruneBlockReason::InvalidStorage,
                    message: Some(error.to_string()),
                });
                return Ok(());
            }
        };

        plan.push_candidate(AgentRunPruneCandidate {
            run,
            action,
            reason,
            stats: storage.total,
        })
    }

    fn blocked_by_active_run(
        &self,
        plan: &mut AgentRunRetentionPlan,
        active_run_ids: &BTreeSet<String>,
        run: &AgentRun,
        action: AgentRunPruneAction,
        reason: AgentRunPruneReason,
    ) -> bool {
        if !active_run_ids.contains(&run.id) {
            return false;
        }

        plan.push_blocked(AgentRunPruneBlockedRun {
            run: run.clone(),
            action,
            reason,
            block_reason: AgentRunPruneBlockReason::ActiveRun,
            message: None,
        });
        true
    }

    async fn blocked_by_missing_terminal_event(
        &self,
        plan: &mut AgentRunRetentionPlan,
        run: &AgentRun,
        action: AgentRunPruneAction,
        reason: AgentRunPruneReason,
    ) -> bool {
        match self.run_repository.read_all_events(&run.id).await {
            Ok(events)
                if events
                    .iter()
                    .any(|event| is_terminal_run_event(&event.event_type)) =>
            {
                false
            }
            Ok(_) => {
                plan.push_blocked(AgentRunPruneBlockedRun {
                    run: run.clone(),
                    action,
                    reason,
                    block_reason: AgentRunPruneBlockReason::MissingTerminalEvent,
                    message: None,
                });
                true
            }
            Err(error) => {
                plan.push_blocked(AgentRunPruneBlockedRun {
                    run: run.clone(),
                    action,
                    reason,
                    block_reason: AgentRunPruneBlockReason::InvalidJournal,
                    message: Some(error.to_string()),
                });
                true
            }
        }
    }
}

impl AgentRunRetentionPlan {
    fn new(
        retention: AgentRunRetentionSettings,
        detail_mode: AgentRunRetentionPlanDetailMode,
    ) -> Self {
        Self {
            retention,
            terminal_run_count: 0,
            non_terminal_run_count: 0,
            blocked_run_count: 0,
            full_retained_run_count: 0,
            core_retained_run_count: 0,
            slim_candidate_count: 0,
            delete_candidate_count: 0,
            total_slim: AgentRunStorageEntryStats::default(),
            total_delete: AgentRunStorageEntryStats::default(),
            total_candidate: AgentRunStorageEntryStats::default(),
            candidate_details_truncated: false,
            candidates: Vec::new(),
            blocked_details_truncated: false,
            blocked_runs: Vec::new(),
            detail_mode,
        }
    }

    fn push_candidate(
        &mut self,
        candidate: AgentRunPruneCandidate,
    ) -> Result<(), ApplicationError> {
        match candidate.action {
            AgentRunPruneAction::SlimHeavyArtifacts => {
                self.slim_candidate_count += 1;
                accumulate_stats(&mut self.total_slim, candidate.stats)?;
            }
            AgentRunPruneAction::DeleteRun => {
                self.delete_candidate_count += 1;
                accumulate_stats(&mut self.total_delete, candidate.stats)?;
            }
        }

        match self.detail_mode {
            AgentRunRetentionPlanDetailMode::Preview { detail_limit } => {
                if self.candidates.len() < detail_limit {
                    self.candidates.push(candidate);
                } else {
                    self.candidate_details_truncated = true;
                }
            }
            AgentRunRetentionPlanDetailMode::Execution => {
                self.candidates.push(candidate);
            }
        }
        Ok(())
    }

    fn push_blocked(&mut self, blocked: AgentRunPruneBlockedRun) {
        self.blocked_run_count += 1;
        match self.detail_mode {
            AgentRunRetentionPlanDetailMode::Preview { detail_limit } => {
                if self.blocked_runs.len() < detail_limit {
                    self.blocked_runs.push(blocked);
                } else {
                    self.blocked_details_truncated = true;
                }
            }
            AgentRunRetentionPlanDetailMode::Execution => {}
        }
    }
}

pub(crate) fn is_terminal_run_event(event_type: &str) -> bool {
    matches!(
        event_type,
        "run_completed" | "run_partial_success" | "run_cancelled" | "run_failed"
    )
}

fn accumulate_stats(
    target: &mut AgentRunStorageEntryStats,
    source: AgentRunStorageEntryStats,
) -> Result<(), ApplicationError> {
    target.file_count = checked_usize_sum(target.file_count, source.file_count)?;
    target.byte_count = checked_u64_sum(target.byte_count, source.byte_count)?;
    Ok(())
}

fn checked_usize_sum(left: usize, right: usize) -> Result<usize, ApplicationError> {
    left.checked_add(right).ok_or_else(|| {
        ApplicationError::InternalError("agent.run_prune_file_count_overflow".to_string())
    })
}

fn checked_u64_sum(left: u64, right: u64) -> Result<u64, ApplicationError> {
    left.checked_add(right).ok_or_else(|| {
        ApplicationError::InternalError("agent.run_prune_byte_count_overflow".to_string())
    })
}
