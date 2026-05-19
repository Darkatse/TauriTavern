use serde_json::{Value, json};

use super::model_turn::{
    append_tool_turn_to_request, assistant_message_for_next_turn, extract_response_text,
};
use super::model_turn_display::model_turn_event_summary;
use super::prompt_snapshot::request_summary;
use super::{AgentCancelReceiver, AgentRuntimeService};
use crate::application::errors::ApplicationError;
use crate::application::services::agent_tools::{AgentToolEffect, AgentToolSession};
use crate::domain::models::agent::profile::ResolvedAgentProfile;
use crate::domain::models::agent::{
    AgentChatCommitMode, AgentModelContentPart, AgentModelMessage, AgentModelRequest,
    AgentModelRole, AgentRunEventLevel, AgentRunStatus, AgentToolResult, WorkspacePath,
};

/// How many in-loop drift recovery attempts to make per run before
/// surrendering to the existing #55 fail-fast path. One attempt is
/// enough for the common case (model forgets `workspace_finish` after
/// the final commit) without burning excessive tokens on stubborn
/// drifters; raise this only after we have data showing repeat drifts
/// are common AND benign.
const DRIFT_RECOVERY_MAX_ATTEMPTS: usize = 1;

/// Tracks a chat commit that the model produced during this run. When the
/// run later fails because of instruction drift (issue #55), we surface
/// these records on a `run_rollback_targets` event so the host UI can offer
/// the user a clean Retry that discards the drift artifacts.
#[derive(Debug, Clone)]
struct CommittedChatMessage {
    path: String,
    mode: AgentChatCommitMode,
    message_id: Option<String>,
    round: usize,
}

impl AgentRuntimeService {
    pub(super) async fn run_tool_loop(
        &self,
        run_id: &str,
        mut request: AgentModelRequest,
        profile: &ResolvedAgentProfile,
        cancel: &mut AgentCancelReceiver,
    ) -> Result<Option<usize>, ApplicationError> {
        let mut tool_session = AgentToolSession::default();
        let mut commit_count = 0_usize;
        let mut committed_messages: Vec<CommittedChatMessage> = Vec::new();
        // Issue #64: counter for soft drift recovery — see
        // `DRIFT_RECOVERY_MAX_ATTEMPTS` above. Persisted across rounds so a
        // single run gets at most N corrective nudges in total, not N per
        // drift event.
        let mut drift_recovery_attempts: usize = 0;
        for round in 1..=profile.tools.max_rounds {
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
                .generate_model_with_retry(
                    run_id,
                    round,
                    &request,
                    &profile.run.model_retry,
                    cancel,
                )
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
            self.event(run_id, AgentRunEventLevel::Info, "model_completed", {
                let mut payload = model_turn_event_summary(&response);
                let object = payload
                    .as_object_mut()
                    .expect("model turn event summary must be a JSON object");
                object.insert("round".to_string(), json!(round));
                object.insert(
                    "modelResponsePath".to_string(),
                    json!(model_response_path.as_str()),
                );
                object.insert("toolCallCount".to_string(), json!(tool_calls.len()));
                object.insert(
                    "textBytes".to_string(),
                    json!(extract_response_text(&response).as_bytes().len()),
                );
                payload
            })
            .await?;

            if tool_calls.is_empty() {
                // Issue #64: instead of failing the run immediately, give the
                // model one chance to self-correct. The most common drift —
                // model commits, then replies in plain text instead of
                // calling `workspace_finish` — is a one-step contract slip,
                // not a fundamental misunderstanding. We push the drifted
                // assistant turn into history (so the model owns what it
                // just said) and follow it with a synthetic `user` reminder.
                // Multi-turn pattern is API-compatible with both Anthropic
                // and OpenAI chat completions (no role-alternation
                // constraint after a no-tool-use turn).
                let can_recover = drift_recovery_attempts < DRIFT_RECOVERY_MAX_ATTEMPTS
                    && round < profile.tools.max_rounds;
                if can_recover {
                    drift_recovery_attempts += 1;
                    let committed_count = committed_messages.len();
                    let nudge_text = build_drift_recovery_nudge(
                        committed_count,
                        drift_recovery_attempts,
                        DRIFT_RECOVERY_MAX_ATTEMPTS,
                    );
                    request.messages.push(response.message.clone());
                    request.messages.push(AgentModelMessage {
                        role: AgentModelRole::User,
                        parts: vec![AgentModelContentPart::Text { text: nudge_text }],
                        provider_metadata: Value::Null,
                    });
                    self.event(
                        run_id,
                        AgentRunEventLevel::Warn,
                        "drift_recovery_attempted",
                        json!({
                            "attempt": drift_recovery_attempts,
                            "maxAttempts": DRIFT_RECOVERY_MAX_ATTEMPTS,
                            "round": round,
                            "committedCount": committed_count,
                            "reasonCode": "model.tool_call_required",
                        }),
                    )
                    .await?;
                    self.ensure_not_cancelled(cancel)?;
                    continue;
                }
                self.emit_run_rollback_targets(run_id, &committed_messages, "model.tool_call_required", round)
                    .await?;
                return Err(ApplicationError::ValidationError(
                    "model.tool_call_required: model must use Agent tools and finish through workspace_finish"
                        .to_string(),
                ));
            }

            let assistant_message = assistant_message_for_next_turn(&response)?;
            let mut tool_results = Vec::with_capacity(tool_calls.len());
            let mut finished = false;

            for call in tool_calls {
                if finished {
                    self.emit_run_rollback_targets(
                        run_id,
                        &committed_messages,
                        "agent.tool_after_finish",
                        round,
                    )
                    .await?;
                    return Err(ApplicationError::ValidationError(
                        "agent.tool_after_finish: model requested additional tools after workspace.finish".to_string(),
                    ));
                }

                let outcome = self
                    .dispatch_tool_call(
                        run_id,
                        round,
                        &call,
                        &mut tool_session,
                        profile,
                        commit_count,
                        cancel,
                    )
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
                    AgentToolEffect::ChatCommitRequested { .. } => {}
                    AgentToolEffect::ChatCommitted {
                        path,
                        mode,
                        message_id,
                    } => {
                        commit_count += 1;
                        committed_messages.push(CommittedChatMessage {
                            path: path.as_str().to_string(),
                            mode: *mode,
                            message_id: message_id.clone(),
                            round,
                        });
                        self.event(
                            run_id,
                            AgentRunEventLevel::Info,
                            "chat_commit_recorded",
                            json!({
                                "commitCount": commit_count,
                                "path": path.as_str(),
                                "mode": mode,
                                "messageId": message_id.as_deref(),
                            }),
                        )
                        .await?;
                    }
                    AgentToolEffect::Finish => {
                        finished = true;
                    }
                    AgentToolEffect::None => {}
                }

                tool_results.push(outcome.result);
                self.ensure_not_cancelled(cancel)?;
            }

            if finished {
                self.event(
                    run_id,
                    AgentRunEventLevel::Info,
                    "agent_loop_finished",
                    json!({ "commitCount": commit_count, "round": round }),
                )
                .await?;
                return Ok(Some(commit_count));
            }

            let tool_results = self
                .hydrate_recent_tool_results_for_model(run_id, round, &tool_results)
                .await?;
            append_tool_turn_to_request(&mut request, assistant_message, &tool_results)?;
            self.ensure_not_cancelled(cancel)?;
        }

        // We ran out of rounds before workspace_finish was called. Any
        // commits made along the way are now orphaned — surface them so the
        // host can offer a clean Retry that rolls them back.
        self.emit_run_rollback_targets(
            run_id,
            &committed_messages,
            "agent.max_tool_rounds_exceeded",
            profile.tools.max_rounds,
        )
        .await?;

        Ok(None)
    }

    async fn emit_run_rollback_targets(
        &self,
        run_id: &str,
        committed_messages: &[CommittedChatMessage],
        reason_code: &str,
        round: usize,
    ) -> Result<(), ApplicationError> {
        if committed_messages.is_empty() {
            return Ok(());
        }

        let targets = committed_messages
            .iter()
            .map(|message| {
                json!({
                    "path": message.path,
                    "mode": message.mode,
                    "messageId": message.message_id.as_deref(),
                    "round": message.round,
                })
            })
            .collect::<Vec<_>>();
        self.event(
            run_id,
            AgentRunEventLevel::Warn,
            "run_rollback_targets",
            json!({
                "reasonCode": reason_code,
                "round": round,
                "targetCount": targets.len(),
                "targets": Value::Array(targets),
            }),
        )
        .await?;
        Ok(())
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

/// Build the corrective `user` message we inject when the model returns a
/// turn with zero tool calls. The phrasing covers both common drift modes:
///
/// * **Post-commit drift** (committed_count > 0): model committed a chat
///   message but then replied with plain text instead of calling
///   `workspace_finish`. We tell it that the commit will be rolled back if
///   it doesn't finish, and that workspace edits only affect the chat after
///   another `workspace_commit`.
/// * **No-commit drift** (committed_count == 0): model bypassed the tool
///   workflow entirely. We tell it that every turn must use a tool until
///   `workspace_finish`.
///
/// The attempt counter is included so the model can see we have a hard
/// budget; if attempt == max_attempts there is no further leniency.
fn build_drift_recovery_nudge(committed_count: usize, attempt: usize, max_attempts: usize) -> String {
    if committed_count > 0 {
        format!(
            "[system reminder, drift recovery attempt {attempt}/{max_attempts}] You replied with \
             plain text but the run is not complete. You have committed {committed_count} \
             message(s) to the chat via workspace_commit; you MUST finalize the run by calling \
             workspace_finish, or the commit(s) will be ROLLED BACK and the run will fail. If \
             you need to revise the committed content, update the workspace file with \
             workspace_apply_patch or workspace_write_file, then call workspace_commit again \
             before workspace_finish. Do NOT repeat the content in plain text — that is treated \
             as instruction drift."
        )
    } else {
        format!(
            "[system reminder, drift recovery attempt {attempt}/{max_attempts}] You replied with \
             plain text, but every turn must use a tool until the run ends with workspace_finish. \
             Inspect the workspace (workspace_list_files / workspace_read_file), produce the \
             answer through workspace_write_file + workspace_commit, then call workspace_finish. \
             Do NOT answer directly in plain text."
        )
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
