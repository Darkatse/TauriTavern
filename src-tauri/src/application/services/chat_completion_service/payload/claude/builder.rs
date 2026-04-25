use serde_json::{Map, Number, Value, json};

use crate::application::errors::ApplicationError;

use super::super::shared::insert_if_present;
use super::contract::{ClaudeModelContract, ClaudeThinkingMode};
use super::messages::{
    convert_messages, merge_consecutive_messages, move_assistant_images_to_next_user_message,
};
use super::params::{
    has_non_default_temperature, has_non_default_top_k, has_non_default_top_p, value_to_i64,
};
use super::tools::{map_openai_tools_to_claude, map_tool_choice_to_claude};

const CLAUDE_THINKING_MIN_TOKENS: i64 = 1024;
const CLAUDE_THINKING_NON_STREAM_CAP: i64 = 21_333;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClaudeReasoningEffort {
    Min,
    Low,
    Medium,
    High,
    Max,
}

impl ClaudeReasoningEffort {
    fn parse(value: Option<&Value>) -> Result<Option<Self>, ApplicationError> {
        let Some(value) = value.and_then(Value::as_str) else {
            return Ok(None);
        };

        let value = value.trim();
        if value.is_empty() || value.eq_ignore_ascii_case("auto") {
            return Ok(None);
        }

        match value.to_ascii_lowercase().as_str() {
            "min" => Ok(Some(Self::Min)),
            "low" => Ok(Some(Self::Low)),
            "medium" => Ok(Some(Self::Medium)),
            "high" => Ok(Some(Self::High)),
            "max" => Ok(Some(Self::Max)),
            other => Err(ApplicationError::ValidationError(format!(
                "Unsupported Claude reasoning_effort: {other}"
            ))),
        }
    }

    fn calculate_budget_tokens(self, max_tokens: i64, stream: bool) -> i64 {
        let max_tokens = max_tokens.max(0);
        let mut budget_tokens = match self {
            Self::Min => CLAUDE_THINKING_MIN_TOKENS,
            Self::Low => max_tokens.saturating_mul(10) / 100,
            Self::Medium => max_tokens.saturating_mul(25) / 100,
            Self::High => max_tokens.saturating_mul(50) / 100,
            Self::Max => max_tokens.saturating_mul(95) / 100,
        };

        budget_tokens = budget_tokens.max(CLAUDE_THINKING_MIN_TOKENS);
        if !stream {
            budget_tokens = budget_tokens.min(CLAUDE_THINKING_NON_STREAM_CAP);
        }

        budget_tokens
    }

    fn as_adaptive_effort(self) -> &'static str {
        match self {
            Self::Min | Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Max => "max",
        }
    }
}

pub(super) fn build_claude_payload(
    payload: &Map<String, Value>,
) -> Result<Map<String, Value>, ApplicationError> {
    build_claude_payload_inner(payload, true)
}

pub(super) fn build_claude_payload_passthrough(
    payload: &Map<String, Value>,
) -> Result<Map<String, Value>, ApplicationError> {
    build_claude_payload_inner(payload, false)
}

fn build_claude_payload_inner(
    payload: &Map<String, Value>,
    enforce_contract: bool,
) -> Result<Map<String, Value>, ApplicationError> {
    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            ApplicationError::ValidationError("Claude request is missing model".to_string())
        })?;
    let contract = enforce_contract.then(|| ClaudeModelContract::resolve(model));
    let reasoning_effort = if contract.is_some() {
        ClaudeReasoningEffort::parse(payload.get("reasoning_effort"))?
    } else {
        None
    };

    let use_system_prompt = payload
        .get("use_sysprompt")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let use_tools = payload
        .get("tools")
        .and_then(Value::as_array)
        .is_some_and(|items| !items.is_empty())
        || payload
            .get("json_schema")
            .and_then(Value::as_object)
            .and_then(|schema| schema.get("value"))
            .is_some_and(|value| !value.is_null());

    let (mut messages, system_prompt) =
        convert_messages(payload.get("messages"), use_system_prompt, use_tools)?;

    let assistant_prefill = payload
        .get("assistant_prefill")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if assistant_prefill.is_some() && reasoning_effort.is_some() {
        return Err(ApplicationError::ValidationError(format!(
            "Claude model `{model}` does not support assistant_prefill with reasoning_effort"
        )));
    }
    if assistant_prefill.is_some()
        && contract.is_some_and(|contract| !contract.supports_assistant_prefill)
    {
        return Err(ApplicationError::ValidationError(format!(
            "Claude model `{model}` does not support assistant_prefill"
        )));
    }
    if let Some(assistant_prefill) = assistant_prefill {
        messages.push(json!({
            "role": "assistant",
            "content": [
                {
                    "type": "text",
                    "text": assistant_prefill,
                }
            ]
        }));
    }

    move_assistant_images_to_next_user_message(&mut messages);
    merge_consecutive_messages(&mut messages);

    if messages.is_empty() {
        messages.push(json!({
            "role": "user",
            "content": [
                {
                    "type": "text",
                    "text": "",
                }
            ]
        }));
    }

    let mut max_tokens = payload
        .get("max_tokens")
        .or_else(|| payload.get("max_completion_tokens"))
        .and_then(value_to_i64)
        .unwrap_or(CLAUDE_THINKING_MIN_TOKENS);
    max_tokens = max_tokens.max(1);
    let stream = payload
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let mut request = Map::new();
    request.insert("model".to_string(), Value::String(model.to_string()));
    insert_if_present(&mut request, payload, "stream");
    insert_claude_sampling_params(&mut request, payload);

    if let Some(stop) = payload.get("stop").filter(|value| value.is_array()) {
        request.insert("stop_sequences".to_string(), stop.clone());
    }

    if use_system_prompt && !system_prompt.is_empty() {
        request.insert("system".to_string(), Value::Array(system_prompt));
    }

    let mut claude_tools = payload
        .get("tools")
        .map(map_openai_tools_to_claude)
        .unwrap_or_default();

    let mut forced_tool_choice: Option<Value> = None;
    if let Some(json_schema) = payload.get("json_schema").and_then(Value::as_object) {
        if let Some(schema_value) = json_schema
            .get("value")
            .cloned()
            .filter(|value| !value.is_null())
        {
            let schema_name = json_schema
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("response")
                .to_string();

            let mut schema_tool = Map::new();
            schema_tool.insert("name".to_string(), Value::String(schema_name.clone()));
            schema_tool.insert(
                "description".to_string(),
                Value::String("Well-formed JSON object".to_string()),
            );
            schema_tool.insert("input_schema".to_string(), schema_value);
            claude_tools.push(Value::Object(schema_tool));

            forced_tool_choice = Some(json!({
                "type": "tool",
                "name": schema_name,
            }));
        }
    }

    if !claude_tools.is_empty() {
        request.insert("tools".to_string(), Value::Array(claude_tools));

        let tool_choice = forced_tool_choice.or_else(|| {
            payload
                .get("tool_choice")
                .and_then(map_tool_choice_to_claude)
        });
        if let Some(choice) = tool_choice {
            request.insert("tool_choice".to_string(), choice);
        }
    }

    if let Some(contract) = contract {
        match contract.thinking {
            ClaudeThinkingMode::Unsupported => {
                if reasoning_effort.is_some() {
                    return Err(ApplicationError::ValidationError(format!(
                        "Claude model `{model}` does not support reasoning_effort"
                    )));
                }
            }
            ClaudeThinkingMode::ManualOnly => {
                if let Some(reasoning_effort) = reasoning_effort {
                    let budget_tokens =
                        reasoning_effort.calculate_budget_tokens(max_tokens, stream);
                    if max_tokens <= CLAUDE_THINKING_MIN_TOKENS {
                        max_tokens += CLAUDE_THINKING_MIN_TOKENS;
                    }

                    request.insert(
                        "thinking".to_string(),
                        json!({
                            "type": "enabled",
                            "budget_tokens": budget_tokens,
                        }),
                    );
                    request.remove("temperature");
                    request.remove("top_p");
                    request.remove("top_k");
                }
            }
            ClaudeThinkingMode::ManualOrAdaptive | ClaudeThinkingMode::AdaptiveOnly => {
                if let Some(reasoning_effort) = reasoning_effort {
                    request.insert(
                        "thinking".to_string(),
                        build_claude_adaptive_thinking(payload),
                    );
                    if contract.supports_output_effort {
                        request.insert(
                            "output_config".to_string(),
                            json!({
                                "effort": reasoning_effort.as_adaptive_effort(),
                            }),
                        );
                    }
                }
            }
        }
    }

    request.insert("messages".to_string(), Value::Array(messages));
    request.insert(
        "max_tokens".to_string(),
        Value::Number(Number::from(max_tokens)),
    );

    Ok(request)
}

fn insert_claude_sampling_params(request: &mut Map<String, Value>, payload: &Map<String, Value>) {
    if has_non_default_temperature(payload) {
        insert_if_present(request, payload, "temperature");
    }
    if has_non_default_top_p(payload) {
        insert_if_present(request, payload, "top_p");
    }
    if has_non_default_top_k(payload) {
        insert_if_present(request, payload, "top_k");
    }
}

fn build_claude_adaptive_thinking(payload: &Map<String, Value>) -> Value {
    let display = if payload
        .get("include_reasoning")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        "summarized"
    } else {
        "omitted"
    };

    json!({
        "type": "adaptive",
        "display": display,
    })
}
