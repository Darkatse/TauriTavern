use std::time::Instant;

use serde_json::{Map, Value, json};
use tokio_util::sync::CancellationToken;

use super::commit_ledger::RunCommitLedger;
use super::model_stream_projection::remove_live_tool_call;
use super::tool_results::{mcp_known_response_result, tool_call_audit_file_stem};
use super::{AgentRuntimeService, PreparedInvocation};
use crate::errors::ApplicationError;
use crate::services::tool_request_gate::{ToolRequestGate, ToolRequestGateError};

use crate::services::agent_tools::{
    AGENT_AWAIT, AGENT_DELEGATE, AGENT_HANDOFF, AgentToolDispatchOutcome, AgentToolEffect,
    AgentToolSession, TASK_RETURN, WORKSPACE_FINISH, WORKSPACE_SHELL,
};
use tt_domain::models::agent::{
    AgentInvocationExitPolicy, AgentRunEventLevel, AgentRunPresentation, AgentRunStatus,
    AgentToolResult, WorkspacePath,
};
use tt_domain::models::tool::{ToolArguments, ToolInvocation};
use tt_ports::mcp::McpCallOutcome;
use tt_ports::workspace_fs::WorkspaceWriteGuard;

pub(super) struct ToolCallFailure {
    pub error: ApplicationError,
    pub started: bool,
}

impl AgentRuntimeService {
    #[expect(
        clippy::too_many_arguments,
        reason = "tool dispatch boundary keeps invocation, call position, session, ledger, and cancellation explicit"
    )]
    pub(super) async fn dispatch_tool_call(
        &self,
        prepared: &PreparedInvocation,
        round: usize,
        tool_call_index: usize,
        tool_invocation: &ToolInvocation,
        gate: &mut ToolRequestGate,
        session: &mut AgentToolSession,
        is_last_call: bool,
        commit_ledger: &mut RunCommitLedger,
        cancel: &mut super::AgentCancelReceiver,
        auto_commit_candidate: Option<WorkspacePath>,
    ) -> Result<AgentToolDispatchOutcome, ToolCallFailure> {
        let mut started_tool = false;
        let result = async {
            let run_id = prepared.invocation.run_id.as_str();
            let invocation_id = prepared.invocation.id.as_str();
            let exit_policy = prepared.invocation.exit_policy;
            let profile = &prepared.profile;
            let tool_name = tool_invocation.tool_id.native_name();
            let snapshot_id = prepared.tool_snapshot.id().as_str();
            let arguments_ref = self.store_tool_arguments(run_id, invocation_id, round, tool_invocation).await?;
            let mut payload = json!({
                "round": round,
                "invocationId": invocation_id,
                "callId": tool_invocation.call_id.as_str(),
                "toolId": tool_invocation.tool_id.as_str(),
                "snapshotId": snapshot_id,
                "name": tool_name,
                "argumentsRef": arguments_ref.as_str(),
            });
            if tool_invocation.tool_id.is_builtin()
                && tool_name == WORKSPACE_SHELL
                && let ToolArguments::Object(args) = &tool_invocation.arguments
                && let Some(command) = args.get("command")
            {
                // Keep the command as input data; Timeline owns its presentation.
                payload["command"] = command.clone();
            }
            self.event(
                run_id,
                AgentRunEventLevel::Info,
                "tool_call_requested",
                payload,
            )
            .await?;
            let active_run = self.active_run_handle(run_id).await?;
            remove_live_tool_call(&active_run.live_projection, invocation_id, tool_call_index);
            let started = Instant::now();

            if let Err(rejection) = gate.authorize_and_reserve(
                &prepared.tool_snapshot,
                &prepared.tool_turn,
                tool_invocation,
            ) {
                let budget_message = match &rejection {
                    ToolRequestGateError::InvocationBudgetExhausted { max_calls } => Some(format!(
                        "The tool call limit for this task has been reached ({max_calls} calls)."
                    )),
                    ToolRequestGateError::ToolBudgetExhausted { max_calls, .. } => Some(format!(
                        "`{tool_name}` has reached its call limit for this task ({max_calls} calls)."
                    )),
                    _ => None,
                };
                if let Some(message) = budget_message {
                    let outcome = recoverable_tool_error(
                        tool_invocation,
                        "agent.tool_budget_exhausted",
                        &message,
                        started.elapsed().as_millis(),
                    );
                    return Ok(outcome);
                }

                let error = if matches!(
                    &rejection,
                    ToolRequestGateError::TurnSnapshotMismatch { .. }
                ) {
                    ApplicationError::InternalError(rejection.to_string())
                } else {
                    ApplicationError::ValidationError(rejection.to_string())
                };
                self.event(
                    run_id,
                    AgentRunEventLevel::Error,
                    "tool_call_failed",
                    json!({
                        "round": round,
                        "invocationId": invocation_id,
                        "callId": tool_invocation.call_id.as_str(),
                        "toolId": tool_invocation.tool_id.as_str(),
                        "snapshotId": snapshot_id,
                        "name": tool_name,
                        "message": error.to_string(),
                    }),
                )
                .await?;
                return Err(error);
            }

            // Charge the budget before rejecting arguments so malformed calls cannot retry for free.
            let args = match tool_invocation.arguments.as_map() {
                Ok(args) => args,
                Err(message) => {
                    return Ok(recoverable_tool_error(
                        tool_invocation,
                        "tool.invalid_arguments",
                        &message,
                        started.elapsed().as_millis(),
                    ));
                }
            };

            let call = tool_invocation;
            if prepared.invocation.kind.owns_run_status() {
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
                    "callId": call.call_id.as_str(),
                    "toolId": tool_invocation.tool_id.as_str(),
                    "snapshotId": snapshot_id,
                    "name": tool_name,
                }),
            )
            .await?;

            let builtin_name = call.tool_id.is_builtin().then_some(tool_name);
            started_tool = true;
            let dispatch_result = if !is_last_call && builtin_name.is_some_and(is_completion_tool) {
                Ok(recoverable_tool_error(
                    call,
                    "agent.tool_after_finish",
                    &format!(
                        "{} must be the final tool call in a model turn; complete the other work first, then call it again.",
                        call.tool_id.native_name()
                    ),
                    started.elapsed().as_millis(),
                ))
            } else if builtin_name == Some(AGENT_DELEGATE) {
                Box::pin(self.dispatch_agent_delegate_tool(
                    run_id,
                    invocation_id,
                    call,
                    args,
                    profile,
                    cancel,
                ))
                .await
            } else if builtin_name == Some(AGENT_AWAIT) {
                self.dispatch_agent_await_tool(
                    prepared,
                    call,
                    args,
                    commit_ledger.explicit_count(),
                    cancel,
                )
                .await
            } else if builtin_name == Some(AGENT_HANDOFF) {
                self.dispatch_agent_handoff_tool(run_id, invocation_id, call, args, profile)
                    .await
            } else if builtin_name == Some(TASK_RETURN) {
                self.dispatch_task_return_tool(
                    run_id,
                    invocation_id,
                    call,
                    args,
                    exit_policy,
                    profile,
                )
                .await
            } else if call.tool_id.extension_id().is_some() {
                let reply = self.extension_tools.call(
                    tt_contracts::extension_tools::ExtensionToolCall {
                        tool_id: call.tool_id.clone(),
                        run_id: run_id.to_string(),
                        invocation_id: invocation_id.to_string(),
                        call_id: call.call_id.clone(),
                        target: (&active_run.target).into(),
                        arguments: args.clone(),
                    },
                    cancel.clone(),
                ).await.map_err(ApplicationError::from);
                reply.map(|reply| AgentToolDispatchOutcome {
                    result: super::tool_results::extension_reply_result(call, reply),
                    effect: AgentToolEffect::None,
                    elapsed_ms: started.elapsed().as_millis(),
                })
            } else if call.tool_id.provider_id().starts_with("mcp/") {
                // The MCP service reports sent-but-unconfirmed calls explicitly.
                started_tool = false;
                let outcome = self.call_mcp_tool(call, args, cancel).await?;
                started_tool = true;
                match outcome {
                    McpCallOutcome::KnownResponse(response) => Ok(AgentToolDispatchOutcome {
                        result: mcp_known_response_result(call, response),
                        effect: AgentToolEffect::None,
                        elapsed_ms: started.elapsed().as_millis(),
                    }),
                    McpCallOutcome::NotSent(issue) => Ok(recoverable_tool_error(
                        call,
                        issue.code.as_str(),
                        issue.message.as_str(),
                        started.elapsed().as_millis(),
                    )),
                    McpCallOutcome::OutcomeUnknown(issue) => {
                        let message = format!(
                            "mcp.call_outcome_unknown: {} The MCP tool may have executed; this call will not be retried.",
                            issue.message
                        );
                        let outcome = recoverable_tool_error(
                            call,
                            "mcp.call_outcome_unknown",
                            &message,
                            started.elapsed().as_millis(),
                        );
                        let _ = self
                            .record_tool_outcome(run_id, invocation_id, round, snapshot_id, &outcome)
                            .await?;
                        return Err(if *cancel.borrow() {
                            ApplicationError::Cancelled(message)
                        } else {
                            ApplicationError::ValidationError(message)
                        });
                    }
                }
            } else {
                self.tool_dispatcher
                    .dispatch(run_id, call, args, session, profile, active_run.files.clone(), cancel.clone(), auto_commit_candidate)
                    .await
            };

            match dispatch_result {
                Ok(outcome) => {
                    ensure_tool_result_identity(tool_invocation, &outcome.result)?;
                    let outcome = match outcome.effect.clone() {
                        AgentToolEffect::Finish => {
                            if exit_policy == AgentInvocationExitPolicy::TaskReturnRequired {
                                recoverable_tool_error(
                                    tool_invocation,
                                    "agent.child_finish_denied",
                                    "Return-mode child Agent invocations must complete with task.return, not workspace.finish.",
                                    outcome.elapsed_ms,
                                )
                            } else if !commit_ledger.has_explicit_commit()
                                && self.run_repository.load_run(run_id).await?.chat_target()?.presentation
                                    == AgentRunPresentation::Foreground
                            {
                                recoverable_tool_error(
                                    tool_invocation,
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
                            self.perform_explicit_host_chat_commit(
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
                            "callId": call.call_id.as_str(),
                            "toolId": tool_invocation.tool_id.as_str(),
                            "snapshotId": snapshot_id,
                            "name": tool_name,
                            "message": error.to_string(),
                        }),
                    )
                    .await?;
                    Err(error)
                }
            }
        }
        .await;
        result.map_err(|error| ToolCallFailure {
            error,
            started: started_tool,
        })
    }

    async fn call_mcp_tool(
        &self,
        call: &ToolInvocation,
        args: &Map<String, Value>,
        cancel: &mut super::AgentCancelReceiver,
    ) -> Result<McpCallOutcome, ApplicationError> {
        let cancellation = CancellationToken::new();
        if *cancel.borrow() {
            cancellation.cancel();
        }
        let watcher = if cancellation.is_cancelled() {
            None
        } else {
            let cancellation = cancellation.clone();
            let mut receiver = cancel.clone();
            Some(tokio::spawn(async move {
                if receiver.changed().await.is_ok() && *receiver.borrow() {
                    cancellation.cancel();
                }
            }))
        };
        let outcome = self
            .mcp_service
            .call_permitted_tool(&call.tool_id, Value::Object(args.clone()), cancellation)
            .await;
        if let Some(watcher) = watcher {
            watcher.abort();
        }

        outcome
    }

    async fn store_tool_arguments(
        &self,
        run_id: &str,
        invocation_id: &str,
        round: usize,
        call: &ToolInvocation,
    ) -> Result<WorkspacePath, ApplicationError> {
        let path = WorkspacePath::parse(format!(
            "tool-args/{invocation_id}/round-{round:03}-{}.json",
            tool_call_audit_file_stem(&call.call_id)
        ))?;
        let text = serde_json::to_string_pretty(&call.arguments).map_err(|error| {
            ApplicationError::ValidationError(format!(
                "agent.tool_arguments_serialize_failed: {error}"
            ))
        })?;
        self.workspace_files(run_id)
            .await?
            .write_text(&path, &text, WorkspaceWriteGuard::MustNotExist)
            .await?;
        Ok(path)
    }
}

fn is_completion_tool(tool_name: &str) -> bool {
    matches!(tool_name, WORKSPACE_FINISH | AGENT_HANDOFF | TASK_RETURN)
}

pub(super) fn recoverable_tool_error(
    call: &ToolInvocation,
    code: &str,
    message: &str,
    elapsed_ms: u128,
) -> AgentToolDispatchOutcome {
    AgentToolDispatchOutcome {
        result: AgentToolResult {
            call_id: call.call_id.clone(),
            tool_id: call.tool_id.clone(),
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

fn ensure_tool_result_identity(
    invocation: &ToolInvocation,
    result: &AgentToolResult,
) -> Result<(), ApplicationError> {
    if result.call_id == invocation.call_id && result.tool_id == invocation.tool_id {
        return Ok(());
    }
    Err(ApplicationError::InternalError(format!(
        "tool.result_identity_mismatch: invocation `{}` / `{}` produced result `{}` / `{}`",
        invocation.call_id, invocation.tool_id, result.call_id, result.tool_id
    )))
}

#[cfg(test)]
mod tests {
    use serde_json::Value;
    use tt_domain::models::agent::AgentToolResult;
    use tt_domain::models::tool::{ToolArguments, ToolId, ToolInvocation};

    use super::ensure_tool_result_identity;

    #[test]
    fn tool_result_identity_must_match_its_invocation() {
        let invocation = ToolInvocation {
            call_id: "call_1".to_string(),
            tool_id: ToolId::builtin("workspace.finish").unwrap(),
            arguments: ToolArguments::empty(),
            provider_metadata: Value::Null,
        };
        let result = AgentToolResult {
            call_id: invocation.call_id.clone(),
            tool_id: ToolId::builtin("workspace.commit").unwrap(),
            content: String::new(),
            structured: Value::Null,
            is_error: false,
            error_code: None,
            resource_refs: Vec::new(),
        };

        let error = ensure_tool_result_identity(&invocation, &result).unwrap_err();
        assert!(error.to_string().contains("tool.result_identity_mismatch"));
    }
}
