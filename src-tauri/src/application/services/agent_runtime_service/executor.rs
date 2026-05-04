use std::sync::Arc;

use serde_json::{Value, json};

use super::artifacts::build_agent_manifest;
use super::prompt_snapshot::{prepare_agent_tool_request, request_summary};
use super::{AgentCancelReceiver, AgentRuntimeService};
use crate::application::dto::chat_completion_dto::ChatCompletionGenerateRequestDto;
use crate::application::errors::ApplicationError;
use crate::domain::models::agent::profile::ResolvedAgentProfile;
use crate::domain::models::agent::{AgentRunEventLevel, AgentRunStatus};

impl AgentRuntimeService {
    pub(super) async fn execute_agent_loop_run(
        self: Arc<Self>,
        run_id: String,
        prompt_snapshot: Value,
        request: ChatCompletionGenerateRequestDto,
        resolved_profile: ResolvedAgentProfile,
        mut cancel: AgentCancelReceiver,
    ) {
        let result = self
            .execute_agent_loop_run_body(
                &run_id,
                prompt_snapshot,
                request,
                resolved_profile,
                &mut cancel,
            )
            .await;

        match result {
            Ok(()) => {}
            Err(ApplicationError::Cancelled(message)) => {
                self.clear_pending_chat_commits_for_run(&run_id).await;
                let _ = self
                    .transition_status(&run_id, AgentRunStatus::Cancelled)
                    .await;
                let _ = self
                    .event(
                        &run_id,
                        AgentRunEventLevel::Info,
                        "run_cancelled",
                        json!({ "message": message }),
                    )
                    .await;
                self.active_runs.write().await.remove(&run_id);
            }
            Err(error) => {
                self.clear_pending_chat_commits_for_run(&run_id).await;
                let _ = self
                    .transition_status(&run_id, AgentRunStatus::Failed)
                    .await;
                let _ = self
                    .event(
                        &run_id,
                        AgentRunEventLevel::Error,
                        "run_failed",
                        json!({ "message": error.to_string() }),
                    )
                    .await;
                self.active_runs.write().await.remove(&run_id);
            }
        }

        self.close_model_session_after_run(run_id);
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
        let result = self
            .execute_agent_loop_run_body(run_id, prompt_snapshot, request, resolved_profile, cancel)
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

        let visible_tools = self.tool_registry.visible_specs(&resolved_profile)?;
        let request =
            prepare_agent_tool_request(request, &visible_tools, &resolved_profile, run_id)?;
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
                "modelRetry": {
                    "maxRetries": resolved_profile.run.model_retry.max_retries,
                    "intervalMs": resolved_profile.run.model_retry.interval_ms,
                },
            }),
        )
        .await?;
        self.ensure_not_cancelled(cancel)?;

        self
            .run_tool_loop(run_id, request, &resolved_profile, cancel)
            .await?
            .ok_or_else(|| {
                ApplicationError::ValidationError(format!(
                    "agent.max_tool_rounds_exceeded: workspace.finish was not called within {} rounds",
                    resolved_profile.tools.max_rounds
                ))
            })?;
        self.ensure_not_cancelled(cancel)?;

        self.finish_run(run_id).await?;
        Ok(())
    }
}
