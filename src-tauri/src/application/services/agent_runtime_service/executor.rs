use std::sync::Arc;

use serde_json::{Value, json};

use super::artifacts::build_agent_manifest;
use super::prompt_snapshot::{prepare_agent_tool_request, request_summary};
use super::{AgentCancelReceiver, AgentRuntimeService, MAX_AGENT_TOOL_ROUNDS};
use crate::application::dto::chat_completion_dto::ChatCompletionGenerateRequestDto;
use crate::application::errors::ApplicationError;
use crate::domain::models::agent::{AgentRunEventLevel, AgentRunStatus};

impl AgentRuntimeService {
    pub(super) async fn execute_agent_loop_run(
        self: Arc<Self>,
        run_id: String,
        prompt_snapshot: Value,
        request: ChatCompletionGenerateRequestDto,
        mut cancel: AgentCancelReceiver,
    ) {
        let result = self
            .execute_agent_loop_run_inner(&run_id, prompt_snapshot, request, &mut cancel)
            .await;

        match result {
            Ok(()) => {}
            Err(ApplicationError::Cancelled(message)) => {
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
    }

    pub(super) async fn execute_agent_loop_run_inner(
        &self,
        run_id: &str,
        prompt_snapshot: Value,
        request: ChatCompletionGenerateRequestDto,
        cancel: &mut AgentCancelReceiver,
    ) -> Result<(), ApplicationError> {
        let run = self
            .transition_status(run_id, AgentRunStatus::InitializingWorkspace)
            .await?;
        let manifest = build_agent_manifest(&run);
        self.workspace_repository
            .initialize_run(&run, &manifest, &prompt_snapshot)
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

        let request = prepare_agent_tool_request(request, &self.tool_registry)?;
        self.transition_status(run_id, AgentRunStatus::AssemblingContext)
            .await?;
        self.event(
            run_id,
            AgentRunEventLevel::Info,
            "context_assembled",
            json!({
                "request": request_summary(&request.payload),
                "tools": self.tool_registry.specs(),
                "maxRounds": MAX_AGENT_TOOL_ROUNDS,
            }),
        )
        .await?;
        self.ensure_not_cancelled(cancel)?;

        let final_path = self
            .run_tool_loop(run_id, request, cancel)
            .await?
            .ok_or_else(|| {
                ApplicationError::ValidationError(format!(
                    "agent.max_tool_rounds_exceeded: workspace.finish was not called within {MAX_AGENT_TOOL_ROUNDS} rounds"
                ))
            })?;

        self.transition_status(run_id, AgentRunStatus::AssemblingArtifacts)
            .await?;
        let artifact = self
            .validate_final_artifact(run_id, &manifest, &final_path)
            .await?;
        self.event(
            run_id,
            AgentRunEventLevel::Info,
            "artifact_assembled",
            json!({
                "id": "main",
                "path": artifact.path.as_str(),
                "bytes": artifact.bytes,
                "sha256": artifact.sha256,
            }),
        )
        .await?;
        self.ensure_not_cancelled(cancel)?;

        self.transition_status(run_id, AgentRunStatus::AwaitingCommit)
            .await?;
        Ok(())
    }
}
