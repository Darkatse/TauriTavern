use std::collections::HashSet;

use serde_json::{Value, json};

use super::commit_ledger::RunCommitLedger;
use super::model_turn_display::model_turn_event_summary;
use super::prompt_snapshot::request_summary;
use super::{AgentCancelReceiver, AgentRuntimeService, PreparedInvocation};
use crate::errors::ApplicationError;
use crate::services::agent_tools::{AGENT_AWAIT, AGENT_HANDOFF, AgentToolEffect, AgentToolSession};
use crate::services::tool_request_gate::ToolRequestGate;
use tt_domain::models::agent::profile::ResolvedAgentProfile;
use tt_domain::models::agent::{
    AgentInvocationExitPolicy, AgentInvocationStatus, AgentModelContentPart, AgentModelMessage,
    AgentModelResponse, AgentModelRole, AgentRunEventLevel, AgentRunPresentation, AgentRunStatus,
    AgentToolResult, WorkspaceFileWriteMode, WorkspacePath,
};
use tt_domain::models::tool::ToolTurnContract;
use tt_domain::text_metrics::TextMetrics;

pub(super) enum AgentLoopExit {
    Finished,
    Transferred {
        task_id: String,
        new_invocation_id: String,
    },
}

impl AgentRuntimeService {
    pub(super) async fn run_tool_loop(
        &self,
        prepared: &mut PreparedInvocation,
        commit_ledger: &mut RunCommitLedger,
        cancel: &mut AgentCancelReceiver,
    ) -> Result<Option<AgentLoopExit>, ApplicationError> {
        let run_id = prepared.invocation.run_id.as_str();
        let invocation_id = prepared.invocation.id.as_str();
        let exit_policy = prepared.invocation.exit_policy;
        let profile = &prepared.profile;
        let updates_run_status = exit_policy == AgentInvocationExitPolicy::RunFinishAllowed;
        let auto_commit_text_mutations = updates_run_status
            && self.run_repository.load_run(run_id).await?.presentation
                == AgentRunPresentation::Foreground;
        let stream = self
            .active_run_handle(run_id)
            .await?
            .stream_enabled(profile.run.stream);
        let mut tool_session = AgentToolSession::new(prepared.effective_skills.clone());
        tool_session.frozen_macros = prepared.frozen_macros.clone();
        let mut tool_request_gate = ToolRequestGate::default();
        let mut seen_child_result_task_ids = HashSet::new();
        // Counts soft drift recovery nudges for model-facing text and
        // journal events. It is intentionally not a separate budget: the
        // existing maxRounds loop remains the only retry boundary.
        let mut drift_recovery_attempts: usize = 0;
        for round in 1..=profile.tools.max_rounds {
            if updates_run_status {
                self.apply_pending_guidance_to_request(
                    run_id,
                    invocation_id,
                    round,
                    &mut prepared.request,
                )
                .await?;
            }
            if updates_run_status {
                self.transition_status(run_id, AgentRunStatus::CallingModel)
                    .await?;
            }
            self.event(
                run_id,
                AgentRunEventLevel::Info,
                "model_request_created",
                json!({
                    "round": round,
                    "invocationId": invocation_id,
                    "toolTurn": &prepared.tool_turn,
                    "request": request_summary(&prepared.request),
                }),
            )
            .await?;

            let exchange = self
                .generate_model_with_retry(
                    &prepared.invocation,
                    round,
                    &prepared.request,
                    &profile.run.model_retry,
                    stream,
                    cancel,
                )
                .await?;
            self.ensure_not_cancelled(cancel)?;
            let response = exchange.response;
            let model_response_path = self
                .store_model_response(run_id, invocation_id, round, &response)
                .await?;
            prepared.request.provider_state = exchange.provider_state;
            self.event(
                run_id,
                AgentRunEventLevel::Debug,
                "provider_state_updated",
                json!({
                    "round": round,
                    "invocationId": invocation_id,
                    "providerState": provider_state_summary(&prepared.request.provider_state),
                }),
            )
            .await?;

            let tool_call_count = response.tool_calls.len();
            self.event(run_id, AgentRunEventLevel::Info, "model_completed", {
                let mut payload = model_turn_event_summary(&response);
                let object = payload
                    .as_object_mut()
                    .expect("model turn event summary must be a JSON object");
                object.insert("round".to_string(), json!(round));
                object.insert("invocationId".to_string(), json!(invocation_id));
                object.insert(
                    "modelResponsePath".to_string(),
                    json!(model_response_path.as_str()),
                );
                object.insert("toolCallCount".to_string(), json!(tool_call_count));
                let text_metrics = TextMetrics::from_text(response.text.as_str());
                object.insert("textChars".to_string(), json!(text_metrics.chars));
                object.insert("textWords".to_string(), json!(text_metrics.words));
                payload
            })
            .await?;

            if response.tool_calls.is_empty() {
                // Issue #64: instead of failing the run immediately, let the
                // model self-correct while normal tool-loop rounds remain.
                // Direct output is usually a contract slip, not a host
                // failure. We push the drifted assistant turn into history
                // (so the model owns what it just said) and follow it with a
                // synthetic `user` reminder. The existing maxRounds/cancel
                // contract is the boundary; there is no extra direct-output
                // attempt cap.
                let direct_output_path = self
                    .capture_direct_output(
                        run_id,
                        round,
                        model_response_path.as_str(),
                        &response,
                        profile,
                    )
                    .await?;
                let can_recover = round < profile.tools.max_rounds;
                if can_recover {
                    drift_recovery_attempts += 1;
                    let committed_count = commit_ledger.explicit_count();
                    let nudge_text = build_drift_recovery_nudge(
                        committed_count,
                        drift_recovery_attempts,
                        direct_output_path.as_ref(),
                        exit_policy,
                        &prepared.tool_turn,
                    );
                    prepared.request.messages.push(response.message);
                    prepared.request.messages.push(AgentModelMessage {
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
                            "maxAttempts": drift_recovery_attempt_limit(profile.tools.max_rounds),
                            "maxRounds": profile.tools.max_rounds,
                            "limitReason": "max_rounds",
                            "round": round,
                            "invocationId": invocation_id,
                            "committedCount": committed_count,
                            "reasonCode": "model.tool_call_required",
                        }),
                    )
                    .await?;
                    self.ensure_not_cancelled(cancel)?;
                    continue;
                }
                return Err(ApplicationError::ValidationError(format!(
                    "model.tool_call_required: model must use Agent tools and complete through {}",
                    completion_tool_name(exit_policy, &prepared.tool_turn)
                )));
            }

            let assistant_message = response.message;
            let tool_calls = response.tool_calls;
            let mut tool_results = Vec::with_capacity(tool_calls.len());
            let mut finished = false;
            let mut handoff = None;
            let tool_call_count = tool_calls.len();
            let mut auto_commit_candidate = None;

            for (index, call) in tool_calls.into_iter().enumerate() {
                let outcome = self
                    .dispatch_tool_call(
                        prepared,
                        round,
                        index,
                        &call,
                        &mut tool_request_gate,
                        &mut tool_session,
                        index + 1 == tool_call_count,
                        commit_ledger,
                        cancel,
                    )
                    .await?;
                let result = outcome.result;
                match outcome.effect {
                    AgentToolEffect::WorkspaceFileWritten { file, mode } => {
                        let metrics = TextMetrics::from_text(&file.text);
                        self.event(
                            run_id,
                            AgentRunEventLevel::Info,
                            "workspace_file_written",
                            json!({
                                "invocationId": invocation_id,
                                "path": file.path.as_str(),
                                "mode": mode,
                                "chars": metrics.chars,
                                "words": metrics.words,
                                "sha256": file.sha256.as_str(),
                            }),
                        )
                        .await?;
                        auto_commit_candidate = if stream {
                            None
                        } else {
                            Some((call.call_id, file))
                        };
                    }
                    AgentToolEffect::WorkspaceFilesWritten {
                        files,
                        last_text_mutation,
                    } => {
                        for file in &files {
                            let metrics = TextMetrics::from_text(&file.text);
                            self.event(
                                run_id,
                                AgentRunEventLevel::Info,
                                "workspace_file_written",
                                json!({
                                    "invocationId": invocation_id,
                                    "path": file.path.as_str(),
                                    "mode": WorkspaceFileWriteMode::Replace,
                                    "chars": metrics.chars,
                                    "words": metrics.words,
                                    "sha256": file.sha256.as_str(),
                                }),
                            )
                            .await?;
                        }
                        if let Some(path) = last_text_mutation {
                            let file = files
                                .into_iter()
                                .find(|file| file.path == path)
                                .ok_or_else(|| {
                                    ApplicationError::InternalError(format!(
                                        "Workspace batch effect is missing its last mutation `{}`",
                                        path.as_str()
                                    ))
                                })?;
                            auto_commit_candidate = Some((call.call_id, file));
                        }
                    }
                    AgentToolEffect::WorkspaceFilePatched {
                        file,
                        replacements,
                        old_sha256,
                    } => {
                        if updates_run_status {
                            self.transition_status(run_id, AgentRunStatus::ApplyingWorkspacePatch)
                                .await?;
                        }
                        let metrics = TextMetrics::from_text(&file.text);
                        self.event(
                            run_id,
                            AgentRunEventLevel::Info,
                            "workspace_patch_applied",
                            json!({
                                "invocationId": invocation_id,
                                "path": file.path.as_str(),
                                "chars": metrics.chars,
                                "words": metrics.words,
                                "oldSha256": old_sha256,
                                "sha256": file.sha256.as_str(),
                                "replacements": replacements,
                            }),
                        )
                        .await?;
                        auto_commit_candidate = Some((call.call_id, file));
                    }
                    AgentToolEffect::ChatCommitRequested { .. } => {}
                    AgentToolEffect::Finish => {
                        finished = true;
                    }
                    AgentToolEffect::TaskReturned {
                        status,
                        result_ref,
                        summary,
                    } => {
                        let metrics = TextMetrics::from_text(&summary);
                        self.event(
                            run_id,
                            AgentRunEventLevel::Info,
                            "task_return_recorded",
                            json!({
                                "invocationId": invocation_id,
                                "status": status,
                                "resultRef": result_ref.as_str(),
                                "summaryChars": metrics.chars,
                                "summaryWords": metrics.words,
                            }),
                        )
                        .await?;
                        finished = true;
                    }
                    AgentToolEffect::HandoffAccepted {
                        task_id,
                        new_invocation_id,
                        ..
                    } => {
                        handoff = Some((task_id, new_invocation_id));
                        self.finish_invocation(
                            run_id,
                            invocation_id,
                            AgentInvocationStatus::Transferred,
                        )
                        .await?;
                        finished = true;
                    }
                    AgentToolEffect::None => {}
                }

                tool_results.push(result);
                self.ensure_not_cancelled(cancel)?;
            }

            if auto_commit_text_mutations && let Some((call_id, file)) = auto_commit_candidate {
                self.auto_commit_text_file_if_eligible(
                    run_id,
                    &call_id,
                    &file,
                    round,
                    invocation_id,
                    commit_ledger,
                    cancel,
                )
                .await?;
            }

            if finished {
                self.event(
                    run_id,
                    AgentRunEventLevel::Info,
                    "agent_loop_finished",
                    json!({
                        "commitCount": commit_ledger.len(),
                        "round": round,
                        "invocationId": invocation_id,
                    }),
                )
                .await?;
                return Ok(Some(if let Some((task_id, new_invocation_id)) = handoff {
                    AgentLoopExit::Transferred {
                        task_id,
                        new_invocation_id,
                    }
                } else {
                    AgentLoopExit::Finished
                }));
            }

            remember_seen_child_results_from_await(&tool_results, &mut seen_child_result_task_ids);
            prepared.request.messages.push(assistant_message);
            prepared
                .request
                .messages
                .extend(tool_results.into_iter().map(|result| AgentModelMessage {
                    role: AgentModelRole::Tool,
                    parts: vec![AgentModelContentPart::ToolResult { result }],
                    provider_metadata: Value::Null,
                }));
            if exit_policy == AgentInvocationExitPolicy::RunFinishAllowed
                && let Some(message) = self
                    .completed_child_results_message(
                        prepared,
                        &mut seen_child_result_task_ids,
                        commit_ledger.explicit_count(),
                    )
                    .await?
            {
                prepared.request.messages.push(AgentModelMessage {
                    role: AgentModelRole::User,
                    parts: vec![AgentModelContentPart::Text { text: message }],
                    provider_metadata: Value::Null,
                });
            }
            self.ensure_not_cancelled(cancel)?;
        }

        Ok(None)
    }

    async fn capture_direct_output(
        &self,
        run_id: &str,
        round: usize,
        model_response_path: &str,
        response: &AgentModelResponse,
        profile: &ResolvedAgentProfile,
    ) -> Result<Option<WorkspacePath>, ApplicationError> {
        let text = response.text.as_str();
        if text.trim().is_empty() {
            return Ok(None);
        }

        let path = direct_output_path(profile)?;
        let file = self
            .workspace_repository
            .write_text(run_id, &path, text)
            .await?;
        let metrics = TextMetrics::from_text(&file.text);
        self.event(
            run_id,
            AgentRunEventLevel::Info,
            "direct_output_captured",
            json!({
                "round": round,
                "path": file.path.as_str(),
                "chars": metrics.chars,
                "words": metrics.words,
                "sha256": file.sha256.as_str(),
                "modelResponsePath": model_response_path,
            }),
        )
        .await?;

        Ok(Some(file.path))
    }
}

fn remember_seen_child_results_from_await(
    tool_results: &[AgentToolResult],
    seen_task_ids: &mut HashSet<String>,
) {
    for result in tool_results {
        if !result.tool_id.is_builtin()
            || result.tool_id.native_name() != AGENT_AWAIT
            || result.is_error
        {
            continue;
        }
        let Some(tasks) = result.structured.get("tasks").and_then(Value::as_array) else {
            continue;
        };
        for task in tasks {
            let Some(status) = task.get("status").and_then(Value::as_str) else {
                continue;
            };
            if !matches!(status, "completed" | "failed" | "cancelled") {
                continue;
            }
            if let Some(task_id) = task
                .get("taskId")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                seen_task_ids.insert(task_id.to_string());
            }
        }
    }
}

fn completion_tool_name(
    exit_policy: AgentInvocationExitPolicy,
    turn: &ToolTurnContract,
) -> &'static str {
    match exit_policy {
        AgentInvocationExitPolicy::RunFinishAllowed => {
            if turn_has_builtin(turn, "workspace.finish") {
                "workspace_finish"
            } else if turn_has_builtin(turn, AGENT_HANDOFF) {
                "agent_handoff"
            } else {
                "an available Agent control tool"
            }
        }
        AgentInvocationExitPolicy::TaskReturnRequired => "task_return",
    }
}

fn turn_has_builtin(turn: &ToolTurnContract, native_name: &str) -> bool {
    turn.tools()
        .iter()
        .any(|tool_id| tool_id.is_builtin() && tool_id.native_name() == native_name)
}

fn drift_recovery_attempt_limit(max_rounds: usize) -> usize {
    max_rounds.saturating_sub(1)
}

/// Build the corrective `user` message we inject when the model returns a
/// turn with zero tool calls. The phrasing covers the common drift modes:
///
/// * **Post-commit drift** (committed_count > 0): model committed a chat
///   message but then replied with plain text instead of using the current
///   stage completion tool. We tell it to complete with `workspace_finish`
///   when available, or continue with `agent_handoff` for handoff-only stages.
/// * **No-commit drift** (committed_count == 0): model bypassed the tool
///   workflow entirely. We tell it that every turn must use a tool until
///   the stage is finished or transferred.
/// * **Child drift** (TaskReturnRequired): return-mode subagents cannot
///   commit or finish the run, so we direct them back to `task_return`.
///
fn build_drift_recovery_nudge(
    committed_count: usize,
    attempt: usize,
    direct_output_path: Option<&WorkspacePath>,
    exit_policy: AgentInvocationExitPolicy,
    turn: &ToolTurnContract,
) -> String {
    match exit_policy {
        AgentInvocationExitPolicy::RunFinishAllowed => {
            if turn_has_builtin(turn, "workspace.finish") {
                let direct_output_hint = direct_output_path
                    .map(|path| {
                        format!(
                            " I saved your direct text to {}. If that text is the intended reply, call workspace_commit with path \"{}\" before workspace_finish.",
                            path.as_str(),
                            path.as_str()
                        )
                    })
                    .unwrap_or_default();

                if committed_count > 0 {
                    format!(
                        "[system reminder, direct output recovery attempt {attempt}] You replied with \
                         plain text but the run is still open. You have committed {committed_count} \
                         message(s) to the chat via workspace_commit; complete cleanly by calling \
                         workspace_finish. If you need to revise the committed content, update the workspace file with \
                         workspace_apply_patch or workspace_write_file, then call workspace_commit again \
                         before workspace_finish.{direct_output_hint} Do NOT repeat the content in plain text; \
                         continue through Agent tools."
                    )
                } else {
                    format!(
                        "[system reminder, direct output recovery attempt {attempt}] You replied with \
                         plain text, but this run must continue through Agent tools until workspace_finish. \
                         Inspect the workspace if needed, produce the answer through workspace_write_file \
                         and workspace_commit, then call workspace_finish.{direct_output_hint} \
                         Do NOT answer directly in plain text."
                    )
                }
            } else if turn_has_builtin(turn, AGENT_HANDOFF) {
                let direct_output_hint = direct_output_path
                    .map(|path| {
                        format!(
                            " I saved your direct text to {}. If it is useful, mention that path in the handoff brief.",
                            path.as_str()
                        )
                    })
                    .unwrap_or_default();

                format!(
                    "[system reminder, direct output recovery attempt {attempt}] You replied with \
                     plain text, but this Agent stage cannot finish the run directly. Continue by \
                     calling agent_handoff with a clear objective, context summary, workspace references, \
                     and preservation constraints for the next Agent.{direct_output_hint} Do NOT answer \
                     directly in plain text."
                )
            } else {
                let direct_output_hint = direct_output_path
                    .map(|path| {
                        format!(
                            " I saved your direct text to {}. If it is useful, reference that path when continuing.",
                            path.as_str()
                        )
                    })
                    .unwrap_or_default();
                format!(
                    "[system reminder, direct output recovery attempt {attempt}] You replied with \
                     plain text, but this run must continue through Agent tools. Use an available \
                     Agent control tool to continue or complete the stage.{direct_output_hint} Do NOT \
                     answer directly in plain text."
                )
            }
        }
        AgentInvocationExitPolicy::TaskReturnRequired => {
            let direct_output_hint = direct_output_path
                .map(|path| {
                    format!(
                        " I saved your direct text to {}. If it is useful, summarize it or reference that path in task_return.artifacts.",
                        path.as_str()
                    )
                })
                .unwrap_or_default();
            format!(
                "[system reminder, direct output recovery attempt {attempt}] You replied with \
                 plain text, but this delegated task must end through task_return. \
                 Call task_return with a concise summary, status, and any useful findings, warnings, \
                 questions, next actions, or artifact paths.{direct_output_hint} Do NOT answer directly \
                 in plain text."
            )
        }
    }
}

fn direct_output_path(profile: &ResolvedAgentProfile) -> Result<WorkspacePath, ApplicationError> {
    let message_body_path = WorkspacePath::parse(&profile.output.message_body_path)?;
    let root = message_body_path
        .as_str()
        .split('/')
        .next()
        .unwrap_or("output");
    WorkspacePath::parse(format!("{root}/direct_output.md")).map_err(ApplicationError::from)
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
