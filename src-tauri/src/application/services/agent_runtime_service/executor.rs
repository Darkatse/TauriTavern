use std::sync::Arc;

use serde_json::{Value, json};

use super::artifacts::build_agent_manifest;
use super::commit_ledger::RunCommitLedger;
use super::error_payload::{run_failure_payload, run_partial_success_payload};
use super::prompt_snapshot::{prepare_agent_tool_request, request_summary};
use super::{AgentCancelReceiver, AgentRuntimeService};
use crate::application::dto::chat_completion_dto::ChatCompletionGenerateRequestDto;
use crate::application::errors::ApplicationError;
use crate::domain::models::agent::profile::ResolvedAgentProfile;
use crate::domain::models::agent::{AgentRunEventLevel, AgentRunStatus, WorkspacePath};
use crate::domain::models::skill::SkillIndexEntry;

impl AgentRuntimeService {
    pub(super) async fn execute_agent_loop_run(
        self: Arc<Self>,
        run_id: String,
        prompt_snapshot: Value,
        request: ChatCompletionGenerateRequestDto,
        resolved_profile: ResolvedAgentProfile,
        effective_skills: Vec<SkillIndexEntry>,
        mut cancel: AgentCancelReceiver,
    ) {
        let mut commit_ledger = RunCommitLedger::default();
        let result = self
            .execute_agent_loop_run_body(
                &run_id,
                prompt_snapshot,
                request,
                resolved_profile,
                effective_skills,
                &mut commit_ledger,
                &mut cancel,
            )
            .await;

        self.finalize_agent_loop_run_result(&run_id, &commit_ledger, &result)
            .await;
        self.close_model_session_after_run(run_id);
    }

    async fn finalize_agent_loop_run_result(
        &self,
        run_id: &str,
        commit_ledger: &RunCommitLedger,
        result: &Result<(), ApplicationError>,
    ) {
        match result {
            Ok(()) => {}
            Err(ApplicationError::Cancelled(message)) => {
                self.clear_pending_host_requests_for_run(run_id).await;
                let _ = self
                    .transition_status(run_id, AgentRunStatus::Cancelled)
                    .await;
                let _ = self
                    .event(
                        run_id,
                        AgentRunEventLevel::Info,
                        "run_cancelled",
                        json!({ "message": message }),
                    )
                    .await;
                self.active_runs.write().await.remove(run_id);
            }
            Err(error) => {
                self.clear_pending_host_requests_for_run(run_id).await;
                if commit_ledger.is_empty() {
                    let _ = self.transition_status(run_id, AgentRunStatus::Failed).await;
                    let _ = self
                        .event(
                            run_id,
                            AgentRunEventLevel::Error,
                            "run_failed",
                            run_failure_payload(error),
                        )
                        .await;
                } else {
                    let _ = self
                        .transition_status(run_id, AgentRunStatus::PartialSuccess)
                        .await;
                    let _ = self
                        .event(
                            run_id,
                            AgentRunEventLevel::Warn,
                            "run_partial_success",
                            run_partial_success_payload(error, commit_ledger),
                        )
                        .await;
                }
                self.active_runs.write().await.remove(run_id);
            }
        }
    }

    #[cfg(test)]
    pub(super) async fn execute_agent_loop_run_inner(
        &self,
        run_id: &str,
        prompt_snapshot: Value,
        request: ChatCompletionGenerateRequestDto,
        resolved_profile: ResolvedAgentProfile,
        cancel: &mut AgentCancelReceiver,
    ) -> Result<(), ApplicationError> {
        let mut commit_ledger = RunCommitLedger::default();
        let effective_skills = self
            .skill_service
            .resolve_effective_skills(
                &[
                    crate::domain::models::skill::SkillScope::Global,
                    crate::domain::models::skill::SkillScope::Profile {
                        profile_id: resolved_profile.id.as_str().to_string(),
                    },
                ],
                &resolved_profile.skills,
            )
            .await?;
        let result = self
            .execute_agent_loop_run_body(
                run_id,
                prompt_snapshot,
                request,
                resolved_profile,
                effective_skills,
                &mut commit_ledger,
                cancel,
            )
            .await;
        self.finalize_agent_loop_run_result(run_id, &commit_ledger, &result)
            .await;
        self.close_model_session_after_run(run_id.to_string());
        result
    }

    fn close_model_session_after_run(&self, run_id: String) {
        let model_gateway = Arc::clone(&self.model_gateway);
        tauri::async_runtime::spawn(async move {
            model_gateway.close_session(&run_id).await;
        });
    }

    async fn execute_agent_loop_run_body(
        &self,
        run_id: &str,
        prompt_snapshot: Value,
        request: ChatCompletionGenerateRequestDto,
        resolved_profile: ResolvedAgentProfile,
        effective_skills: Vec<SkillIndexEntry>,
        commit_ledger: &mut RunCommitLedger,
        cancel: &mut AgentCancelReceiver,
    ) -> Result<(), ApplicationError> {
        let run = self
            .transition_status(run_id, AgentRunStatus::InitializingWorkspace)
            .await?;
        let manifest = build_agent_manifest(&run, &resolved_profile);
        self.workspace_repository
            .initialize_run(&run, &manifest, &prompt_snapshot, &resolved_profile)
            .await?;
        self.event(
            run_id,
            AgentRunEventLevel::Info,
            "workspace_initialized",
            json!({
                "workspaceId": run.workspace_id,
                "stableChatId": run.stable_chat_id,
            }),
        )
        .await?;
        let persistent_roots = manifest
            .roots
            .iter()
            .filter(|root| {
                root.commit == crate::domain::models::agent::WorkspaceRootCommit::OnRunCompleted
            })
            .map(|root| root.path.as_str())
            .collect::<Vec<_>>();
        if !persistent_roots.is_empty() {
            self.event(
                run_id,
                AgentRunEventLevel::Info,
                "persistent_projection_initialized",
                json!({
                    "roots": persistent_roots,
                }),
            )
            .await?;
        }
        self.ensure_not_cancelled(cancel)?;
        let resolved_skills = serde_json::to_string_pretty(&effective_skills).map_err(|error| {
            ApplicationError::ValidationError(format!(
                "agent.resolved_skills_serialize_failed: {error}"
            ))
        })?;
        self.workspace_repository
            .write_text(
                run_id,
                &WorkspacePath::parse("input/resolved_skills.json")?,
                &resolved_skills,
            )
            .await?;

        let visible_tools = self.tool_registry.visible_specs(&resolved_profile)?;
        let request = prepare_agent_tool_request(request, &visible_tools, run_id)?;
        self.transition_status(run_id, AgentRunStatus::AssemblingContext)
            .await?;
        self.event(
            run_id,
            AgentRunEventLevel::Info,
            "context_assembled",
            json!({
                "request": request_summary(&request),
                "tools": &visible_tools,
                "maxRounds": resolved_profile.tools.max_rounds,
                "contextPolicy": &resolved_profile.context,
                "modelRetry": {
                    "maxRetries": resolved_profile.run.model_retry.max_retries,
                    "intervalMs": resolved_profile.run.model_retry.interval_ms,
                },
            }),
        )
        .await?;
        self.ensure_not_cancelled(cancel)?;

        self.run_tool_loop(
            run_id,
            request,
            &resolved_profile,
            &effective_skills,
            commit_ledger,
            cancel,
        )
        .await?
        .ok_or_else(|| {
            ApplicationError::ValidationError(format!(
                "agent.max_tool_rounds_exceeded: workspace.finish was not called within {} rounds",
                resolved_profile.tools.max_rounds
            ))
        })?;
        self.ensure_not_cancelled(cancel)?;

        self.finish_run(run_id, commit_ledger, cancel).await?;
        Ok(())
    }
}
