use serde_json::{Map, Value};

use crate::errors::ApplicationError;

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

        if thinking_mode == Some(DeepSeekThinkingMode::Enabled) {
            ensure_tool_context_reasoning_content(&mut processed, has_tools)?;
        }

        let processed = Value::Array(processed);
        tool_calls::validate_openai_chat_tool_transcript(Some(&processed), false)?;
        payload.insert("messages".to_string(), processed);
    }

    strip_empty_required_arrays_from_tools(&mut payload);

    let (endpoint, mut upstream_payload) = openai::build(payload)?;
    if endpoint == "/chat/completions"
        && let (Some(mode), Some(body)) = (thinking_mode, upstream_payload.as_object_mut())
    {
        apply_thinking_mode(body, mode, reasoning_effort);
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
                (model.starts_with("deepseek-v4") || model == "deepseek-flash")
                    .then_some(DeepSeekThinkingMode::Enabled)
            }),
    }
}

/// DeepSeek V4 家族（v4* / flash）判定，用于 OpenAI 兼容源的模型名匹配。
/// 取 `/` 分隔的最后一段再匹配，容忍网关前缀（如 `GO/deepseek-flash`、
/// `OR/deepseek-v4.1-flash`、`newapi/openrouter/deepseek-v4`）。
/// DeepSeek 3.x 不命中。
pub(super) fn is_deepseek_v4_model(model: &str) -> bool {
    let normalized = model.trim().to_ascii_lowercase();
    let last_segment = normalized.rsplit('/').next().unwrap_or_default();

    last_segment.starts_with("deepseek-v4") || last_segment.starts_with("deepseek-flash")
}

/// 对 OpenAI 兼容源（Custom/OpenCode OpenAiCompat）构建后的请求体应用
/// DeepSeek V4 语义对齐：当模型命中 V4 家族时，对齐官方 DeepSeek 渠道的
/// thinking 控制、effort 归一化、采样参数清理、工具上下文 reasoning_content
/// 补齐与空 required 剥离。非 V4 模型零改动。`include_reasoning` 由调用方
/// 在 builder 白名单化之前从 TT payload 读出传入（思考开关意图），它本身
/// 不透传给上游。
pub(super) fn apply_deepseek_v4_compat(
    upstream_payload: &mut Value,
    include_reasoning: Option<bool>,
) -> Result<(), ApplicationError> {
    let Some(body) = upstream_payload.as_object_mut() else {
        return Ok(());
    };

    let model = body
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if !is_deepseek_v4_model(&model) {
        return Ok(());
    }

    // 未提供时与官方渠道默认一致：V4 家族默认开启思考。
    let thinking_mode = match include_reasoning {
        Some(true) => DeepSeekThinkingMode::Enabled,
        Some(false) => DeepSeekThinkingMode::Disabled,
        None => DeepSeekThinkingMode::Enabled,
    };

    let reasoning_effort = match thinking_mode {
        DeepSeekThinkingMode::Enabled => normalize_reasoning_effort(
            body.get("reasoning_effort")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        )?,
        DeepSeekThinkingMode::Disabled => None,
    };

    let has_tools = body
        .get("tools")
        .and_then(Value::as_array)
        .is_some_and(|tools| !tools.is_empty());

    if thinking_mode == DeepSeekThinkingMode::Enabled
        && let Some(messages) = body.get_mut("messages").and_then(Value::as_array_mut)
    {
        ensure_tool_context_reasoning_content(messages, has_tools)?;
    }

    strip_empty_required_arrays_from_tools(body);

    // CUSTOM 源会无条件转发 reasoning_effort；官方渠道语义是 disabled 时
    // 不发送 effort，这里对齐（enabled 时 apply_thinking_mode 内部会重写）。
    if thinking_mode == DeepSeekThinkingMode::Disabled {
        body.remove("reasoning_effort");
    }

    apply_thinking_mode(body, thinking_mode, reasoning_effort);

    Ok(())
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

fn ensure_tool_context_reasoning_content(
    messages: &mut [Value],
    has_tools: bool,
) -> Result<(), ApplicationError> {
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
        for model in [
            "deepseek-v4-flash",
            "deepseek-v4-flash-vision-exp",
            "deepseek-flash",
            "deepseek-v4-pro",
        ] {
            let payload = json!({
                "model": model,
                "messages": [{"role": "user", "content": "hello"}],
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
