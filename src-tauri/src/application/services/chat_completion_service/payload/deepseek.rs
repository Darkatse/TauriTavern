use serde_json::{Map, Value};

use crate::application::errors::ApplicationError;

use super::openai;
use super::prompt_post_processing::{PromptNames, PromptProcessingType, post_process_prompt};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeepSeekThinkingMode {
    Enabled,
    Disabled,
}

pub(super) fn build(mut payload: Map<String, Value>) -> Result<(String, Value), ApplicationError> {
    let names = PromptNames::from_payload(&payload);
    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let thinking_mode = resolve_thinking_mode(&payload, &model);
    let reasoning_effort = match thinking_mode {
        Some(DeepSeekThinkingMode::Enabled) => normalize_reasoning_effort(
            payload
                .get("reasoning_effort")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        )?,
        _ => None,
    };
    let tools_snapshot = payload.get("tools").cloned();

    if let Some(messages) = payload.get_mut("messages").and_then(Value::as_array_mut) {
        let raw = std::mem::take(messages);
        let mut processed = post_process_prompt(raw, PromptProcessingType::SemiTools, &names);

        add_assistant_prefix(&mut processed, tools_snapshot.as_ref(), "prefix");

        if thinking_mode == Some(DeepSeekThinkingMode::Enabled) {
            ensure_tool_context_reasoning_content(&mut processed)?;
        }

        payload.insert("messages".to_string(), Value::Array(processed));
    }

    strip_empty_required_arrays_from_tools(&mut payload);

    let (endpoint, mut upstream_payload) = openai::build(payload);
    if endpoint == "/chat/completions" {
        if let (Some(mode), Some(body)) = (thinking_mode, upstream_payload.as_object_mut()) {
            apply_thinking_mode(body, mode, reasoning_effort);
        }
    }

    Ok((endpoint, upstream_payload))
}

fn resolve_thinking_mode(
    payload: &Map<String, Value>,
    model: &str,
) -> Option<DeepSeekThinkingMode> {
    let model = model.trim().to_ascii_lowercase();

    match model.as_str() {
        "deepseek-chat" => Some(DeepSeekThinkingMode::Disabled),
        "deepseek-reasoner" => Some(DeepSeekThinkingMode::Enabled),
        _ => payload
            .get("include_reasoning")
            .and_then(Value::as_bool)
            .map(|include_reasoning| {
                if include_reasoning {
                    DeepSeekThinkingMode::Enabled
                } else {
                    DeepSeekThinkingMode::Disabled
                }
            })
            .or_else(|| {
                model
                    .starts_with("deepseek-v4-")
                    .then_some(DeepSeekThinkingMode::Enabled)
            }),
    }
}

fn normalize_reasoning_effort(value: &str) -> Result<Option<&'static str>, ApplicationError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "" | "auto" => Ok(None),
        "min" | "minimum" | "low" | "medium" | "high" => Ok(Some("high")),
        "max" | "maximum" | "xhigh" => Ok(Some("max")),
        other => Err(ApplicationError::ValidationError(format!(
            "Unsupported DeepSeek reasoning_effort: {other}"
        ))),
    }
}

fn apply_thinking_mode(
    body: &mut Map<String, Value>,
    mode: DeepSeekThinkingMode,
    reasoning_effort: Option<&str>,
) {
    body.insert(
        "thinking".to_string(),
        serde_json::json!({
            "type": match mode {
                DeepSeekThinkingMode::Enabled => "enabled",
                DeepSeekThinkingMode::Disabled => "disabled",
            },
        }),
    );

    if mode == DeepSeekThinkingMode::Enabled {
        for key in [
            "temperature",
            "top_p",
            "presence_penalty",
            "frequency_penalty",
        ] {
            body.remove(key);
        }

        if let Some(reasoning_effort) = reasoning_effort {
            body.insert(
                "reasoning_effort".to_string(),
                Value::String(reasoning_effort.to_string()),
            );
        }
    }
}

fn ensure_tool_context_reasoning_content(messages: &mut [Value]) -> Result<(), ApplicationError> {
    let has_tool_context = messages.iter().any(|message| {
        let Some(message_object) = message.as_object() else {
            return false;
        };

        message_object
            .get("tool_calls")
            .and_then(Value::as_array)
            .is_some_and(|calls| !calls.is_empty())
            || message_object.get("role").and_then(Value::as_str) == Some("tool")
    });

    if !has_tool_context {
        return Ok(());
    }

    for message in messages {
        let Some(message_object) = message.as_object_mut() else {
            continue;
        };

        if message_object.get("role").and_then(Value::as_str) != Some("assistant") {
            continue;
        }

        match message_object.get("reasoning_content") {
            Some(Value::String(_)) => {}
            Some(_) => {
                return Err(ApplicationError::ValidationError(
                    "DeepSeek thinking assistant messages in tool context must have string reasoning_content"
                        .to_string(),
                ));
            }
            None => {
                message_object.insert(
                    "reasoning_content".to_string(),
                    Value::String(String::new()),
                );
            }
        }
    }

    Ok(())
}

fn add_assistant_prefix(messages: &mut [Value], tools: Option<&Value>, property: &str) {
    if messages.is_empty() {
        return;
    }

    let has_tools = tools
        .and_then(Value::as_array)
        .is_some_and(|tools| !tools.is_empty());
    let has_tool_messages = messages.iter().any(|message| {
        message
            .as_object()
            .and_then(|object| object.get("role"))
            .and_then(Value::as_str)
            == Some("tool")
    });

    if has_tools || has_tool_messages {
        return;
    }

    let Some(last_message) = messages.last_mut().and_then(Value::as_object_mut) else {
        return;
    };

    if last_message.get("role").and_then(Value::as_str) != Some("assistant") {
        return;
    }

    last_message.insert(property.to_string(), Value::Bool(true));
}

fn strip_empty_required_arrays_from_tools(payload: &mut Map<String, Value>) {
    let Some(tools) = payload.get_mut("tools").and_then(Value::as_array_mut) else {
        return;
    };

    for tool in tools {
        let should_remove = tool
            .as_object()
            .and_then(|tool| tool.get("function"))
            .and_then(Value::as_object)
            .and_then(|function| function.get("parameters"))
            .and_then(Value::as_object)
            .and_then(|parameters| parameters.get("required"))
            .and_then(Value::as_array)
            .is_some_and(|required| required.is_empty());

        if !should_remove {
            continue;
        }

        if let Some(parameters) = tool
            .as_object_mut()
            .and_then(|tool| tool.get_mut("function"))
            .and_then(Value::as_object_mut)
            .and_then(|function| function.get_mut("parameters"))
            .and_then(Value::as_object_mut)
        {
            parameters.remove("required");
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::build;

    #[test]
    fn deepseek_build_marks_assistant_prefill_as_prefix() {
        let payload = json!({
            "model": "deepseek-reasoner",
            "messages": [
                {"role":"user","content":"hi"},
                {"role":"assistant","content":"prefill"}
            ],
            "chat_completion_source": "deepseek"
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_, upstream) = build(payload).expect("payload should build");
        let body = upstream.as_object().expect("body must be object");

        let last = body
            .get("messages")
            .and_then(Value::as_array)
            .and_then(|messages| messages.last())
            .and_then(Value::as_object)
            .expect("last message must be object");

        assert_eq!(last.get("role").and_then(Value::as_str), Some("assistant"));
        assert_eq!(last.get("prefix").and_then(Value::as_bool), Some(true));
    }

    #[test]
    fn deepseek_v4_enables_thinking_and_maps_effort() {
        let payload = json!({
            "model": "deepseek-v4-pro",
            "messages": [{"role": "user", "content": "hello"}],
            "include_reasoning": true,
            "reasoning_effort": "max",
            "temperature": 1.2,
            "top_p": 0.7,
            "presence_penalty": 0.1,
            "frequency_penalty": 0.2,
            "chat_completion_source": "deepseek"
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_, upstream) = build(payload).expect("payload should build");
        let body = upstream.as_object().expect("body must be object");

        assert_eq!(
            body.get("thinking")
                .and_then(Value::as_object)
                .and_then(|thinking| thinking.get("type"))
                .and_then(Value::as_str),
            Some("enabled")
        );
        assert_eq!(
            body.get("reasoning_effort").and_then(Value::as_str),
            Some("max")
        );
        assert!(body.get("temperature").is_none());
        assert!(body.get("top_p").is_none());
        assert!(body.get("presence_penalty").is_none());
        assert!(body.get("frequency_penalty").is_none());
    }

    #[test]
    fn deepseek_v4_disables_thinking_without_reasoning_effort() {
        let payload = json!({
            "model": "deepseek-v4-flash",
            "messages": [{"role": "user", "content": "hello"}],
            "include_reasoning": false,
            "reasoning_effort": "high",
            "temperature": 1.2,
            "chat_completion_source": "deepseek"
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_, upstream) = build(payload).expect("payload should build");
        let body = upstream.as_object().expect("body must be object");

        assert_eq!(
            body.get("thinking")
                .and_then(Value::as_object)
                .and_then(|thinking| thinking.get("type"))
                .and_then(Value::as_str),
            Some("disabled")
        );
        assert!(body.get("reasoning_effort").is_none());
        assert!(body.get("temperature").is_some());
    }

    #[test]
    fn deepseek_alias_defaults_match_compat_modes() {
        let chat_payload = json!({
            "model": "deepseek-chat",
            "messages": [{"role": "user", "content": "hello"}],
            "chat_completion_source": "deepseek"
        })
        .as_object()
        .cloned()
        .expect("payload must be object");
        let reasoner_payload = json!({
            "model": "deepseek-reasoner",
            "messages": [{"role": "user", "content": "hello"}],
            "chat_completion_source": "deepseek"
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_, chat_upstream) = build(chat_payload).expect("payload should build");
        let (_, reasoner_upstream) = build(reasoner_payload).expect("payload should build");

        assert_eq!(
            chat_upstream
                .get("thinking")
                .and_then(Value::as_object)
                .and_then(|thinking| thinking.get("type"))
                .and_then(Value::as_str),
            Some("disabled")
        );
        assert_eq!(
            reasoner_upstream
                .get("thinking")
                .and_then(Value::as_object)
                .and_then(|thinking| thinking.get("type"))
                .and_then(Value::as_str),
            Some("enabled")
        );
    }

    #[test]
    fn deepseek_aliases_ignore_include_reasoning_overrides() {
        let chat_payload = json!({
            "model": "deepseek-chat",
            "messages": [{"role": "user", "content": "hello"}],
            "include_reasoning": true,
            "chat_completion_source": "deepseek"
        })
        .as_object()
        .cloned()
        .expect("payload must be object");
        let reasoner_payload = json!({
            "model": "deepseek-reasoner",
            "messages": [{"role": "user", "content": "hello"}],
            "include_reasoning": false,
            "chat_completion_source": "deepseek"
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_, chat_upstream) = build(chat_payload).expect("payload should build");
        let (_, reasoner_upstream) = build(reasoner_payload).expect("payload should build");

        assert_eq!(
            chat_upstream
                .get("thinking")
                .and_then(Value::as_object)
                .and_then(|thinking| thinking.get("type"))
                .and_then(Value::as_str),
            Some("disabled")
        );
        assert_eq!(
            reasoner_upstream
                .get("thinking")
                .and_then(Value::as_object)
                .and_then(|thinking| thinking.get("type"))
                .and_then(Value::as_str),
            Some("enabled")
        );
    }

    #[test]
    fn deepseek_thinking_tool_calls_keep_reasoning_content() {
        let payload = json!({
            "model": "deepseek-v4-flash",
            "messages": [
                {"role":"user","content":"weather"},
                {
                    "role":"assistant",
                    "content":"",
                    "reasoning_content":"need weather",
                    "tool_calls":[{
                        "id":"call_1",
                        "type":"function",
                        "function":{"name":"weather","arguments":"{}"}
                    }]
                },
                {"role":"tool","tool_call_id":"call_1","content":"cloudy"}
            ],
            "include_reasoning": true,
            "chat_completion_source": "deepseek"
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_, upstream) = build(payload).expect("payload should build");
        let assistant = upstream
            .get("messages")
            .and_then(Value::as_array)
            .and_then(|messages| messages.get(1))
            .and_then(Value::as_object)
            .expect("assistant must be object");

        assert_eq!(
            assistant.get("reasoning_content").and_then(Value::as_str),
            Some("need weather")
        );
    }

    #[test]
    fn deepseek_thinking_tool_context_fills_missing_reasoning_content() {
        let payload = json!({
            "model": "deepseek-v4-flash",
            "messages": [
                {"role":"user","content":"weather"},
                {"role":"assistant","content":"I'll check."},
                {"role":"user","content":"ok"},
                {
                    "role":"assistant",
                    "content":"",
                    "tool_calls":[{
                        "id":"call_1",
                        "type":"function",
                        "function":{"name":"weather","arguments":"{}"}
                    }]
                },
                {"role":"tool","tool_call_id":"call_1","content":"cloudy"}
            ],
            "include_reasoning": true,
            "chat_completion_source": "deepseek"
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_, upstream) = build(payload).expect("payload should build");
        let messages = upstream
            .get("messages")
            .and_then(Value::as_array)
            .expect("messages must be array");

        for index in [1_usize, 3] {
            let assistant = messages
                .get(index)
                .and_then(Value::as_object)
                .expect("assistant must be object");
            assert_eq!(
                assistant.get("reasoning_content").and_then(Value::as_str),
                Some("")
            );
        }
    }

    #[test]
    fn deepseek_thinking_without_tool_context_does_not_add_reasoning_content() {
        let payload = json!({
            "model": "deepseek-v4-flash",
            "messages": [
                {"role":"user","content":"hello"},
                {"role":"assistant","content":"hi"}
            ],
            "include_reasoning": true,
            "chat_completion_source": "deepseek"
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_, upstream) = build(payload).expect("payload should build");
        let assistant = upstream
            .get("messages")
            .and_then(Value::as_array)
            .and_then(|messages| messages.get(1))
            .and_then(Value::as_object)
            .expect("assistant must be object");

        assert!(assistant.get("reasoning_content").is_none());
    }

    #[test]
    fn deepseek_thinking_tool_context_rejects_non_string_reasoning_content() {
        let payload = json!({
            "model": "deepseek-v4-flash",
            "messages": [
                {"role":"user","content":"weather"},
                {
                    "role":"assistant",
                    "content":"",
                    "reasoning_content": {"text":"need weather"},
                    "tool_calls":[{
                        "id":"call_1",
                        "type":"function",
                        "function":{"name":"weather","arguments":"{}"}
                    }]
                },
                {"role":"tool","tool_call_id":"call_1","content":"cloudy"}
            ],
            "include_reasoning": true,
            "chat_completion_source": "deepseek"
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let error = build(payload).expect_err("non-string reasoning_content must fail");
        assert!(error.to_string().contains("string reasoning_content"));
    }

    #[test]
    fn deepseek_rejects_unknown_reasoning_effort() {
        let payload = json!({
            "model": "deepseek-v4-flash",
            "messages": [{"role": "user", "content": "hello"}],
            "include_reasoning": true,
            "reasoning_effort": "auto-ish",
            "chat_completion_source": "deepseek"
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let error = build(payload).expect_err("invalid effort must fail");
        assert!(
            error
                .to_string()
                .contains("Unsupported DeepSeek reasoning_effort")
        );
    }
}
