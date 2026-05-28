use std::time::Instant;

use serde_json::json;
use sha2::{Digest, Sha256};

use super::AgentRuntimeService;
use super::commit_ledger::RunCommitLedger;
use crate::application::errors::ApplicationError;

use crate::application::services::agent_tools::{
    AGENT_AWAIT, AGENT_DELEGATE, AGENT_LIST, AgentToolDispatchOutcome, AgentToolEffect,
    AgentToolSession, TASK_RETURN,
};
use crate::domain::models::agent::profile::ResolvedAgentProfile;
use crate::domain::models::agent::{
    AgentInvocationExitPolicy, AgentRunEventLevel, AgentRunPresentation, AgentRunStatus,
    AgentToolCall, AgentToolResult, WorkspacePath,
};

const TOOL_CALL_AUDIT_DIGEST_BYTES: usize = 8;

impl AgentRuntimeService {
    pub(super) async fn dispatch_tool_call(
        &self,
        run_id: &str,
        invocation_id: &str,
        exit_policy: AgentInvocationExitPolicy,
        round: usize,
        call: &AgentToolCall,
        session: &mut AgentToolSession,
        profile: &ResolvedAgentProfile,
        commit_count: usize,
        commit_ledger: &mut RunCommitLedger,
        cancel: &mut super::AgentCancelReceiver,
    ) -> Result<AgentToolDispatchOutcome, ApplicationError> {
        let canonical_call = self
            .tool_registry
            .spec_by_name_or_model_name(&call.name)
            .filter(|spec| spec.name != call.name)
            .map(|spec| AgentToolCall {
                id: call.id.clone(),
                name: spec.name.clone(),
                arguments: call.arguments.clone(),
                provider_metadata: call.provider_metadata.clone(),
            });
        let call = canonical_call.as_ref().unwrap_or(call);
        let arguments_ref = self.store_tool_arguments(run_id, call).await?;
        self.event(
            run_id,
            AgentRunEventLevel::Info,
            "tool_call_requested",
            json!({
                "round": round,
                "invocationId": invocation_id,
                "callId": call.id.as_str(),
                "name": call.name.as_str(),
                "argumentsRef": arguments_ref.as_str(),
                "providerMetadata": &call.provider_metadata,
            }),
        )
        .await?;
        let started = Instant::now();

        if self.tool_registry.spec_by_name(&call.name).is_none() {
            let error = ApplicationError::ValidationError(format!(
                "model.unknown_tool_call: model requested unknown Agent tool `{}`",
                call.name
            ));
            self.event(
                run_id,
                AgentRunEventLevel::Error,
                "tool_call_failed",
                json!({
                    "round": round,
                    "invocationId": invocation_id,
                    "callId": call.id.as_str(),
                    "name": call.name.as_str(),
                    "message": error.to_string(),
                }),
            )
            .await?;
            return Err(error);
        }

        if !tool_is_visible(profile, call.name.as_str(), exit_policy) {
            let outcome = recoverable_tool_error(
                call,
                "agent.tool_policy_denied",
                &format!(
                    "Tool `{}` is not available in the current Agent profile.",
                    call.name
                ),
                started.elapsed().as_millis(),
            );
            self.record_tool_outcome(run_id, invocation_id, round, &outcome)
                .await?;
            return Ok(outcome);
        }

        if session.total_calls() >= profile.tools.max_calls_per_run {
            let outcome = recoverable_tool_error(
                call,
                "agent.tool_budget_exhausted",
                &format!(
                    "Agent profile tool call budget is exhausted for this run (max {}).",
                    profile.tools.max_calls_per_run
                ),
                started.elapsed().as_millis(),
            );
            self.record_tool_outcome(run_id, invocation_id, round, &outcome)
                .await?;
            return Ok(outcome);
        }

        if let Some(max_calls) = profile.tools.max_calls_per_tool.get(&call.name) {
            if session.calls_for_tool(&call.name) >= *max_calls {
                let outcome = recoverable_tool_error(
                    call,
                    "agent.tool_budget_exhausted",
                    &format!(
                        "Agent profile tool call budget for `{}` is exhausted (max {}).",
                        call.name, max_calls
                    ),
                    started.elapsed().as_millis(),
                );
                self.record_tool_outcome(run_id, invocation_id, round, &outcome)
                    .await?;
                return Ok(outcome);
            }
        }

        session.remember_tool_call(&call.name);
        if exit_policy == AgentInvocationExitPolicy::RunFinishAllowed {
            self.transition_status(run_id, AgentRunStatus::DispatchingTool)
                .await?;
        }
        self.event(
            run_id,
            AgentRunEventLevel::Info,
            "tool_call_started",
            json!({
                "round": round,
                "invocationId": invocation_id,
                "callId": call.id.as_str(),
                "name": call.name.as_str(),
            }),
        )
        .await?;

        let child_workspace_view = if exit_policy == AgentInvocationExitPolicy::TaskReturnRequired {
            Some(self.child_workspace_view(run_id, invocation_id).await?)
        } else {
            None
        };

        if exit_policy == AgentInvocationExitPolicy::TaskReturnRequired
            && child_workspace_view
                .as_ref()
                .is_some_and(|view| view.write_is_denied(call))
        {
            let outcome = recoverable_tool_error(
                call,
                "agent.child_workspace_write_denied",
                "Return-mode child Agents may write only under scratch/ or summaries/.",
                started.elapsed().as_millis(),
            );
            self.record_tool_outcome(run_id, invocation_id, round, &outcome)
                .await?;
            return Ok(outcome);
        }

        let dispatch_result = if call.name == AGENT_LIST {
            self.dispatch_agent_list_tool(call, profile).await
        } else if call.name == AGENT_DELEGATE {
            Box::pin(self.dispatch_agent_delegate_tool(
                run_id,
                invocation_id,
                call,
                profile,
                cancel,
            ))
            .await
        } else if call.name == AGENT_AWAIT {
            self.dispatch_agent_await_tool(run_id, invocation_id, call, cancel)
                .await
        } else if call.name == TASK_RETURN {
            self.dispatch_task_return_tool(run_id, invocation_id, call, exit_policy)
                .await
        } else if let Some(view) = child_workspace_view.as_ref() {
            let workspace_repository = view.repository(self.workspace_repository.as_ref());
            self.tool_dispatcher
                .dispatch_with_workspace_repository(
                    run_id,
                    call,
                    session,
                    profile,
                    &workspace_repository,
                )
                .await
        } else {
            self.tool_dispatcher
                .dispatch(run_id, call, session, profile)
                .await
        };

        match dispatch_result {
            Ok(outcome) => {
                let outcome = match outcome.effect.clone() {
                    AgentToolEffect::Finish => {
                        if exit_policy == AgentInvocationExitPolicy::TaskReturnRequired {
                            recoverable_tool_error(
                                call,
                                "agent.child_finish_denied",
                                "Return-mode child Agent invocations must complete with task.return, not workspace.finish.",
                                outcome.elapsed_ms,
                            )
                        } else if profile.run.presentation == AgentRunPresentation::Foreground
                            && commit_count == 0
                        {
                            recoverable_tool_error(
                                call,
                                "agent.foreground_commit_required",
                                "Foreground Agent runs must call workspace.commit successfully before workspace.finish.",
                                outcome.elapsed_ms,
                            )
                        } else {
                            if self.has_pending_child_tasks(run_id, invocation_id).await? {
                                self.active_run_handle(run_id)
                                    .await?
                                    .scheduler
                                    .cancel_unfinished_for_parent(invocation_id)
                                    .await?;
                            }
                            outcome
                        }
                    }
                    AgentToolEffect::ChatCommitRequested { path, mode, reason } => {
                        self.perform_host_chat_commit(
                            run_id,
                            call,
                            path,
                            mode,
                            reason,
                            outcome.elapsed_ms,
                            round,
                            invocation_id,
                            commit_ledger,
                            cancel,
                        )
                        .await?
                    }
                    _ => outcome,
                };
                let outcome = if let Some(view) = child_workspace_view.as_ref() {
                    view.physicalize_outcome_effect(outcome)?
                } else {
                    outcome
                };
                self.record_tool_outcome(run_id, invocation_id, round, &outcome)
                    .await?;
                Ok(outcome)
            }
            Err(error) => {
                self.event(
                    run_id,
                    AgentRunEventLevel::Error,
                    "tool_call_failed",
                    json!({
                    "round": round,
                    "invocationId": invocation_id,
                    "callId": call.id.as_str(),
                    "name": call.name.as_str(),
                    "message": error.to_string(),
                    }),
                )
                .await?;
                Err(error)
            }
        }
    }

    async fn record_tool_outcome(
        &self,
        run_id: &str,
        invocation_id: &str,
        round: usize,
        outcome: &AgentToolDispatchOutcome,
    ) -> Result<(), ApplicationError> {
        self.store_tool_result(run_id, round, &outcome.result)
            .await?;
        self.event(
            run_id,
            if outcome.result.is_error {
                AgentRunEventLevel::Warn
            } else {
                AgentRunEventLevel::Info
            },
            if outcome.result.is_error {
                "tool_call_failed"
            } else {
                "tool_call_completed"
            },
            json!({
                "round": round,
                "invocationId": invocation_id,
                "callId": outcome.result.call_id.as_str(),
                "name": outcome.result.name.as_str(),
                "isError": outcome.result.is_error,
                "errorCode": outcome.result.error_code.as_deref(),
                "message": outcome.result.is_error.then_some(outcome.result.content.as_str()),
                "elapsedMs": outcome.elapsed_ms,
                "resourceRefs": &outcome.result.resource_refs,
            }),
        )
        .await?;
        Ok(())
    }

    async fn store_tool_result(
        &self,
        run_id: &str,
        round: usize,
        result: &AgentToolResult,
    ) -> Result<(), ApplicationError> {
        let path = WorkspacePath::parse(format!(
            "tool-results/{}.json",
            tool_call_audit_file_stem(&result.call_id)
        ))?;
        let text = serde_json::to_string_pretty(result).map_err(|error| {
            ApplicationError::ValidationError(format!(
                "agent.tool_result_serialize_failed: {error}"
            ))
        })?;
        self.workspace_repository
            .write_text(run_id, &path, &text)
            .await?;
        self.event(
            run_id,
            AgentRunEventLevel::Debug,
            "tool_result_stored",
            json!({
                "round": round,
                "callId": result.call_id.as_str(),
                "path": path.as_str(),
            }),
        )
        .await?;
        Ok(())
    }

    async fn store_tool_arguments(
        &self,
        run_id: &str,
        call: &AgentToolCall,
    ) -> Result<WorkspacePath, ApplicationError> {
        let path = WorkspacePath::parse(format!(
            "tool-args/{}.json",
            tool_call_audit_file_stem(&call.id)
        ))?;
        let text = serde_json::to_string_pretty(&call.arguments).map_err(|error| {
            ApplicationError::ValidationError(format!(
                "agent.tool_arguments_serialize_failed: {error}"
            ))
        })?;
        self.workspace_repository
            .write_text(run_id, &path, &text)
            .await?;
        Ok(path)
    }
}

fn tool_call_audit_file_stem(call_id: &str) -> String {
    let digest = Sha256::digest(call_id.as_bytes());
    format!(
        "call_{}",
        hex_encode(&digest[..TOOL_CALL_AUDIT_DIGEST_BYTES])
    )
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn tool_is_visible(
    profile: &ResolvedAgentProfile,
    name: &str,
    exit_policy: AgentInvocationExitPolicy,
) -> bool {
    if exit_policy == AgentInvocationExitPolicy::TaskReturnRequired {
        if name == TASK_RETURN {
            return true;
        }
        if name == "workspace.commit"
            || name == "workspace.finish"
            || name == AGENT_LIST
            || name == AGENT_DELEGATE
            || name == AGENT_AWAIT
        {
            return false;
        }
    }
    profile.tools.allow.iter().any(|allowed| allowed == name)
        && !profile.tools.deny.iter().any(|denied| denied == name)
}

fn recoverable_tool_error(
    call: &AgentToolCall,
    code: &str,
    message: &str,
    elapsed_ms: u128,
) -> AgentToolDispatchOutcome {
    AgentToolDispatchOutcome {
        result: AgentToolResult {
            call_id: call.id.clone(),
            name: call.name.clone(),
            content: message.to_string(),
            structured: json!({
                "error": {
                    "code": code,
                    "message": message,
                }
            }),
            is_error: true,
            error_code: Some(code.to_string()),
            resource_refs: Vec::new(),
        },
        effect: AgentToolEffect::None,
        elapsed_ms,
    }
}
