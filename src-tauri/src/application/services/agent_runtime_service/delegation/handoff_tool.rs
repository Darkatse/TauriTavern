use std::time::Instant;

use serde::Deserialize;
use serde_json::{Map, Value, json};
use uuid::Uuid;

use super::policy::validate_handoff_target;
use super::tool_error::tool_error_outcome;
use crate::application::errors::ApplicationError;
use crate::application::services::agent_profile_service::AgentProfileResolveInput;
use crate::application::services::agent_runtime_service::AgentRuntimeService;
use crate::application::services::agent_tools::{AgentToolDispatchOutcome, AgentToolEffect};
use crate::domain::models::agent::profile::{AgentProfileId, ResolvedAgentProfile};
use crate::domain::models::agent::{
    AgentDelegationContinuation, AgentRunEventLevel, AgentToolCall, AgentToolResult,
};

const MAX_AGENT_HANDOFF_FIELD_CHARS: usize = 8_000;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AgentHandoffArgs {
    agent_id: String,
    handoff: Value,
    #[serde(default)]
    pending_task_policy: Option<PendingTaskPolicy>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum PendingTaskPolicy {
    DenyIfPending,
}

impl AgentRuntimeService {
    pub(in crate::application::services::agent_runtime_service) async fn dispatch_agent_handoff_tool(
        &self,
        run_id: &str,
        invocation_id: &str,
        call: &AgentToolCall,
        profile: &ResolvedAgentProfile,
    ) -> Result<AgentToolDispatchOutcome, ApplicationError> {
        let started = Instant::now();
        let args = match serde_json::from_value::<AgentHandoffArgs>(call.arguments.clone()) {
            Ok(args) => args,
            Err(error) => {
                return Ok(tool_error_outcome(
                    call,
                    "tool.invalid_arguments",
                    &format!("invalid agent.handoff arguments: {error}"),
                    started.elapsed().as_millis(),
                ));
            }
        };
        if let Err(message) = validate_handoff_packet(&args.handoff) {
            return Ok(tool_error_outcome(
                call,
                "tool.invalid_arguments",
                &message,
                started.elapsed().as_millis(),
            ));
        }
        let pending_task_policy = args
            .pending_task_policy
            .unwrap_or(PendingTaskPolicy::DenyIfPending);
        if pending_task_policy != PendingTaskPolicy::DenyIfPending {
            return Ok(tool_error_outcome(
                call,
                "tool.invalid_arguments",
                "pendingTaskPolicy currently supports only denyIfPending",
                started.elapsed().as_millis(),
            ));
        }
        self.event(
            run_id,
            AgentRunEventLevel::Info,
            "agent_handoff_requested",
            json!({
                "sourceInvocationId": invocation_id,
                "targetProfileId": args.agent_id.as_str(),
                "pendingTaskPolicy": "denyIfPending",
            }),
        )
        .await?;

        if !profile.delegation.can_handoff {
            return Ok(tool_error_outcome(
                call,
                "agent.handoff_policy_denied",
                &format!(
                    "agent.profile_cannot_handoff: your current Agent configuration `{}` does not allow handoff",
                    profile.id.as_str()
                ),
                started.elapsed().as_millis(),
            ));
        }
        if self.has_pending_child_tasks(run_id, invocation_id).await? {
            return Ok(tool_error_outcome(
                call,
                "agent.handoff_pending_tasks",
                "You still have unfinished delegated tasks. Use agent.await before handing off.",
                started.elapsed().as_millis(),
            ));
        }
        if let Err(message) = self.validate_handoff_depth(run_id, profile).await? {
            return Ok(tool_error_outcome(
                call,
                "agent.handoff_depth_exhausted",
                &message,
                started.elapsed().as_millis(),
            ));
        }

        let target_id = match AgentProfileId::parse(&args.agent_id) {
            Ok(target_id) => target_id,
            Err(message) => {
                return Ok(tool_error_outcome(
                    call,
                    "tool.invalid_arguments",
                    &message,
                    started.elapsed().as_millis(),
                ));
            }
        };
        let target = match self
            .profile_service
            .resolve_profile(AgentProfileResolveInput {
                profile_id: Some(target_id.as_str()),
                known_tools: self.tool_registry.specs(),
            })
            .await
        {
            Ok(target) => target,
            Err(ApplicationError::NotFound(message)) => {
                return Ok(tool_error_outcome(
                    call,
                    "agent.target_profile_not_found",
                    &message,
                    started.elapsed().as_millis(),
                ));
            }
            Err(error) => return Err(error),
        };
        if let Err(message) = validate_handoff_target(profile, &target) {
            return Ok(tool_error_outcome(
                call,
                "agent.handoff_policy_denied",
                &message,
                started.elapsed().as_millis(),
            ));
        }

        let task_id = format!("handoff_{}", Uuid::new_v4().simple());
        let child_invocation_id = format!("inv_{}", Uuid::new_v4().simple());
        let workspace_key = self
            .allocate_child_workspace_key(run_id, target.id.as_str())
            .await?;
        let task = self
            .create_handoff_task(
                run_id,
                invocation_id,
                child_invocation_id.clone(),
                task_id.clone(),
                target.id.as_str().to_string(),
                workspace_key,
                call.id.clone(),
                args.handoff.clone(),
            )
            .await?;
        self.event(
            run_id,
            AgentRunEventLevel::Info,
            "agent_handoff_accepted",
            json!({
                "taskId": task.id.as_str(),
                "sourceInvocationId": invocation_id,
                "newInvocationId": task.child_invocation_id.as_str(),
                "targetProfileId": task.target_profile_id.as_str(),
                "workspaceKey": task.workspace_key.as_str(),
            }),
        )
        .await?;

        let structured = json!({
            "handoffId": task.id.as_str(),
            "taskId": task.id.as_str(),
            "status": "accepted",
            "agentId": target.id.as_str(),
        });
        Ok(AgentToolDispatchOutcome {
            result: AgentToolResult {
                call_id: call.id.clone(),
                name: call.name.clone(),
                content: format!(
                    "Handoff accepted for Agent {}. Your part is complete.",
                    target.id.as_str()
                ),
                structured,
                is_error: false,
                error_code: None,
                resource_refs: Vec::new(),
            },
            effect: AgentToolEffect::HandoffAccepted {
                task_id,
                new_invocation_id: child_invocation_id,
            },
            elapsed_ms: started.elapsed().as_millis(),
        })
    }

    async fn validate_handoff_depth(
        &self,
        run_id: &str,
        profile: &ResolvedAgentProfile,
    ) -> Result<Result<(), String>, ApplicationError> {
        let handoff_count = self
            .invocation_repository
            .list_tasks(run_id)
            .await?
            .into_iter()
            .filter(|task| task.continuation == AgentDelegationContinuation::TransferControl)
            .count();
        if handoff_count >= profile.delegation.max_handoff_depth {
            return Ok(Err(format!(
                "agent.max_handoff_depth_exhausted: this run has reached the handoff limit for your current Agent configuration ({})",
                profile.delegation.max_handoff_depth
            )));
        }
        Ok(Ok(()))
    }
}

fn validate_handoff_packet(handoff: &Value) -> Result<(), String> {
    let object = handoff
        .as_object()
        .ok_or_else(|| "handoff must be an object".to_string())?;
    validate_required_handoff_string(object, "objective")?;
    for key in ["title", "reason", "contextSummary"] {
        if let Some(value) = object.get(key) {
            validate_optional_handoff_string(value, key)?;
        }
    }
    Ok(())
}

fn validate_required_handoff_string(object: &Map<String, Value>, key: &str) -> Result<(), String> {
    let value = object
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("handoff.{key} must be a non-empty string"))?;
    validate_handoff_string_len(value, key)
}

fn validate_optional_handoff_string(value: &Value, key: &str) -> Result<(), String> {
    if value.is_null() {
        return Ok(());
    }
    let value = value
        .as_str()
        .ok_or_else(|| format!("handoff.{key} must be a string when provided"))?
        .trim();
    if value.is_empty() {
        return Ok(());
    }
    validate_handoff_string_len(value, key)
}

fn validate_handoff_string_len(value: &str, key: &str) -> Result<(), String> {
    if value.len() > MAX_AGENT_HANDOFF_FIELD_CHARS {
        return Err(format!(
            "handoff.{key} must be <= {MAX_AGENT_HANDOFF_FIELD_CHARS} chars"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::validate_handoff_packet;

    #[test]
    fn handoff_packet_accepts_8000_char_fields() {
        let handoff = json!({
            "title": "Revision pass",
            "objective": "a".repeat(8_000),
            "contextSummary": "Ready for the next stage."
        });

        assert!(validate_handoff_packet(&handoff).is_ok());
    }

    #[test]
    fn handoff_packet_rejects_fields_over_8000_chars() {
        let error = validate_handoff_packet(&json!({
            "title": "Revision pass",
            "objective": "a".repeat(8_001)
        }))
        .expect_err("overlong objective should fail");

        assert_eq!(error, "handoff.objective must be <= 8000 chars");
    }
}
