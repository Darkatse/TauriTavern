use std::time::Instant;

use serde_json::json;
use sha2::{Digest, Sha256};

use super::commit_ledger::RunCommitLedger;
use super::delegation::workspace_policy::InvocationWorkspaceRepository;
use super::{AgentRuntimeService, PreparedInvocation};
use crate::errors::ApplicationError;
use crate::services::hashing::hex_lower;
use crate::services::tool_request_gate::{ToolRequestGate, ToolRequestGateError};

use crate::services::agent_tools::{
    AGENT_AWAIT, AGENT_DELEGATE, AGENT_HANDOFF, AGENT_LIST, AgentToolDispatchOutcome,
    AgentToolEffect, AgentToolSession, TASK_RETURN,
};
use tt_domain::models::agent::{
    AgentInvocationExitPolicy, AgentRunEventLevel, AgentRunPresentation, AgentRunStatus,
    AgentToolResult, WorkspacePath,
};
use tt_domain::models::tool::ToolInvocation;
use tt_ports::repositories::workspace_repository::WorkspaceWriteGuard;

const TOOL_CALL_AUDIT_DIGEST_BYTES: usize = 8;

impl AgentRuntimeService {
    #[expect(
        clippy::too_many_arguments,
        reason = "tool dispatch boundary keeps invocation, call position, session, ledger, and cancellation explicit"
    )]
    pub(super) async fn dispatch_tool_call(
        &self,
        prepared: &PreparedInvocation,
        round: usize,
        tool_invocation: &ToolInvocation,
        gate: &mut ToolRequestGate,
        session: &mut AgentToolSession,
        is_last_call: bool,
        commit_ledger: &mut RunCommitLedger,
        cancel: &mut super::AgentCancelReceiver,
    ) -> Result<AgentToolDispatchOutcome, ApplicationError> {
        let run_id = prepared.invocation.run_id.as_str();
        let invocation_id = prepared.invocation.id.as_str();
        let exit_policy = prepared.invocation.exit_policy;
        let profile = &prepared.profile;
        let tool_name = tool_invocation.tool_id.native_name();
        let snapshot_id = prepared.tool_snapshot.id().as_str();
        let arguments_ref = self.store_tool_arguments(run_id, tool_invocation).await?;
        self.event(
            run_id,
            AgentRunEventLevel::Info,
            "tool_call_requested",
            json!({
                "round": round,
                "invocationId": invocation_id,
                "callId": tool_invocation.call_id.as_str(),
                "toolId": tool_invocation.tool_id.as_str(),
                "snapshotId": snapshot_id,
                "name": tool_name,
                "argumentsRef": arguments_ref.as_str(),
                "providerMetadata": &tool_invocation.provider_metadata,
            }),
        )
        .await?;
        let started = Instant::now();

        if let Err(rejection) = gate.authorize_and_reserve(
            &prepared.tool_snapshot,
            &prepared.tool_turn,
            tool_invocation,
        ) {
            let budget_message = match &rejection {
                ToolRequestGateError::InvocationBudgetExhausted { max_calls } => Some(format!(
                    "Agent tool call budget is exhausted for this invocation (max {max_calls})."
                )),
                ToolRequestGateError::ToolBudgetExhausted { max_calls, .. } => Some(format!(
                    "Agent profile tool call budget for `{tool_name}` is exhausted (max {max_calls})."
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
                self.record_tool_outcome(
                    run_id,
                    invocation_id,
                    round,
                    tool_invocation,
                    snapshot_id,
                    &outcome,
                )
                .await?;
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

        let call = tool_invocation;
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
                "callId": call.call_id.as_str(),
                "toolId": tool_invocation.tool_id.as_str(),
                "snapshotId": snapshot_id,
                "name": tool_name,
            }),
        )
        .await?;

        let builtin_name = call.tool_id.is_builtin().then_some(tool_name);
        let dispatch_result = if builtin_name == Some(AGENT_LIST) {
            self.dispatch_agent_list_tool(call, profile).await
        } else if builtin_name == Some(AGENT_DELEGATE) {
            Box::pin(self.dispatch_agent_delegate_tool(
                run_id,
                invocation_id,
                call,
                profile,
                cancel,
            ))
            .await
        } else if builtin_name == Some(AGENT_AWAIT) {
            self.dispatch_agent_await_tool(prepared, call, commit_ledger.len(), cancel)
                .await
        } else if builtin_name == Some(AGENT_HANDOFF) {
            self.dispatch_agent_handoff_tool(run_id, invocation_id, call, profile, is_last_call)
                .await
        } else if builtin_name == Some(TASK_RETURN) {
            self.dispatch_task_return_tool(
                run_id,
                invocation_id,
                call,
                exit_policy,
                profile,
                is_last_call,
            )
            .await
        } else if exit_policy == AgentInvocationExitPolicy::TaskReturnRequired {
            let workspace_repository =
                InvocationWorkspaceRepository::new(self.workspace_repository.as_ref(), profile);
            self.tool_dispatcher
                .dispatch_with_model_workspace_repository(
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
                        } else if commit_ledger.is_empty()
                            && self.run_repository.load_run(run_id).await?.presentation
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
                self.record_tool_outcome(
                    run_id,
                    invocation_id,
                    round,
                    tool_invocation,
                    snapshot_id,
                    &outcome,
                )
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

    async fn record_tool_outcome(
        &self,
        run_id: &str,
        invocation_id: &str,
        round: usize,
        invocation: &ToolInvocation,
        snapshot_id: &str,
        outcome: &AgentToolDispatchOutcome,
    ) -> Result<(), ApplicationError> {
        ensure_tool_result_identity(invocation, &outcome.result)?;
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
                "toolId": outcome.result.tool_id.as_str(),
                "snapshotId": snapshot_id,
                "name": outcome.result.tool_id.native_name(),
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
            .write_text_guarded(run_id, &path, &text, WorkspaceWriteGuard::MustNotExist)
            .await?;
        self.event(
            run_id,
            AgentRunEventLevel::Debug,
            "tool_result_stored",
            json!({
                "round": round,
                "callId": result.call_id.as_str(),
                "toolId": result.tool_id.as_str(),
                "path": path.as_str(),
            }),
        )
        .await?;
        Ok(())
    }

    async fn store_tool_arguments(
        &self,
        run_id: &str,
        call: &ToolInvocation,
    ) -> Result<WorkspacePath, ApplicationError> {
        let path = WorkspacePath::parse(format!(
            "tool-args/{}.json",
            tool_call_audit_file_stem(&call.call_id)
        ))?;
        let text = serde_json::to_string_pretty(&call.arguments).map_err(|error| {
            ApplicationError::ValidationError(format!(
                "agent.tool_arguments_serialize_failed: {error}"
            ))
        })?;
        self.workspace_repository
            .write_text_guarded(run_id, &path, &text, WorkspaceWriteGuard::MustNotExist)
            .await?;
        Ok(path)
    }
}

fn tool_call_audit_file_stem(call_id: &str) -> String {
    let digest = Sha256::digest(call_id.as_bytes());
    format!(
        "call_{}",
        hex_lower(&digest[..TOOL_CALL_AUDIT_DIGEST_BYTES])
    )
}

fn recoverable_tool_error(
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
    use tt_domain::models::tool::{ToolId, ToolInvocation};

    use super::ensure_tool_result_identity;

    #[test]
    fn tool_result_identity_must_match_its_invocation() {
        let invocation = ToolInvocation {
            call_id: "call_1".to_string(),
            tool_id: ToolId::builtin("workspace.finish").unwrap(),
            arguments: Value::Null,
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
