use std::time::Instant;

use serde_json::json;
use sha2::{Digest, Sha256};

use super::AgentRuntimeService;
use super::commit_ledger::RunCommitLedger;
use crate::application::errors::ApplicationError;
use crate::application::services::agent_tools::{
    AgentToolDispatchOutcome, AgentToolEffect, AgentToolSession,
};
use crate::domain::models::agent::profile::ResolvedAgentProfile;
use crate::domain::models::agent::{
    AgentRunEventLevel, AgentRunPresentation, AgentRunStatus, AgentToolCall, AgentToolResult,
    WorkspacePath,
};

const TOOL_CALL_AUDIT_DIGEST_BYTES: usize = 8;

impl AgentRuntimeService {
    pub(super) async fn dispatch_tool_call(
        &self,
        run_id: &str,
        round: usize,
        call: &AgentToolCall,
        session: &mut AgentToolSession,
        profile: &ResolvedAgentProfile,
        commit_count: usize,
        commit_ledger: &mut RunCommitLedger,
        cancel: &mut super::AgentCancelReceiver,
    ) -> Result<AgentToolDispatchOutcome, ApplicationError> {
        let arguments_ref = self.store_tool_arguments(run_id, call).await?;
        self.event(
            run_id,
            AgentRunEventLevel::Info,
            "tool_call_requested",
            json!({
                "round": round,
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
                    "callId": call.id.as_str(),
                    "name": call.name.as_str(),
                    "message": error.to_string(),
                }),
            )
            .await?;
            return Err(error);
        }

        if !tool_is_visible(profile, call.name.as_str()) {
            let outcome = recoverable_tool_error(
                call,
                "agent.tool_policy_denied",
                &format!(
                    "Tool `{}` is not available in the current Agent profile.",
                    call.name
                ),
                started.elapsed().as_millis(),
            );
            self.record_tool_outcome(run_id, round, &outcome).await?;
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
            self.record_tool_outcome(run_id, round, &outcome).await?;
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
                self.record_tool_outcome(run_id, round, &outcome).await?;
                return Ok(outcome);
            }
        }

        session.remember_tool_call(&call.name);
        self.transition_status(run_id, AgentRunStatus::DispatchingTool)
            .await?;
        self.event(
            run_id,
            AgentRunEventLevel::Info,
            "tool_call_started",
            json!({
                "round": round,
                "callId": call.id.as_str(),
                "name": call.name.as_str(),
            }),
        )
        .await?;

        match self
            .tool_dispatcher
            .dispatch(run_id, call, session, profile)
            .await
        {
            Ok(outcome) => {
                let outcome = match outcome.effect.clone() {
                    AgentToolEffect::Finish
                        if profile.run.presentation == AgentRunPresentation::Foreground
                            && commit_count == 0 =>
                    {
                        recoverable_tool_error(
                            call,
                            "agent.foreground_commit_required",
                            "Foreground Agent runs must call workspace.commit successfully before workspace.finish.",
                            outcome.elapsed_ms,
                        )
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
                            commit_ledger,
                            cancel,
                        )
                        .await?
                    }
                    _ => outcome,
                };
                self.record_tool_outcome(run_id, round, &outcome).await?;
                Ok(outcome)
            }
            Err(error) => {
                self.event(
                    run_id,
                    AgentRunEventLevel::Error,
                    "tool_call_failed",
                    json!({
                    "round": round,
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

fn tool_is_visible(profile: &ResolvedAgentProfile, name: &str) -> bool {
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
