use std::collections::HashSet;

use serde_json::{Value, json};

use super::commit_ledger::RunCommitLedger;
use super::model_turn::{
    append_tool_turn_to_request, assistant_message_for_next_turn, extract_response_text,
};
use super::model_turn_display::model_turn_event_summary;
use super::prompt_snapshot::request_summary;
use super::{AgentCancelReceiver, AgentRuntimeService};
use crate::application::errors::ApplicationError;
use crate::application::services::agent_tools::{AGENT_AWAIT, AgentToolEffect, AgentToolSession};
use crate::domain::models::agent::profile::ResolvedAgentProfile;

use crate::domain::models::agent::{
    AgentInvocationExitPolicy, AgentModelContentPart, AgentModelMessage, AgentModelRequest,
    AgentModelResponse, AgentModelRole, AgentRunEventLevel, AgentRunStatus, AgentToolResult,
    WorkspacePath,
};
use crate::domain::models::skill::SkillIndexEntry;
use crate::domain::repositories::workspace_repository::WorkspaceRepository;
use crate::domain::text_metrics::TextMetrics;

/// How many in-loop drift recovery attempts to make per run before
/// surrendering to the existing #55 fail-fast path. One attempt is
/// enough for the common case (model forgets `workspace_finish` after
/// the final commit) without burning excessive tokens on stubborn
/// drifters; raise this only after we have data showing repeat drifts
/// are common AND benign.
const DRIFT_RECOVERY_MAX_ATTEMPTS: usize = 1;

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
    ) -> Result<Option<usize>, ApplicationError> {
        let mut tool_session = AgentToolSession::new(effective_skills.to_vec());
        let mut seen_child_result_task_ids = HashSet::new();
        let mut commit_count = 0_usize;
        // Issue #64: counter for soft drift recovery — see
        // `DRIFT_RECOVERY_MAX_ATTEMPTS` above. Persisted across rounds so a
        // single run gets at most N corrective nudges in total, not N per
        // drift event.
        let mut drift_recovery_attempts: usize = 0;
        for round in 1..=profile.tools.max_rounds {
            let updates_run_status = exit_policy == AgentInvocationExitPolicy::RunFinishAllowed;
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
                let can_recover = drift_recovery_attempts < DRIFT_RECOVERY_MAX_ATTEMPTS
                    && round < profile.tools.max_rounds;
                if can_recover {
                    drift_recovery_attempts += 1;
                    let committed_count = commit_ledger.len();
                    let nudge_text = build_drift_recovery_nudge(
                        committed_count,
                        drift_recovery_attempts,
                        DRIFT_RECOVERY_MAX_ATTEMPTS,
                        direct_output_path.as_ref(),
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
                    "model.tool_call_required: model must use Agent tools and finish through {}",
                    completion_tool_name(exit_policy)
                )));
            }

            let assistant_message = assistant_message_for_next_turn(&response)?;
            let mut tool_results = Vec::with_capacity(tool_calls.len());
            let mut finished = false;

            for call in tool_calls {
                if finished {
                    return Err(ApplicationError::ValidationError(format!(
                        "agent.tool_after_finish: model requested additional tools after {}",
                        completion_tool_name(exit_policy)
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
                    AgentToolEffect::WorkspaceFileWritten { file } => {
                        let metrics = TextMetrics::from_text(&file.text);
                        self.checkpoint_workspace_file(
                            run_id,
                            updates_run_status,
                            "tool_workspace_write",
                            "workspace_file_written",
                            json!({
                                "invocationId": invocation_id,
                                "path": file.path.as_str(),
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
                return Ok(Some(commit_count));
            }

            let tool_results = if exit_policy == AgentInvocationExitPolicy::TaskReturnRequired {
                let workspace_view = self
                    .child_workspace_view(run_id, invocation_id, profile)
                    .await?;
                let workspace_repository =
                    workspace_view.repository(self.workspace_repository.as_ref());
                self.hydrate_recent_tool_results_for_model(
                    run_id,
                    round,
                    &tool_results,
                    &workspace_repository,
                )
                .await?
            } else {
                self.hydrate_recent_tool_results_for_model(
                    run_id,
                    round,
                    &tool_results,
                    self.workspace_repository.as_ref(),
                )
                .await?
            };
            remember_seen_child_results_from_await(&tool_results, &mut seen_child_result_task_ids);
            append_tool_turn_to_request(&mut request, assistant_message, &tool_results)?;
            if exit_policy == AgentInvocationExitPolicy::RunFinishAllowed {
                if let Some(message) = self
                    .completed_child_results_message(
                        run_id,
                        invocation_id,
                        &mut seen_child_result_task_ids,
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

    async fn hydrate_recent_tool_results_for_model(
        &self,
        run_id: &str,
        round: usize,
        tool_results: &[AgentToolResult],
        workspace_repository: &dyn WorkspaceRepository,
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
            let file = workspace_repository
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
                {
                    let metrics = TextMetrics::from_text(&file.text);
                    json!({
                        "round": round,
                        "callId": result.call_id.as_str(),
                        "path": file.path.as_str(),
                        "chars": metrics.chars,
                        "words": metrics.words,
                    })
                },
            )
            .await?;
            hydrated.push(result);
        }

        Ok(hydrated)
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

fn completion_tool_name(exit_policy: AgentInvocationExitPolicy) -> &'static str {
    match exit_policy {
        AgentInvocationExitPolicy::RunFinishAllowed => "workspace_finish",
        AgentInvocationExitPolicy::TaskReturnRequired => "task_return",
    }
}

/// Build the corrective `user` message we inject when the model returns a
/// turn with zero tool calls. The phrasing covers both common drift modes:
///
/// * **Post-commit drift** (committed_count > 0): model committed a chat
///   message but then replied with plain text instead of calling
///   `workspace_finish`. We tell it that clean completion still requires
///   `workspace_finish`, and that workspace edits only affect the chat after
///   another `workspace_commit`.
/// * **No-commit drift** (committed_count == 0): model bypassed the tool
///   workflow entirely. We tell it that every turn must use a tool until
///   `workspace_finish`.
///
/// The attempt counter is included so the model can see we have a hard
/// budget; if attempt == max_attempts there is no further leniency.
fn build_drift_recovery_nudge(
    committed_count: usize,
    attempt: usize,
    max_attempts: usize,
    direct_output_path: Option<&WorkspacePath>,
) -> String {
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
            "[system reminder, drift recovery attempt {attempt}/{max_attempts}] You replied with \
             plain text but the run is not complete. You have committed {committed_count} \
             message(s) to the chat via workspace_commit; you MUST finalize the run by calling \
             workspace_finish, or the run will stop as partial_success with a warning instead of \
             clean completion. If you need to revise the committed content, update the workspace file with \
             workspace_apply_patch or workspace_write_file, then call workspace_commit again \
             before workspace_finish.{direct_output_hint} Do NOT repeat the content in plain text — \
             that is treated as instruction drift."
        )
    } else {
        format!(
            "[system reminder, drift recovery attempt {attempt}/{max_attempts}] You replied with \
             plain text, but every turn must use a tool until the run ends with workspace_finish. \
             Inspect the workspace (workspace_list_files / workspace_read_file), produce the \
             answer through workspace_write_file + workspace_commit, then call workspace_finish.{direct_output_hint} \
             Do NOT answer directly in plain text."
        )
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
