use serde_json::json;

use super::model_turn::{
    append_tool_turn_to_request, assistant_message_for_next_turn, extract_agent_tool_calls,
    extract_response_text,
};
use super::prompt_snapshot::request_summary;
use super::{AgentCancelReceiver, AgentRuntimeService, MAX_AGENT_TOOL_ROUNDS};
use crate::application::dto::chat_completion_dto::ChatCompletionGenerateRequestDto;
use crate::application::errors::ApplicationError;
use crate::application::services::agent_tools::AgentToolEffect;
use crate::domain::models::agent::{AgentRunEventLevel, AgentRunStatus, WorkspacePath};

impl AgentRuntimeService {
    pub(super) async fn run_tool_loop(
        &self,
        run_id: &str,
        mut request: ChatCompletionGenerateRequestDto,
        cancel: &mut AgentCancelReceiver,
    ) -> Result<Option<WorkspacePath>, ApplicationError> {
        for round in 1..=MAX_AGENT_TOOL_ROUNDS {
            self.transition_status(run_id, AgentRunStatus::CallingModel)
                .await?;
            self.event(
                run_id,
                AgentRunEventLevel::Info,
                "model_request_created",
                json!({
                    "round": round,
                    "request": request_summary(&request.payload),
                }),
            )
            .await?;

            let response = self
                .model_gateway
                .generate_with_cancel(request.clone(), cancel.clone())
                .await?;
            self.ensure_not_cancelled(cancel)?;

            let tool_calls = extract_agent_tool_calls(&response, &self.tool_registry)?;
            self.event(
                run_id,
                AgentRunEventLevel::Info,
                "model_completed",
                json!({
                    "round": round,
                    "toolCallCount": tool_calls.len(),
                    "textBytes": extract_response_text(&response).unwrap_or_default().as_bytes().len(),
                }),
            )
            .await?;

            if tool_calls.is_empty() {
                return Err(ApplicationError::ValidationError(
                    "model.tool_call_required_phase2a: model must call workspace_write_file and workspace_finish in Agent Phase 2A".to_string(),
                ));
            }

            let assistant_message = assistant_message_for_next_turn(&response)?;
            let mut tool_results = Vec::with_capacity(tool_calls.len());
            let mut final_path = None;

            for call in tool_calls {
                if final_path.is_some() {
                    return Err(ApplicationError::ValidationError(
                        "agent.tool_after_finish: model requested additional tools after workspace.finish".to_string(),
                    ));
                }

                let outcome = self.dispatch_tool_call(run_id, &call).await?;
                match &outcome.effect {
                    AgentToolEffect::WorkspaceFileWritten { file } => {
                        self.checkpoint_workspace_file(
                            run_id,
                            "tool_workspace_write",
                            file.path.clone(),
                        )
                        .await?;
                    }
                    AgentToolEffect::Finish { final_path: path } => {
                        final_path = Some(path.clone());
                    }
                    AgentToolEffect::None => {}
                }

                tool_results.push(outcome.result);
                self.ensure_not_cancelled(cancel)?;
            }

            if let Some(final_path) = final_path {
                self.event(
                    run_id,
                    AgentRunEventLevel::Info,
                    "agent_loop_finished",
                    json!({ "finalPath": final_path.as_str(), "round": round }),
                )
                .await?;
                return Ok(Some(final_path));
            }

            append_tool_turn_to_request(&mut request, assistant_message, &tool_results)?;
            self.ensure_not_cancelled(cancel)?;
        }

        Ok(None)
    }
}
