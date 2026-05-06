use serde_json::{Map, Value, json};

use crate::application::dto::chat_completion_dto::ChatCompletionGenerateRequestDto;
use crate::application::errors::ApplicationError;
use crate::domain::models::agent::profile::{AgentContextPolicy, ResolvedAgentProfile};
use crate::domain::models::agent::{
    AgentModelContentPart, AgentModelMessage, AgentModelRequest, AgentModelRole,
    AgentRunPresentation, AgentToolSpec,
};

const AGENT_PROMPT_MARKER_FIELD: &str = "_tauritavern_agent_prompt_marker";
const AGENT_SYSTEM_PROMPT_MARKER: &str = "agentSystemPrompt";

pub(super) fn request_from_prompt_snapshot(
    prompt_snapshot: &Value,
) -> Result<ChatCompletionGenerateRequestDto, ApplicationError> {
    let payload = find_payload_object(prompt_snapshot).ok_or_else(|| {
        ApplicationError::ValidationError(
            "agent.invalid_prompt_snapshot: expected a chat completion payload object".to_string(),
        )
    })?;
    let mut payload = payload.clone();

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
    tools: &[AgentToolSpec],
    profile: &ResolvedAgentProfile,
    run_id: &str,
) -> Result<AgentModelRequest, ApplicationError> {
    reject_external_tool_request(&request.payload)?;

    let agent_system_prompt = build_agent_system_prompt(tools, profile);
    let messages = messages_from_payload(&mut request.payload, &agent_system_prompt)?;

    request.payload.remove("tools");
    request.payload.remove("tool_choice");
    request
        .payload
        .insert("stream".to_string(), Value::Bool(false));

    Ok(AgentModelRequest {
        payload: request.payload,
        messages,
        tools: tools.to_vec(),
        tool_choice: Value::String("auto".to_string()),
        provider_state: json!({ "sessionId": run_id }),
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

fn build_agent_system_prompt(tools: &[AgentToolSpec], profile: &ResolvedAgentProfile) -> String {
    if let Some(prompt) = profile.instructions.agent_system_prompt.as_ref() {
        return prompt.clone();
    }

    let mut lines = vec![
        "TauriTavern Agent Mode is active.".to_string(),
        "Work through the available Agent tools. Tool results are private run state, not chat messages.".to_string(),
        format!(
            "Available tool function names: {}.",
            tools
                .iter()
                .map(|tool| tool.model_name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    ];

    if has_tool(tools, "chat.search") {
        lines.push(format!(
            "Use {} to find relevant prior messages when you need more context. Only query is required.",
            model_name(tools, "chat.search")
        ));
    }
    if has_tool(tools, "chat.read_messages") {
        let source_hint = if has_tool(tools, "chat.search") {
            format!("message indexes from {}", model_name(tools, "chat.search"))
        } else {
            "exact indexes you already know".to_string()
        };
        lines.push(format!(
            "Use {} with {source_hint}. For long messages, read smaller ranges with start_char and max_chars.",
            model_name(tools, "chat.read_messages")
        ));
    }
    if has_tool(tools, "worldinfo.read_activated") {
        lines.push(format!(
            "Use {} when active lore for this run matters. Call it with no arguments to list active refs, then pass entries with ref and optional start_char/max_chars to read only needed content.",
            model_name(tools, "worldinfo.read_activated")
        ));
    }
    if has_tool(tools, "skill.list") {
        lines.push(format!(
            "Use {} to discover visible Agent Skills when reusable writing, editing, planning, style, or character guidance may help.",
            model_name(tools, "skill.list")
        ));
    }
    if has_tool(tools, "skill.search") {
        lines.push(format!(
            "Use {} to locate relevant text inside large visible Skill files before reading exact ranges.",
            model_name(tools, "skill.search")
        ));
    }
    if has_tool(tools, "skill.read") {
        lines.push(format!(
            "Use {} to read SKILL.md first, then read referenced Skill files or ranges only when needed.",
            model_name(tools, "skill.read")
        ));
    }
    if has_tool(tools, "workspace.list_files") {
        lines.push(format!(
            "Use {} to inspect visible workspace files.",
            model_name(tools, "workspace.list_files")
        ));
    }
    if has_tool(tools, "workspace.search_files") {
        lines.push(format!(
            "Use {} to find relevant text in visible workspace files such as persist/ memory before reading exact ranges.",
            model_name(tools, "workspace.search_files")
        ));
    }
    if has_tool(tools, "workspace.read_file") {
        lines.push(format!(
            "Use {} before modifying an existing file. Read output has line numbers; never include line number prefixes in old_string or new_string.",
            model_name(tools, "workspace.read_file")
        ));
    }
    if has_tool(tools, "workspace.apply_patch") {
        lines.push(format!(
            "Use {} for precise edits to existing files. The old_string must match exactly and uniquely unless replace_all is true.",
            model_name(tools, "workspace.apply_patch")
        ));
    }
    if has_tool(tools, "workspace.write_file") {
        lines.push(format!(
            "Use {} for new files or complete rewrites.",
            model_name(tools, "workspace.write_file")
        ));
    }
    if has_tool(tools, "workspace.commit") {
        lines.push(format!(
            "Use {} to publish a visible workspace file to the current chat message. With no arguments it replaces the run's chat message with {}; mode append appends to the same message and creates it if this run has not committed yet.",
            model_name(tools, "workspace.commit"),
            profile.output.message_body_path
        ));
    }

    if profile
        .workspace
        .visible_roots
        .iter()
        .any(|root| root == "persist")
        && profile
            .workspace
            .writable_roots
            .iter()
            .any(|root| root == "persist")
    {
        lines.push("Use persist/ for concise information that should carry into later runs of this same chat, such as durable plot facts, unresolved threads, relationship state, and user style preferences.".to_string());
        lines.push(
            "Do not copy full chat history, final replies, tool results, or temporary reasoning into persist/."
                .to_string(),
        );
    }

    lines.push(format!(
        "Visible workspace roots: {}.",
        profile.workspace.visible_roots.join(", ")
    ));
    lines.push(format!(
        "Writable workspace roots: {}.",
        profile.workspace.writable_roots.join(", ")
    ));
    match profile.run.presentation {
        AgentRunPresentation::Foreground => lines.push(format!(
            "Before calling {}, make at least one successful {} call so the user can see the final chat message.",
            model_name(tools, "workspace.finish"),
            model_name(tools, "workspace.commit")
        )),
        AgentRunPresentation::Background => lines.push(format!(
            "Background runs may call {} without committing a chat message.",
            model_name(tools, "workspace.finish")
        )),
    }
    lines.push(format!(
        "Do not answer directly without finishing through {}.",
        model_name(tools, "workspace.finish")
    ));

    lines.join("\n")
}

fn has_tool(tools: &[AgentToolSpec], name: &str) -> bool {
    tools.iter().any(|tool| tool.name == name)
}

fn model_name<'a>(tools: &'a [AgentToolSpec], name: &'a str) -> &'a str {
    tools
        .iter()
        .find(|tool| tool.name == name)
        .map(|tool| tool.model_name.as_str())
        .unwrap_or(name)
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
            "agent.external_tools_unsupported_phase2b: Agent Phase 2B owns the tool registry"
                .to_string(),
        ));
    }

    if payload.contains_key("tool_choice") {
        return Err(ApplicationError::ValidationError(
            "agent.external_tool_choice_unsupported_phase2b: Agent Phase 2B owns tool choice"
                .to_string(),
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
            "agent.external_tool_turns_unsupported_phase2b: prompt snapshot already contains tool turns"
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
    agent_system_prompt: &str,
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

    let mut marker_count = 0_usize;
    for message in &messages {
        if agent_prompt_marker(message)?.is_some() {
            marker_count += 1;
        }
    }

    match marker_count {
        0 => {
            return Err(ApplicationError::ValidationError(
                "agent.system_prompt_marker_missing: prompt snapshot must include exactly one agentSystemPrompt marker".to_string(),
            ));
        }
        1 => {}
        _ => {
            return Err(ApplicationError::ValidationError(
                "agent.system_prompt_marker_duplicate: prompt snapshot must include exactly one agentSystemPrompt marker".to_string(),
            ));
        }
    }

    messages
        .iter()
        .map(|message| {
            if agent_prompt_marker(message)?.is_some() {
                Ok(text_message(
                    AgentModelRole::System,
                    agent_system_prompt.to_string(),
                ))
            } else {
                message_from_openai_value(message)
            }
        })
        .collect::<Result<Vec<_>, _>>()
}

fn agent_prompt_marker(value: &Value) -> Result<Option<&str>, ApplicationError> {
    let Some(marker) = value
        .as_object()
        .and_then(|object| object.get(AGENT_PROMPT_MARKER_FIELD))
    else {
        return Ok(None);
    };

    let marker = marker.as_str().ok_or_else(|| {
        ApplicationError::ValidationError(
            "agent.invalid_prompt_marker: prompt marker must be a string".to_string(),
        )
    })?;

    if marker != AGENT_SYSTEM_PROMPT_MARKER {
        return Err(ApplicationError::ValidationError(format!(
            "agent.unknown_prompt_marker: unsupported prompt marker {marker}"
        )));
    }

    Ok(Some(marker))
}

fn message_from_openai_value(value: &Value) -> Result<AgentModelMessage, ApplicationError> {
    let object = value.as_object().ok_or_else(|| {
        ApplicationError::ValidationError(
            "agent.invalid_prompt_snapshot: message must be an object".to_string(),
        )
    })?;
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
        parts: content_parts_from_openai_value(object.get("content")),
        provider_metadata,
    })
}

fn content_parts_from_openai_value(value: Option<&Value>) -> Vec<AgentModelContentPart> {
    match value {
        Some(Value::String(text)) => vec![AgentModelContentPart::Text { text: text.clone() }],
        Some(Value::Array(parts)) => parts
            .iter()
            .map(|part| match part {
                Value::String(text) => AgentModelContentPart::Text { text: text.clone() },
                Value::Object(object)
                    if object.get("type").and_then(Value::as_str) == Some("text") =>
                {
                    AgentModelContentPart::Text {
                        text: object
                            .get("text")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                    }
                }
                other => AgentModelContentPart::Native {
                    provider: "openai.content_part".to_string(),
                    value: other.clone(),
                },
            })
            .collect(),
        Some(Value::Null) | None => Vec::new(),
        Some(other) => vec![AgentModelContentPart::Text {
            text: other.to_string(),
        }],
    }
}

fn text_message(role: AgentModelRole, text: String) -> AgentModelMessage {
    AgentModelMessage {
        role,
        parts: vec![AgentModelContentPart::Text { text }],
        provider_metadata: Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        prepare_agent_tool_request, reject_external_tool_request, request_from_prompt_snapshot,
        validate_prompt_snapshot_context_policy,
    };
    use crate::domain::models::agent::profile::ResolvedAgentProfile;
    use crate::domain::models::agent::{AgentModelContentPart, AgentModelRequest, AgentModelRole};

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
                .contains("agent.external_tool_choice_unsupported_phase2b")
        );
    }

    #[test]
    fn agent_system_prompt_replaces_marker_at_prompt_manager_position() {
        let request = request_from_prompt_snapshot(&json!({
            "chatCompletionPayload": {
                "messages": [
                    { "role": "system", "content": "Before marker." },
                    agent_system_marker(),
                    { "role": "user", "content": "hello" }
                ]
            }
        }))
        .expect("request");
        let profile = test_profile(Some(
            "Custom Agent System Prompt.\nUse the creator contract.",
        ));

        let request =
            prepare_agent_tool_request(request, &[], &profile, "run_test").expect("agent request");

        assert_eq!(message_text(&request, 0), "Before marker.");
        assert_eq!(
            message_text(&request, 1),
            "Custom Agent System Prompt.\nUse the creator contract."
        );
        assert_eq!(message_text(&request, 2), "hello");
    }

    #[test]
    fn agent_system_prompt_defaults_when_profile_omits_it() {
        let request = request_from_prompt_snapshot(&json!({
            "chatCompletionPayload": {
                "messages": [
                    agent_system_marker(),
                    { "role": "user", "content": "hello" }
                ]
            }
        }))
        .expect("request");
        let profile = test_profile(None);

        let request =
            prepare_agent_tool_request(request, &[], &profile, "run_test").expect("agent request");

        assert!(message_text(&request, 0).contains("TauriTavern Agent Mode is active."));
    }

    #[test]
    fn agent_system_prompt_marker_is_required() {
        let request = request_from_prompt_snapshot(&json!({
            "chatCompletionPayload": {
                "messages": [{ "role": "user", "content": "hello" }]
            }
        }))
        .expect("request");
        let profile = test_profile(None);

        let error = prepare_agent_tool_request(request, &[], &profile, "run_test")
            .expect_err("marker fails");

        assert!(
            error
                .to_string()
                .contains("agent.system_prompt_marker_missing")
        );
    }

    #[test]
    fn agent_system_prompt_marker_must_be_unique() {
        let request = request_from_prompt_snapshot(&json!({
            "chatCompletionPayload": {
                "messages": [
                    agent_system_marker(),
                    { "role": "user", "content": "hello" },
                    agent_system_marker()
                ]
            }
        }))
        .expect("request");
        let profile = test_profile(None);

        let error = prepare_agent_tool_request(request, &[], &profile, "run_test")
            .expect_err("marker fails");

        assert!(
            error
                .to_string()
                .contains("agent.system_prompt_marker_duplicate")
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
                "messages": [agent_system_marker()]
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
                "messages": [agent_system_marker()]
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
                "messages": [agent_system_marker()]
            }
        });

        validate_prompt_snapshot_context_policy(&prompt_snapshot, &profile)
            .expect("matching truncated context policy should pass");
    }

    fn message_text(request: &AgentModelRequest, index: usize) -> &str {
        if index == 1 {
            assert_eq!(request.messages[index].role, AgentModelRole::System);
        }
        match &request.messages[index].parts[0] {
            AgentModelContentPart::Text { text } => text.as_str(),
            _ => panic!("expected text message"),
        }
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
            "schemaVersion": 1,
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
                "allow": ["workspace.write_file", "workspace.commit", "workspace.finish"],
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
