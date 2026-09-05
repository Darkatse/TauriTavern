use serde_json::{Map, Value, json};

use crate::dto::chat_completion_dto::ChatCompletionGenerateRequestDto;
use crate::errors::ApplicationError;
use crate::services::chat_completion_service::OPENCODE_STABLE_CHAT_ID_FIELD;
use tt_domain::models::agent::profile::{AgentContextPolicy, ResolvedAgentProfile};
use tt_domain::models::agent::{
    AgentModelContentPart, AgentModelMessage, AgentModelRequest, AgentModelRole, AgentModelTool,
};
use tt_domain::models::tool::ToolChoice;
use tt_ports::repositories::chat_completion_repository::OPENAI_RESPONSES_WEBSOCKET_TRANSPORT;

use super::invocation::model_session_id;

const AGENT_PROMPT_MARKER_FIELD: &str = "_tauritavern_agent_prompt_marker";

pub(super) fn request_from_prompt_snapshot(
    prompt_snapshot: &Value,
) -> Result<ChatCompletionGenerateRequestDto, ApplicationError> {
    let mut payload = find_payload_object(prompt_snapshot).ok_or_else(|| {
        ApplicationError::ValidationError(
            "agent.invalid_prompt_snapshot: expected a chat completion payload object".to_string(),
        )
    })?;

    payload.insert("stream".to_string(), Value::Bool(false));
    if !payload.contains_key("chat_completion_source") {
        payload.insert(
            "chat_completion_source".to_string(),
            Value::String("openai".to_string()),
        );
    }

    if !payload.contains_key("messages") && !payload.contains_key("prompt") {
        return Err(ApplicationError::ValidationError(
            "agent.invalid_prompt_snapshot: payload must contain messages or prompt".to_string(),
        ));
    }

    Ok(ChatCompletionGenerateRequestDto { payload })
}

pub(super) fn prepare_agent_tool_request(
    mut request: ChatCompletionGenerateRequestDto,
    tools: &[AgentModelTool],
    tool_choice: ToolChoice,
    stable_chat_id: &str,
    run_id: &str,
    invocation_id: &str,
) -> Result<AgentModelRequest, ApplicationError> {
    reject_external_tool_request(&request.payload)?;

    let responses_websocket = match request.payload.get("custom_openai_responses_websocket") {
        Some(Value::Bool(enabled)) => *enabled,
        Some(_) => {
            return Err(ApplicationError::ValidationError(
                "agent.responses_websocket_invalid: custom_openai_responses_websocket must be boolean"
                    .to_string(),
            ));
        }
        None => false,
    };
    if responses_websocket
        && (request
            .payload
            .get("chat_completion_source")
            .and_then(Value::as_str)
            != Some("custom")
            || request
                .payload
                .get("custom_api_format")
                .and_then(Value::as_str)
                != Some("openai_responses"))
    {
        return Err(ApplicationError::ValidationError(
            "agent.responses_websocket_format_mismatch: Responses WebSocket mode requires chat_completion_source=custom and custom_api_format=openai_responses"
                .to_string(),
        ));
    }

    let messages = messages_from_payload(&mut request.payload)?;

    request.payload.remove("tools");
    request.payload.remove("tool_choice");
    request
        .payload
        .insert("stream".to_string(), Value::Bool(false));
    if request
        .payload
        .get("chat_completion_source")
        .and_then(Value::as_str)
        == Some("opencode")
    {
        request.payload.insert(
            OPENCODE_STABLE_CHAT_ID_FIELD.to_string(),
            Value::String(stable_chat_id.to_string()),
        );
    }

    let mut provider_state = json!({
        "sessionId": model_session_id(run_id, invocation_id),
        "runId": run_id,
        "invocationId": invocation_id,
    });
    if responses_websocket {
        provider_state["transport"] =
            Value::String(OPENAI_RESPONSES_WEBSOCKET_TRANSPORT.to_string());
    }

    Ok(AgentModelRequest {
        payload: request.payload,
        messages,
        tools: tools.to_vec(),
        tool_choice,
        provider_state,
    })
}

pub(super) fn validate_prompt_snapshot_context_policy(
    prompt_snapshot: &Value,
    profile: &ResolvedAgentProfile,
) -> Result<(), ApplicationError> {
    let snapshot_policy_value = prompt_snapshot
        .as_object()
        .and_then(|object| object.get("contextPolicy"))
        .ok_or_else(|| {
            ApplicationError::ValidationError(
                "agent.context_policy_required: prompt snapshot must include contextPolicy"
                    .to_string(),
            )
        })?;
    let snapshot_policy = serde_json::from_value::<AgentContextPolicy>(
        snapshot_policy_value.clone(),
    )
    .map_err(|error| {
        ApplicationError::ValidationError(format!(
            "agent.invalid_context_policy_snapshot: contextPolicy is invalid: {error}"
        ))
    })?;

    if snapshot_policy != profile.context {
        return Err(ApplicationError::ValidationError(
            "agent.context_policy_mismatch: prompt snapshot contextPolicy does not match resolved Agent profile"
                .to_string(),
        ));
    }

    Ok(())
}

pub(super) fn reject_external_tool_request(
    payload: &Map<String, Value>,
) -> Result<(), ApplicationError> {
    let has_tools = payload
        .get("tools")
        .and_then(Value::as_array)
        .is_some_and(|tools| !tools.is_empty());
    if has_tools {
        return Err(ApplicationError::ValidationError(
            "agent.external_tools_unsupported: Agent runtime owns the tool registry".to_string(),
        ));
    }

    if payload.contains_key("tool_choice") {
        return Err(ApplicationError::ValidationError(
            "agent.external_tool_choice_unsupported: Agent runtime owns tool choice".to_string(),
        ));
    }

    if payload
        .get("messages")
        .and_then(Value::as_array)
        .is_some_and(|messages| {
            messages.iter().any(|message| {
                message
                    .get("role")
                    .and_then(Value::as_str)
                    .is_some_and(|role| role.eq_ignore_ascii_case("tool"))
                    || message
                        .pointer("/tool_calls")
                        .and_then(Value::as_array)
                        .is_some_and(|tool_calls| !tool_calls.is_empty())
            })
        })
    {
        return Err(ApplicationError::ValidationError(
            "agent.external_tool_turns_unsupported: prompt snapshot already contains tool turns"
                .to_string(),
        ));
    }

    Ok(())
}

pub(super) fn request_summary(request: &AgentModelRequest) -> Value {
    json!({
        "chatCompletionSource": request.payload.get("chat_completion_source").and_then(Value::as_str),
        "customApiFormat": request.payload.get("custom_api_format").and_then(Value::as_str),
        "model": request.payload.get("model").and_then(Value::as_str),
        "messageCount": request.messages.len(),
        "toolCount": request.tools.len(),
    })
}

fn find_payload_object(value: &Value) -> Option<Map<String, Value>> {
    let object = value.as_object()?;

    for key in [
        "chatCompletionPayload",
        "chat_completion_payload",
        "generateData",
        "generate_data",
    ] {
        if let Some(payload) = object.get(key).and_then(Value::as_object) {
            return Some(payload.clone());
        }
    }

    if object.contains_key("messages") || object.contains_key("prompt") {
        return Some(object.clone());
    }

    None
}

fn messages_from_payload(
    payload: &mut Map<String, Value>,
) -> Result<Vec<AgentModelMessage>, ApplicationError> {
    let messages = match payload.remove("messages") {
        Some(Value::Array(messages)) => messages,
        Some(Value::String(prompt)) => vec![json!({
            "role": "user",
            "content": prompt,
        })],
        Some(_) => {
            return Err(ApplicationError::ValidationError(
                "agent.tool_loop_requires_messages: messages must be an array".to_string(),
            ));
        }
        None => {
            let prompt = payload
                .remove("prompt")
                .and_then(|value| value.as_str().map(str::to_string))
                .ok_or_else(|| {
                    ApplicationError::ValidationError(
                        "agent.tool_loop_requires_messages: prompt snapshot must contain messages or a string prompt"
                            .to_string(),
                    )
                })?;
            vec![json!({
                "role": "user",
                "content": prompt,
            })]
        }
    };
    payload.remove("prompt");

    messages
        .into_iter()
        .map(message_from_openai_value)
        .collect::<Result<Vec<_>, _>>()
}

fn reject_agent_prompt_marker(value: &Value) -> Result<(), ApplicationError> {
    let Some(marker) = value
        .as_object()
        .and_then(|object| object.get(AGENT_PROMPT_MARKER_FIELD))
    else {
        return Ok(());
    };

    if !marker.is_string() {
        return Err(ApplicationError::ValidationError(
            "agent.invalid_prompt_marker: prompt marker must be a string".to_string(),
        ));
    }

    Err(ApplicationError::ValidationError(
        "agent.prompt_marker_unmaterialized: prompt snapshot must materialize agentSystemPrompt before entering the Agent runtime".to_string(),
    ))
}

fn message_from_openai_value(value: Value) -> Result<AgentModelMessage, ApplicationError> {
    reject_agent_prompt_marker(&value)?;
    let mut object = match value {
        Value::Object(object) => object,
        _ => {
            return Err(ApplicationError::ValidationError(
                "agent.invalid_prompt_snapshot: message must be an object".to_string(),
            ));
        }
    };
    let role = match object
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or("user")
        .trim()
        .to_lowercase()
        .as_str()
    {
        "system" => AgentModelRole::System,
        "developer" => AgentModelRole::Developer,
        "assistant" => AgentModelRole::Assistant,
        "tool" | "function" => AgentModelRole::Tool,
        _ => AgentModelRole::User,
    };

    let provider_metadata = json!({
        "openai": {
            "name": object.get("name").and_then(Value::as_str),
        }
    });

    Ok(AgentModelMessage {
        role,
        parts: content_parts_from_openai_value(object.remove("content")),
        provider_metadata,
    })
}

fn content_parts_from_openai_value(value: Option<Value>) -> Vec<AgentModelContentPart> {
    match value {
        Some(Value::String(text)) => vec![AgentModelContentPart::Text { text }],
        Some(Value::Array(parts)) => parts
            .into_iter()
            .map(|part| match part {
                Value::String(text) => AgentModelContentPart::Text { text },
                Value::Object(mut object)
                    if object.get("type").and_then(Value::as_str) == Some("text") =>
                {
                    AgentModelContentPart::Text {
                        text: match object.remove("text") {
                            Some(Value::String(text)) => text,
                            _ => String::new(),
                        },
                    }
                }
                other => AgentModelContentPart::Native {
                    provider: "openai.content_part".to_string(),
                    value: other,
                },
            })
            .collect(),
        Some(Value::Null) | None => Vec::new(),
        Some(other) => vec![AgentModelContentPart::Text {
            text: other.to_string(),
        }],
    }
}

/// Compile captured values once per invocation; readers share the immutable table.
pub(super) fn frozen_macros_from_snapshot(
    snapshot: &Value,
) -> Result<std::sync::Arc<tt_domain::frozen_macros::FrozenMacros>, ApplicationError> {
    let macros = snapshot
        .pointer("/frozenRunInputSnapshot/macroContext")
        .map(|context| {
            tt_domain::frozen_macros::FrozenMacros::from_context(
                context,
                snapshot.pointer("/frozenRunInputSnapshot/promptInputs/extensionPrompts"),
            )
        })
        .transpose()?
        .unwrap_or_default();
    Ok(std::sync::Arc::new(macros))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        OPENCODE_STABLE_CHAT_ID_FIELD, prepare_agent_tool_request, reject_external_tool_request,
        request_from_prompt_snapshot, validate_prompt_snapshot_context_policy,
    };
    use tt_domain::models::agent::profile::ResolvedAgentProfile;
    use tt_domain::models::tool::ToolChoice;

    #[test]
    fn rejects_external_tool_choice_even_when_null() {
        let prompt_snapshot = json!({
            "chatCompletionPayload": {
                "messages": [{ "role": "user", "content": "hello" }],
                "tool_choice": null
            }
        });
        let request = request_from_prompt_snapshot(&prompt_snapshot).expect("request");

        let error = reject_external_tool_request(&request.payload).expect_err("tool_choice fails");
        assert!(
            error
                .to_string()
                .contains("agent.external_tool_choice_unsupported")
        );
    }

    #[test]
    fn responses_websocket_mode_is_an_explicit_agent_transport() {
        let request = request_from_prompt_snapshot(&json!({
            "chatCompletionPayload": {
                "chat_completion_source": "custom",
                "custom_api_format": "openai_responses",
                "custom_openai_responses_websocket": true,
                "messages": [{ "role": "user", "content": "hello" }]
            }
        }))
        .expect("request");

        let request = prepare_agent_tool_request(
            request,
            &[],
            ToolChoice::Auto,
            "stable",
            "run_test",
            "inv_root",
        )
        .expect("agent request");

        assert_eq!(
            request.provider_state["transport"],
            json!("responses_websocket")
        );
    }

    #[test]
    fn opencode_agent_uses_the_run_chat_identity() {
        let request = request_from_prompt_snapshot(&json!({
            "chatCompletionPayload": {
                "chat_completion_source": "opencode",
                "messages": [{ "role": "user", "content": "hello" }]
            }
        }))
        .expect("request");

        let request = prepare_agent_tool_request(
            request,
            &[],
            ToolChoice::Auto,
            "stable-chat",
            "run_test",
            "inv_root",
        )
        .expect("agent request");

        assert_eq!(
            request.payload[OPENCODE_STABLE_CHAT_ID_FIELD],
            json!("stable-chat")
        );
    }

    #[test]
    fn responses_websocket_mode_rejects_other_provider_formats() {
        let request = request_from_prompt_snapshot(&json!({
            "chatCompletionPayload": {
                "chat_completion_source": "custom",
                "custom_api_format": "openai_compat",
                "custom_openai_responses_websocket": true,
                "messages": [{ "role": "user", "content": "hello" }]
            }
        }))
        .expect("request");

        let error = prepare_agent_tool_request(
            request,
            &[],
            ToolChoice::Auto,
            "stable",
            "run_test",
            "inv_root",
        )
        .expect_err("format mismatch must fail");
        assert!(
            error
                .to_string()
                .contains("responses_websocket_format_mismatch")
        );
    }

    #[test]
    fn internal_agent_prompt_marker_is_rejected() {
        let request = request_from_prompt_snapshot(&json!({
            "chatCompletionPayload": {
                "messages": [
                    agent_system_marker(),
                    { "role": "user", "content": "hello" }
                ]
            }
        }))
        .expect("request");

        let error = prepare_agent_tool_request(
            request,
            &[],
            ToolChoice::Auto,
            "stable",
            "run_test",
            "inv_root",
        )
        .expect_err("marker leak fails");

        assert!(
            error
                .to_string()
                .contains("agent.prompt_marker_unmaterialized")
        );
    }

    #[test]
    fn context_policy_must_match_resolved_profile() {
        let profile = test_profile(None);
        let prompt_snapshot = json!({
            "contextPolicy": {
                "initialChatHistoryMessages": 8,
                "includeActivatedWorldInfo": true
            },
            "chatCompletionPayload": {
                "messages": [{ "role": "system", "content": "Materialized Agent System Prompt." }]
            }
        });

        let error = validate_prompt_snapshot_context_policy(&prompt_snapshot, &profile)
            .expect_err("context policy mismatch fails");

        assert!(error.to_string().contains("agent.context_policy_mismatch"));
    }

    #[test]
    fn context_policy_is_required_for_agent_run_start() {
        let profile = test_profile(None);
        let prompt_snapshot = json!({
            "chatCompletionPayload": {
                "messages": [{ "role": "system", "content": "Materialized Agent System Prompt." }]
            }
        });

        let error = validate_prompt_snapshot_context_policy(&prompt_snapshot, &profile)
            .expect_err("missing context policy fails");

        assert!(error.to_string().contains("agent.context_policy_required"));
    }

    #[test]
    fn truncated_context_policy_does_not_change_tool_history_source() {
        let mut profile = test_profile(None);
        profile.context.initial_chat_history_messages = 8;
        let prompt_snapshot = json!({
            "contextPolicy": {
                "initialChatHistoryMessages": 8,
                "includeActivatedWorldInfo": true
            },
            "chatCompletionPayload": {
                "messages": [{ "role": "system", "content": "Materialized Agent System Prompt." }]
            }
        });

        validate_prompt_snapshot_context_policy(&prompt_snapshot, &profile)
            .expect("matching truncated context policy should pass");
    }

    fn agent_system_marker() -> serde_json::Value {
        json!({
            "role": "system",
            "content": "[marker]",
            "_tauritavern_agent_prompt_marker": "agentSystemPrompt"
        })
    }

    fn test_profile(agent_system_prompt: Option<&str>) -> ResolvedAgentProfile {
        let instructions = match agent_system_prompt {
            Some(prompt) => json!({ "agentSystemPrompt": prompt }),
            None => json!({}),
        };

        serde_json::from_value(json!({
            "schemaVersion": 3,
            "kind": "tauritavern.agentProfile",
            "id": "test",
            "displayName": "Test",
            "preset": {
                "mode": "none",
                "required": false
            },
            "model": {
                "mode": "currentPromptSnapshot"
            },
            "run": {
                "presentation": "background"
            },
            "instructions": instructions,
            "tools": {
                "allow": [
                    "builtin:workspace.write_file",
                    "builtin:workspace.commit",
                    "builtin:workspace.finish"
                ],
                "deny": [],
                "toolDescriptions": {},
                "maxRounds": 1,
                "maxCallsPerRun": 1,
                "maxCallsPerTool": {}
            },
            "skills": {
                "visible": ["*"],
                "deny": [],
                "maxReadCharsPerCall": 1,
                "maxReadCharsPerRun": 1
            },
            "workspace": {
                "visibleRoots": ["output"],
                "writableRoots": ["output"]
            },
            "plan": {
                "mode": "none",
                "beta": true,
                "nodes": []
            },
            "output": {
                "artifacts": [{
                    "id": "main",
                    "path": "output/main.md",
                    "kind": "markdown",
                    "target": "message_body",
                    "required": true,
                    "assemblyOrder": 0
                }],
                "messageBodyArtifactId": "main",
                "messageBodyPath": "output/main.md"
            },
            "sourceTrace": {
                "profileSource": "test"
            }
        }))
        .expect("test profile")
    }
}
