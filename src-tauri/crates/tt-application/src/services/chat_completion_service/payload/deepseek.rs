use serde_json::{Map, Value};
use tt_ports::repositories::chat_completion_repository::ChatCompletionSource;

use crate::errors::ApplicationError;

use super::super::exchange::ChatCompletionProviderFormat;
use super::super::model_capabilities::{
    RequestedReasoningEffort, parse_known_reasoning_effort, unsupported_reasoning_effort,
};
use super::openai;
use super::prompt_post_processing::{PromptNames, PromptProcessingType, post_process_prompt};
use super::shared::add_assistant_prefix;
use super::tool_calls;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeepSeekThinkingMode {
    Enabled,
    Disabled,
}

/// Resolved from the TT request, before a wire builder can drop provider controls.
pub(super) struct RequestOptions {
    thinking_mode: Option<DeepSeekThinkingMode>,
    reasoning_effort: Option<Value>,
    strip_sampling: bool,
}

impl RequestOptions {
    pub(super) fn from_payload(
        source: ChatCompletionSource,
        payload: &Map<String, Value>,
    ) -> Result<Option<Self>, ApplicationError> {
        let model = payload
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or_default();

        if source == ChatCompletionSource::DeepSeek {
            let thinking_mode = resolve_thinking_mode(payload, model);
            let reasoning_effort = if thinking_mode == Some(DeepSeekThinkingMode::Enabled) {
                normalize_reasoning_effort(
                    payload
                        .get("reasoning_effort")
                        .and_then(Value::as_str)
                        .unwrap_or_default(),
                )?
                .map(|effort| Value::String(effort.to_string()))
            } else {
                None
            };
            return Ok(Some(Self {
                thinking_mode,
                reasoning_effort,
                strip_sampling: true,
            }));
        }

        if !matches!(
            source,
            ChatCompletionSource::Custom | ChatCompletionSource::OpenCode
        ) || ChatCompletionProviderFormat::from_payload(source, payload)?
            != ChatCompletionProviderFormat::OpenAiCompatible
            || !is_deepseek_v4_model(model)
        {
            return Ok(None);
        }

        let thinking_mode =
            if payload.get("include_reasoning").and_then(Value::as_bool) == Some(false) {
                DeepSeekThinkingMode::Disabled
            } else {
                DeepSeekThinkingMode::Enabled
            };
        // Compatible gateways own their parameter contract. Preserve explicit values,
        // including effort that the OpenCode wire builder would otherwise discard.
        Ok(Some(Self {
            thinking_mode: Some(thinking_mode),
            reasoning_effort: payload.get("reasoning_effort").cloned(),
            strip_sampling: false,
        }))
    }

    pub(super) fn apply(self, body: &mut Map<String, Value>) {
        if let Some(mode) = self.thinking_mode {
            body.insert(
                "thinking".to_string(),
                serde_json::json!({
                    "type": match mode {
                        DeepSeekThinkingMode::Enabled => "enabled",
                        DeepSeekThinkingMode::Disabled => "disabled",
                    },
                }),
            );
        }

        if let Some(effort) = self.reasoning_effort {
            body.insert("reasoning_effort".to_string(), effort);
        } else {
            body.remove("reasoning_effort");
        }

        if self.thinking_mode == Some(DeepSeekThinkingMode::Enabled) {
            if self.strip_sampling {
                for key in [
                    "temperature",
                    "top_p",
                    "presence_penalty",
                    "frequency_penalty",
                ] {
                    body.remove(key);
                }
            }
            let has_tools = body
                .get("tools")
                .and_then(Value::as_array)
                .is_some_and(|tools| !tools.is_empty());
            if let Some(messages) = body.get_mut("messages").and_then(Value::as_array_mut) {
                fill_missing_tool_reasoning_content(messages, has_tools);
            }
        }

        strip_empty_required_arrays_from_tools(body);
    }
}

/// The native source owns its prompt preset; compatible sources keep the user's preset.
pub(super) fn build(mut payload: Map<String, Value>) -> Result<(String, Value), ApplicationError> {
    let names = PromptNames::from_payload(&payload);
    let has_tools = payload
        .get("tools")
        .and_then(Value::as_array)
        .is_some_and(|tools| !tools.is_empty());

    if let Some(messages) = payload.get_mut("messages").and_then(Value::as_array_mut) {
        let raw = std::mem::take(messages);
        let mut processed = post_process_prompt(raw, PromptProcessingType::SemiTools, &names);
        let has_tool_messages = processed.iter().any(|message| {
            message
                .as_object()
                .and_then(|object| object.get("role"))
                .and_then(Value::as_str)
                == Some("tool")
        });

        if !has_tools && !has_tool_messages {
            add_assistant_prefix(&mut processed, "prefix");
        }

        let processed = Value::Array(processed);
        tool_calls::validate_openai_chat_tool_transcript(Some(&processed), false)?;
        payload.insert("messages".to_string(), processed);
    }

    openai::build(payload)
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
                (model.starts_with("deepseek-v4") || model == "deepseek-flash")
                    .then_some(DeepSeekThinkingMode::Enabled)
            }),
    }
}

/// Match the final segment to allow gateway prefixes such as `GO/deepseek-flash`.
fn is_deepseek_v4_model(model: &str) -> bool {
    let normalized = model.trim().to_ascii_lowercase();
    let last_segment = normalized.rsplit('/').next().unwrap_or_default();

    last_segment.starts_with("deepseek-v4") || last_segment.starts_with("deepseek-flash")
}

fn normalize_reasoning_effort(value: &str) -> Result<Option<&'static str>, ApplicationError> {
    match parse_known_reasoning_effort(value, "DeepSeek")? {
        RequestedReasoningEffort::Auto => Ok(None),
        RequestedReasoningEffort::None => Err(unsupported_reasoning_effort("DeepSeek", value)),
        RequestedReasoningEffort::Minimal | RequestedReasoningEffort::Low => Ok(Some("low")),
        RequestedReasoningEffort::Medium
        | RequestedReasoningEffort::High
        | RequestedReasoningEffort::XHigh => Ok(Some("high")),
        RequestedReasoningEffort::Max => Ok(Some("max")),
    }
}

fn fill_missing_tool_reasoning_content(messages: &mut [Value], has_tools: bool) {
    let has_tool_context = has_tools
        || messages.iter().any(|message| {
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
        return;
    }

    for message in messages {
        let Some(message_object) = message.as_object_mut() else {
            continue;
        };

        if message_object.get("role").and_then(Value::as_str) != Some("assistant") {
            continue;
        }

        // Preserve explicit values for later body overrides and upstream handling.
        message_object
            .entry("reasoning_content")
            .or_insert_with(|| Value::String(String::new()));
    }
}

fn strip_empty_required_arrays_from_tools(payload: &mut Map<String, Value>) {
    let Some(tools) = payload.get_mut("tools").and_then(Value::as_array_mut) else {
        return;
    };

    for tool in tools {
        let Some(parameters) = tool
            .pointer_mut("/function/parameters")
            .and_then(Value::as_object_mut)
        else {
            continue;
        };
        if parameters
            .get("required")
            .and_then(Value::as_array)
            .is_some_and(|required| required.is_empty())
        {
            parameters.remove("required");
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Map, Value, json};
    use tt_ports::repositories::chat_completion_repository::ChatCompletionSource;

    use crate::errors::ApplicationError;

    fn build(payload: Map<String, Value>) -> Result<(String, Value), ApplicationError> {
        super::super::build_payload(ChatCompletionSource::DeepSeek, payload)
    }

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
        for (model, requested_effort, expected_effort) in [
            ("deepseek-v4-flash", "medium", Some("high")),
            ("deepseek-v4-flash-vision-exp", "minimal", Some("low")),
            ("deepseek-flash", "auto", None),
            ("deepseek-v4-pro", "max", Some("max")),
        ] {
            let payload = json!({
                "model": model,
                "messages": [{"role": "user", "content": "hello"}],
                "reasoning_effort": requested_effort,
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
                body.get("reasoning_effort"),
                expected_effort.map(|effort| json!(effort)).as_ref()
            );
            assert!(body.get("temperature").is_none());
            assert!(body.get("top_p").is_none());
            assert!(body.get("presence_penalty").is_none());
            assert!(body.get("frequency_penalty").is_none());
        }
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
