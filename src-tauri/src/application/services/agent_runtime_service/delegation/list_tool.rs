use std::time::Instant;

use serde::Serialize;
use serde_json::{Map, Value};

use super::policy::caller_allowed;
use super::tool_error::tool_error_outcome;
use crate::application::errors::ApplicationError;
use crate::application::services::agent_runtime_service::AgentRuntimeService;
use crate::application::services::agent_tools::{AgentToolDispatchOutcome, AgentToolEffect};
use crate::domain::models::agent::profile::ResolvedAgentProfile;
use crate::domain::models::agent::{AgentToolCall, AgentToolResult};

const DEFAULT_AGENT_LIST_LIMIT: usize = 8;
const MAX_AGENT_LIST_LIMIT: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
enum AgentListPurpose {
    Any,
    Delegate,
    Handoff,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentListStructured {
    purpose: AgentListPurpose,
    agents: Vec<AgentListItem>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentListItem {
    profile_id: String,
    display_name: String,
    description: String,
    operations: Vec<&'static str>,
    result_budget_tokens: usize,
    max_invocations_per_run: usize,
    max_concurrent_invocations: usize,
    max_handoff_depth: usize,
}

impl AgentRuntimeService {
    pub(in crate::application::services::agent_runtime_service) async fn dispatch_agent_list_tool(
        &self,
        call: &AgentToolCall,
        profile: &ResolvedAgentProfile,
    ) -> Result<AgentToolDispatchOutcome, ApplicationError> {
        let started = Instant::now();
        let Some(args) = call.arguments.as_object() else {
            return Ok(tool_error_outcome(
                call,
                "tool.invalid_arguments",
                "arguments must be an object",
                started.elapsed().as_millis(),
            ));
        };

        let purpose = match agent_list_purpose(args) {
            Ok(purpose) => purpose,
            Err(message) => {
                return Ok(tool_error_outcome(
                    call,
                    "tool.invalid_arguments",
                    &message,
                    started.elapsed().as_millis(),
                ));
            }
        };
        if let Err(message) = validate_agent_list_capability(profile, purpose) {
            return Ok(tool_error_outcome(
                call,
                "agent.delegation_policy_denied",
                &message,
                started.elapsed().as_millis(),
            ));
        }

        let query = optional_query(args);
        let limit = match agent_list_limit(args) {
            Ok(limit) => limit,
            Err(message) => {
                return Ok(tool_error_outcome(
                    call,
                    "tool.invalid_arguments",
                    &message,
                    started.elapsed().as_millis(),
                ));
            }
        };

        let profiles = self
            .profile_service
            .list_resolved_profiles(self.tool_registry.specs())
            .await?;
        let mut agents = profiles
            .into_iter()
            .filter_map(|target| {
                list_item_for_target(profile, &target, purpose)
                    .filter(|item| query_matches(query.as_deref(), item))
            })
            .take(limit)
            .collect::<Vec<_>>();
        agents.sort_by(|left, right| left.profile_id.cmp(&right.profile_id));

        let structured = AgentListStructured { purpose, agents };
        let content = render_agent_list_content(&structured);
        let structured = serde_json::to_value(structured).map_err(|error| {
            ApplicationError::ValidationError(format!(
                "agent.list_result_serialize_failed: {error}"
            ))
        })?;

        Ok(AgentToolDispatchOutcome {
            result: AgentToolResult {
                call_id: call.id.clone(),
                name: call.name.clone(),
                content,
                structured,
                is_error: false,
                error_code: None,
                resource_refs: Vec::new(),
            },
            effect: AgentToolEffect::None,
            elapsed_ms: started.elapsed().as_millis(),
        })
    }
}

impl AgentListPurpose {
    fn as_str(self) -> &'static str {
        match self {
            AgentListPurpose::Any => "any",
            AgentListPurpose::Delegate => "delegate",
            AgentListPurpose::Handoff => "handoff",
        }
    }
}

fn validate_agent_list_capability(
    profile: &ResolvedAgentProfile,
    purpose: AgentListPurpose,
) -> Result<(), String> {
    match purpose {
        AgentListPurpose::Delegate if !profile.delegation.can_delegate => Err(format!(
            "agent.profile_cannot_delegate: profile `{}` cannot delegate to subagents",
            profile.id.as_str()
        )),
        AgentListPurpose::Handoff if !profile.delegation.can_handoff => Err(format!(
            "agent.profile_cannot_handoff: profile `{}` cannot hand off control",
            profile.id.as_str()
        )),
        AgentListPurpose::Any
            if !profile.delegation.can_delegate && !profile.delegation.can_handoff =>
        {
            Err(format!(
                "agent.profile_cannot_delegate: profile `{}` cannot delegate or hand off control",
                profile.id.as_str()
            ))
        }
        _ => Ok(()),
    }
}

fn agent_list_purpose(args: &Map<String, Value>) -> Result<AgentListPurpose, String> {
    reject_unknown_args(args)?;
    let Some(value) = args.get("purpose") else {
        return Ok(AgentListPurpose::Any);
    };
    let Some(raw) = value.as_str() else {
        return Err("purpose must be a string".to_string());
    };
    match raw.trim() {
        "" | "any" => Ok(AgentListPurpose::Any),
        "delegate" => Ok(AgentListPurpose::Delegate),
        "handoff" => Ok(AgentListPurpose::Handoff),
        other => Err(format!("unsupported purpose `{other}`")),
    }
}

fn reject_unknown_args(args: &Map<String, Value>) -> Result<(), String> {
    for key in args.keys() {
        if !matches!(key.as_str(), "purpose" | "query" | "limit") {
            return Err(format!("unknown argument `{key}`"));
        }
    }
    Ok(())
}

fn optional_query(args: &Map<String, Value>) -> Option<String> {
    args.get("query")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase)
}

fn agent_list_limit(args: &Map<String, Value>) -> Result<usize, String> {
    let Some(value) = args.get("limit") else {
        return Ok(DEFAULT_AGENT_LIST_LIMIT);
    };
    let Some(limit) = value.as_u64() else {
        return Err("limit must be a non-negative integer".to_string());
    };
    let limit = usize::try_from(limit).map_err(|_| "limit is too large".to_string())?;
    if limit == 0 {
        return Err("limit must be >= 1".to_string());
    }
    if limit > MAX_AGENT_LIST_LIMIT {
        return Err(format!("limit must be <= {MAX_AGENT_LIST_LIMIT}"));
    }
    Ok(limit)
}

fn list_item_for_target(
    source: &ResolvedAgentProfile,
    target: &ResolvedAgentProfile,
    purpose: AgentListPurpose,
) -> Option<AgentListItem> {
    if !target.delegation.callable || !caller_allowed(source, target) {
        return None;
    }

    let mut operations = Vec::new();
    if source.delegation.can_delegate && target.delegation.allow_as_subagent {
        operations.push("delegate");
    }
    if source.delegation.can_handoff && target.delegation.allow_as_handoff_target {
        operations.push("handoff");
    }
    match purpose {
        AgentListPurpose::Delegate => operations.retain(|operation| *operation == "delegate"),
        AgentListPurpose::Handoff => operations.retain(|operation| *operation == "handoff"),
        AgentListPurpose::Any => {}
    }
    if operations.is_empty() {
        return None;
    }

    Some(AgentListItem {
        profile_id: target.id.as_str().to_string(),
        display_name: target.display_name.clone(),
        description: target
            .delegation
            .description_for_agents
            .as_deref()
            .or(target.description.as_deref())
            .unwrap_or("")
            .to_string(),
        operations,
        result_budget_tokens: target.delegation.result_budget_tokens,
        max_invocations_per_run: target.delegation.max_invocations_per_run,
        max_concurrent_invocations: target.delegation.max_concurrent_invocations,
        max_handoff_depth: target.delegation.max_handoff_depth,
    })
}

fn query_matches(query: Option<&str>, item: &AgentListItem) -> bool {
    let Some(query) = query else {
        return true;
    };
    item.profile_id.to_ascii_lowercase().contains(query)
        || item.display_name.to_ascii_lowercase().contains(query)
        || item.description.to_ascii_lowercase().contains(query)
}

fn render_agent_list_content(structured: &AgentListStructured) -> String {
    if structured.agents.is_empty() {
        return format!(
            "No callable Agents match purpose {}.",
            structured.purpose.as_str()
        );
    }

    let mut content = structured
        .agents
        .iter()
        .map(|agent| {
            let operations = agent.operations.join(", ");
            if agent.description.is_empty() {
                format!("- {} ({operations})", agent.profile_id)
            } else {
                format!(
                    "- {} ({operations}): {}",
                    agent.profile_id, agent.description
                )
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    content.push_str("\n\nThis is a read-only list; no Agent was started.");
    content
}
