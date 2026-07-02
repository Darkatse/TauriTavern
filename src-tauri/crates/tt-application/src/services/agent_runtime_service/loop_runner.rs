use std::collections::HashSet;

use serde_json::{Value, json};

use super::commit_ledger::RunCommitLedger;
use super::model_turn::{
    append_tool_turn_to_request, assistant_message_for_next_turn, extract_response_text,
};
use super::model_turn_display::model_turn_event_summary;
use super::prompt_snapshot::request_summary;
use super::{AgentCancelReceiver, AgentRuntimeService};
use crate::errors::ApplicationError;
use crate::services::agent_tools::{AGENT_AWAIT, AGENT_HANDOFF, AgentToolEffect, AgentToolSession};
use tt_domain::models::agent::profile::ResolvedAgentProfile;

use tt_domain::models::agent::{
    AgentInvocationExitPolicy, AgentInvocationStatus, AgentModelContentPart, AgentModelMessage,
    AgentModelRequest, AgentModelResponse, AgentModelRole, AgentRunEventLevel, AgentRunStatus,
    AgentToolResult, WorkspacePath,
};
use tt_domain::models::skill::SkillIndexEntry;
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
        run_id: &str,
        invocation_id: &str,
        exit_policy: AgentInvocationExitPolicy,
        mut request: AgentModelRequest,
        profile: &ResolvedAgentProfile,
        effective_skills: &[SkillIndexEntry],
        commit_ledger: &mut RunCommitLedger,
        cancel: &mut AgentCancelReceiver,
    ) -> Result<Option<AgentLoopExit>, ApplicationError> {
        let mut tool_session = AgentToolSession::new(effective_skills.to_vec());
        let mut seen_child_result_task_ids = HashSet::new();
        let mut commit_count = commit_ledger.len();
        // Counts soft drift recovery nudges for model-facing text and
        // journal events. It is intentionally not a separate budget: the
        // existing maxRounds loop remains the only retry boundary.
        let mut drift_recovery_attempts: usize = 0;
        for round in 1..=profile.tools.max_rounds {
            let updates_run_status = exit_policy == AgentInvocationExitPolicy::RunFinishAllowed;
            if updates_run_status {
                self.apply_pending_guidance_to_request(run_id, invocation_id, round, &mut request)
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
                    "request": request_summary(&request),
                }),
            )
            .await?;

            let exchange = self
                .generate_model_with_retry(
                    run_id,
                    invocation_id,
                    round,
                    &request,
                    &profile.run.model_retry,
                    cancel,
                )
                .await?;
            self.ensure_not_cancelled(cancel)?;
            let response = exchange.response;
            let model_response_path = self
                .store_model_response(run_id, invocation_id, round, &response)
                .await?;
            request.provider_state = exchange.provider_state;
            self.event(
                run_id,
                AgentRunEventLevel::Debug,
                "provider_state_updated",
                json!({
                    "round": round,
                    "invocationId": invocation_id,
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
                object.insert("invocationId".to_string(), json!(invocation_id));
                object.insert(
                    "modelResponsePath".to_string(),
                    json!(model_response_path.as_str()),
                );
                object.insert("toolCallCount".to_string(), json!(tool_calls.len()));
                let text_metrics = TextMetrics::from_text(extract_response_text(&response));
                object.insert("textChars".to_string(), json!(text_metrics.chars));
                object.insert("textWords".to_string(), json!(text_metrics.words));
                payload
            })
            .await?;

            if tool_calls.is_empty() {
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
                        updates_run_status,
                        round,
                        model_response_path.as_str(),
                        &response,
                        profile,
                    )
                    .await?;
                let can_recover = round < profile.tools.max_rounds;
                if can_recover {
                    drift_recovery_attempts += 1;
                    let committed_count = commit_ledger.len();
                    let nudge_text = build_drift_recovery_nudge(
                        committed_count,
                        drift_recovery_attempts,
                        direct_output_path.as_ref(),
                        exit_policy,
                        profile,
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
                    completion_tool_name(exit_policy, profile)
                )));
            }

            let assistant_message = assistant_message_for_next_turn(&response)?;
            let mut tool_results = Vec::with_capacity(tool_calls.len());
            let mut finished = false;
            let mut handoff = None;
            let mut completion_tool = completion_tool_name(exit_policy, profile);

            for call in tool_calls {
                if finished {
                    return Err(ApplicationError::ValidationError(format!(
                        "agent.tool_after_finish: model requested additional tools after {}",
                        completion_tool
                    )));
                }

                let outcome = self
                    .dispatch_tool_call(
                        run_id,
                        invocation_id,
                        exit_policy,
                        round,
                        &call,
                        &mut tool_session,
                        profile,
                        commit_count,
                        commit_ledger,
                        cancel,
                    )
                    .await?;
                match &outcome.effect {
                    AgentToolEffect::WorkspaceFileWritten { file, mode } => {
                        let metrics = TextMetrics::from_text(&file.text);
                        self.checkpoint_workspace_file(
                            run_id,
                            updates_run_status,
                            "tool_workspace_write",
                            "workspace_file_written",
                            json!({
                                "invocationId": invocation_id,
                                "path": file.path.as_str(),
                                "mode": mode,
                                "chars": metrics.chars,
                                "words": metrics.words,
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
                        if updates_run_status {
                            self.transition_status(run_id, AgentRunStatus::ApplyingWorkspacePatch)
                                .await?;
                        }
                        let metrics = TextMetrics::from_text(&file.text);
                        self.checkpoint_workspace_file(
                            run_id,
                            updates_run_status,
                            "tool_workspace_patch",
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
                        self.event(
                            run_id,
                            AgentRunEventLevel::Info,
                            "chat_commit_recorded",
                            json!({
                                "invocationId": invocation_id,
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
                    AgentToolEffect::TaskReturned {
                        status,
                        result_ref,
                        summary,
                    } => {
                        let metrics = TextMetrics::from_text(summary);
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
                        handoff = Some((task_id.clone(), new_invocation_id.clone()));
                        self.finish_invocation(
                            run_id,
                            invocation_id,
                            AgentInvocationStatus::Transferred,
                        )
                        .await?;
                        completion_tool = "agent_handoff";
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
                    json!({
                        "commitCount": commit_count,
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
            append_tool_turn_to_request(&mut request, assistant_message, &tool_results)?;
            if exit_policy == AgentInvocationExitPolicy::RunFinishAllowed {
                if let Some(message) = self
                    .completed_child_results_message(
                        run_id,
                        invocation_id,
                        &mut seen_child_result_task_ids,
                        profile,
                        commit_count,
                    )
                    .await?
                {
                    request.messages.push(AgentModelMessage {
                        role: AgentModelRole::User,
                        parts: vec![AgentModelContentPart::Text { text: message }],
                        provider_metadata: Value::Null,
                    });
                }
            }
            self.ensure_not_cancelled(cancel)?;
        }

        Ok(None)
    }

    async fn capture_direct_output(
        &self,
        run_id: &str,
        update_run_status: bool,
        round: usize,
        model_response_path: &str,
        response: &AgentModelResponse,
        profile: &ResolvedAgentProfile,
    ) -> Result<Option<WorkspacePath>, ApplicationError> {
        let text = extract_response_text(response);
        if text.trim().is_empty() {
            return Ok(None);
        }

        let path = direct_output_path(profile)?;
        let file = self
            .workspace_repository
            .write_text(run_id, &path, text)
            .await?;
        let metrics = TextMetrics::from_text(&file.text);
        self.checkpoint_workspace_file(
            run_id,
            update_run_status,
            "direct_output_capture",
            "direct_output_captured",
            json!({
                "round": round,
                "path": file.path.as_str(),
                "chars": metrics.chars,
                "words": metrics.words,
                "sha256": file.sha256.as_str(),
                "modelResponsePath": model_response_path,
            }),
            file.path.clone(),
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
        if result.name != AGENT_AWAIT || result.is_error {
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
    profile: &ResolvedAgentProfile,
) -> &'static str {
    match exit_policy {
        AgentInvocationExitPolicy::RunFinishAllowed => {
            if profile_tool_visible(profile, "workspace.finish") {
                "workspace_finish"
            } else if profile_tool_visible(profile, AGENT_HANDOFF) {
                "agent_handoff"
            } else {
                "an available Agent control tool"
            }
        }
        AgentInvocationExitPolicy::TaskReturnRequired => "task_return",
    }
}

fn profile_tool_visible(profile: &ResolvedAgentProfile, tool_name: &str) -> bool {
    profile.tools.allow.iter().any(|name| name == tool_name)
        && !profile.tools.deny.iter().any(|name| name == tool_name)
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
    profile: &ResolvedAgentProfile,
) -> String {
    match exit_policy {
        AgentInvocationExitPolicy::RunFinishAllowed => {
            if profile_tool_visible(profile, "workspace.finish") {
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
            } else if profile_tool_visible(profile, AGENT_HANDOFF) {
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
