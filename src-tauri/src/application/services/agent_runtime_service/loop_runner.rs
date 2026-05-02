use serde_json::json;

use super::model_turn::{
    append_tool_turn_to_request, assistant_message_for_next_turn, extract_response_text,
};
use super::prompt_snapshot::request_summary;
use super::{AgentCancelReceiver, AgentRuntimeService, MAX_AGENT_TOOL_ROUNDS};
use crate::application::errors::ApplicationError;
use crate::application::services::agent_tools::{AgentToolEffect, AgentToolSession};
use crate::domain::models::agent::{
    AgentModelRequest, AgentRunEventLevel, AgentRunStatus, AgentToolResult, WorkspacePath,
};

impl AgentRuntimeService {
    pub(super) async fn run_tool_loop(
        &self,
        run_id: &str,
        mut request: AgentModelRequest,
        cancel: &mut AgentCancelReceiver,
    ) -> Result<Option<WorkspacePath>, ApplicationError> {
        let mut tool_session = AgentToolSession::default();
        for round in 1..=MAX_AGENT_TOOL_ROUNDS {
            self.transition_status(run_id, AgentRunStatus::CallingModel)
                .await?;
            self.event(
                run_id,
                AgentRunEventLevel::Info,
                "model_request_created",
                json!({
                    "round": round,
                    "request": request_summary(&request),
                }),
            )
            .await?;

            let exchange = self
                .model_gateway
                .generate_with_cancel(request.clone(), cancel.clone())
                .await?;
            self.ensure_not_cancelled(cancel)?;
            let response = exchange.response;
            let model_response_path = self.store_model_response(run_id, round, &response).await?;
            request.provider_state = exchange.provider_state;
            self.event(
                run_id,
                AgentRunEventLevel::Debug,
                "provider_state_updated",
                json!({
                    "round": round,
                    "providerState": provider_state_summary(&request.provider_state),
                }),
            )
            .await?;

            let tool_calls = response.tool_calls.clone();
            self.event(
                run_id,
                AgentRunEventLevel::Info,
                "model_completed",
                json!({
                    "round": round,
                    "modelResponsePath": model_response_path.as_str(),
                    "toolCallCount": tool_calls.len(),
                    "textBytes": extract_response_text(&response).as_bytes().len(),
                }),
            )
            .await?;

            if tool_calls.is_empty() {
                return Err(ApplicationError::ValidationError(
                    "model.tool_call_required: model must use Agent tools and finish through workspace_finish"
                        .to_string(),
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

                let outcome = self
                    .dispatch_tool_call(run_id, &call, &mut tool_session)
                    .await?;
                match &outcome.effect {
                    AgentToolEffect::WorkspaceFileWritten { file } => {
                        self.checkpoint_workspace_file(
                            run_id,
                            "tool_workspace_write",
                            "workspace_file_written",
                            json!({
                                "path": file.path.as_str(),
                                "bytes": file.bytes,
                                "sha256": file.sha256.as_str(),
                            }),
                            file.path.clone(),
                        )
                        .await?;
                    }
                    AgentToolEffect::WorkspaceFilePatched {
                        file,
                        replacements,
                        old_sha256,
                    } => {
                        self.transition_status(run_id, AgentRunStatus::ApplyingWorkspacePatch)
                            .await?;
                        self.checkpoint_workspace_file(
                            run_id,
                            "tool_workspace_patch",
                            "workspace_patch_applied",
                            json!({
                                "path": file.path.as_str(),
                                "bytes": file.bytes,
                                "oldSha256": old_sha256,
                                "sha256": file.sha256.as_str(),
                                "replacements": replacements,
                            }),
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

            let tool_results = self
                .hydrate_recent_tool_results_for_model(run_id, round, &tool_results)
                .await?;
            append_tool_turn_to_request(&mut request, assistant_message, &tool_results)?;
            self.ensure_not_cancelled(cancel)?;
        }

        Ok(None)
    }

    async fn hydrate_recent_tool_results_for_model(
        &self,
        run_id: &str,
        round: usize,
        tool_results: &[AgentToolResult],
    ) -> Result<Vec<AgentToolResult>, ApplicationError> {
        if round > 5 {
            return Ok(tool_results.to_vec());
        }

        let mut hydrated = Vec::with_capacity(tool_results.len());
        for result in tool_results {
            let mut result = result.clone();
            if result.is_error
                || !(result.name == "workspace.write_file"
                    || result.name == "workspace.apply_patch")
            {
                hydrated.push(result);
                continue;
            }

            let Some(path) = result
                .structured
                .get("path")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            else {
                hydrated.push(result);
                continue;
            };
            let workspace_path = WorkspacePath::parse(path)?;
            let file = self
                .workspace_repository
                .read_text(run_id, &workspace_path)
                .await?;
            result.content = format!(
                "{}\n\nFull content of {}:\n{}",
                result.content,
                file.path.as_str(),
                file.text
            );
            self.event(
                run_id,
                AgentRunEventLevel::Debug,
                "context_tool_result_hydrated",
                json!({
                    "round": round,
                    "callId": result.call_id.as_str(),
                    "path": file.path.as_str(),
                    "bytes": file.bytes,
                }),
            )
            .await?;
            hydrated.push(result);
        }

        Ok(hydrated)
    }
}

fn provider_state_summary(provider_state: &serde_json::Value) -> serde_json::Value {
    json!({
        "chatCompletionSource": provider_state.get("chatCompletionSource"),
        "providerFormat": provider_state.get("providerFormat"),
        "transport": provider_state.get("transport"),
        "messageCursor": provider_state.get("messageCursor"),
        "lastResponseId": provider_state.get("lastResponseId"),
        "previousResponseId": provider_state.get("previousResponseId"),
        "nativeContinuation": provider_state.get("nativeContinuation"),
    })
}
