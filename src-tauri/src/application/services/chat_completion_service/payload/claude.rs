use serde_json::{json, Map, Number, Value};

use crate::application::errors::ApplicationError;

use super::prompt_cache::{
    append_cache_control_to_last, apply_depth_cache_for_claude, PromptCacheConfig,
    PromptCacheProvider,
};
use super::shared::{insert_if_present, message_content_to_text};
use super::tool_calls::{
    extract_openai_tool_calls, message_tool_call_id, message_tool_result_text, OpenAiToolCall,
};

const CLAUDE_THINKING_MIN_TOKENS: i64 = 1024;
const CLAUDE_THINKING_NON_STREAM_CAP: i64 = 21_333;

pub(super) fn build(payload: Map<String, Value>) -> Result<(String, Value), ApplicationError> {
    Ok((
        "/messages".to_string(),
        Value::Object(build_claude_payload(&payload)?),
    ))
}

fn build_claude_payload(
    payload: &Map<String, Value>,
) -> Result<Map<String, Value>, ApplicationError> {
    let cache_config = PromptCacheConfig::from_payload(payload, PromptCacheProvider::Claude);

    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            ApplicationError::ValidationError("Claude request is missing model".to_string())
        })?;

    let (mut messages, mut system_prompt) = convert_messages(payload.get("messages"));

    if let Some(assistant_prefill) = payload
        .get("assistant_prefill")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
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

    if let Some(caching_at_depth) = cache_config.caching_at_depth {
        apply_depth_cache_for_claude(&mut messages, caching_at_depth, cache_config.ttl);
    }

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

    for key in ["temperature", "top_p", "top_k", "stream"] {
        insert_if_present(&mut request, payload, key);
    }

    if let Some(stop) = payload.get("stop").filter(|value| value.is_array()) {
        request.insert("stop_sequences".to_string(), stop.clone());
    }

    let use_system_prompt = payload
        .get("use_sysprompt")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if use_system_prompt && !system_prompt.is_empty() {
        if cache_config.enable_system_prompt_cache {
            append_cache_control_to_last(&mut system_prompt, cache_config.ttl);
        }
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
        if cache_config.enable_system_prompt_cache {
            append_cache_control_to_last(&mut claude_tools, cache_config.ttl);
        }

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

    if supports_claude_thinking(model) {
        let reasoning_effort = payload.get("reasoning_effort").and_then(Value::as_str);
        if let Some(budget_tokens) =
            calculate_claude_budget_tokens(max_tokens, reasoning_effort, stream)
        {
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

            convert_thinking_prefill_to_user(&mut messages);
        }
    }

    request.insert("messages".to_string(), Value::Array(messages));
    request.insert(
        "max_tokens".to_string(),
        Value::Number(Number::from(max_tokens)),
    );

    Ok(request)
}

fn convert_messages(messages: Option<&Value>) -> (Vec<Value>, Vec<Value>) {
    let mut converted = Vec::new();
    let mut system_parts: Vec<Value> = Vec::new();

    let Some(messages) = messages else {
        return (converted, system_parts);
    };

    if let Some(prompt) = messages.as_str() {
        converted.push(json!({
            "role": "user",
            "content": [{ "type": "text", "text": prompt }],
        }));
        return (converted, system_parts);
    }

    let Some(entries) = messages.as_array() else {
        return (converted, system_parts);
    };

    for entry in entries {
        let Some(message) = entry.as_object() else {
            continue;
        };

        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("user")
            .trim()
            .to_lowercase();

        let content_text = message_content_to_text(message.get("content"));

        match role.as_str() {
            "system" => {
                if !content_text.is_empty() {
                    system_parts.push(json!({
                        "type": "text",
                        "text": content_text,
                    }));
                }
            }
            "assistant" => {
                let mut content_blocks = Vec::new();

                if !content_text.is_empty() {
                    content_blocks.push(json!({
                        "type": "text",
                        "text": content_text,
                    }));
                }

                let tool_calls = extract_openai_tool_calls(message.get("tool_calls"));
                if !tool_calls.is_empty() {
                    content_blocks.extend(convert_openai_tool_calls_to_claude_blocks(&tool_calls));
                }

                if !content_blocks.is_empty() {
                    converted.push(json!({
                        "role": "assistant",
                        "content": content_blocks,
                    }));
                }
            }
            "tool" => {
                if let Some(tool_use_id) = message_tool_call_id(message) {
                    converted.push(json!({
                        "role": "user",
                        "content": [{
                            "type": "tool_result",
                            "tool_use_id": tool_use_id,
                            "content": message_tool_result_text(message),
                        }],
                    }));
                } else if !content_text.is_empty() {
                    converted.push(json!({
                        "role": "user",
                        "content": [{
                            "type": "text",
                            "text": content_text,
                        }],
                    }));
                }
            }
            _ => {
                converted.push(json!({
                    "role": "user",
                    "content": [{
                        "type": "text",
                        "text": content_text,
                    }],
                }));
            }
        }
    }

    (converted, system_parts)
}

fn convert_openai_tool_calls_to_claude_blocks(tool_calls: &[OpenAiToolCall]) -> Vec<Value> {
    tool_calls
        .iter()
        .map(|tool_call| {
            json!({
                "type": "tool_use",
                "id": tool_call.id,
                "name": tool_call.name,
                "input": tool_call.arguments,
            })
        })
        .collect()
}

fn map_openai_tools_to_claude(tools: &Value) -> Vec<Value> {
    let Some(entries) = tools.as_array() else {
        return Vec::new();
    };

    entries
        .iter()
        .filter_map(|tool| {
            let object = tool.as_object()?;
            if object.get("type").and_then(Value::as_str) != Some("function") {
                return None;
            }

            let function = object.get("function")?.as_object()?;
            let name = function
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())?
                .to_string();

            let mut mapped = Map::new();
            mapped.insert("name".to_string(), Value::String(name));
            if let Some(description) = function
                .get("description")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                mapped.insert(
                    "description".to_string(),
                    Value::String(description.to_string()),
                );
            }

            let input_schema = function
                .get("parameters")
                .cloned()
                .filter(|value| !value.is_null())
                .unwrap_or_else(|| json!({ "type": "object", "properties": {} }));
            mapped.insert("input_schema".to_string(), input_schema);

            Some(Value::Object(mapped))
        })
        .collect()
}

fn map_tool_choice_to_claude(value: &Value) -> Option<Value> {
    if let Some(choice) = value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return match choice {
            "auto" => Some(json!({ "type": "auto" })),
            "required" => Some(json!({ "type": "any" })),
            "none" => None,
            _ => Some(json!({ "type": "auto" })),
        };
    }

    let object = value.as_object()?;
    if let Some(function_name) = object
        .get("function")
        .and_then(Value::as_object)
        .and_then(|function| function.get("name"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(json!({
            "type": "tool",
            "name": function_name,
        }));
    }

    object
        .get("type")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(|raw| match raw {
            "tool" | "auto" | "any" => Some(json!({ "type": raw })),
            _ => None,
        })
}

fn value_to_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|number| i64::try_from(number).ok()))
}

fn supports_claude_thinking(model: &str) -> bool {
    let model = model.trim().to_ascii_lowercase();
    [
        "claude-3-7",
        "claude-opus-4",
        "claude-sonnet-4",
        "claude-haiku-4-5",
        "claude-opus-4-5",
    ]
    .iter()
    .any(|prefix| model.starts_with(prefix))
}

fn calculate_claude_budget_tokens(
    max_tokens: i64,
    reasoning_effort: Option<&str>,
    stream: bool,
) -> Option<i64> {
    let max_tokens = max_tokens.max(0);
    let effort = reasoning_effort
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .unwrap_or_else(|| "auto".to_string());

    let mut budget_tokens = match effort.as_str() {
        "auto" => return None,
        "min" => CLAUDE_THINKING_MIN_TOKENS,
        "low" => max_tokens.saturating_mul(10) / 100,
        "medium" => max_tokens.saturating_mul(25) / 100,
        "high" => max_tokens.saturating_mul(50) / 100,
        "max" => max_tokens.saturating_mul(95) / 100,
        _ => return None,
    };

    budget_tokens = budget_tokens.max(CLAUDE_THINKING_MIN_TOKENS);
    if !stream {
        budget_tokens = budget_tokens.min(CLAUDE_THINKING_NON_STREAM_CAP);
    }

    Some(budget_tokens)
}

fn convert_thinking_prefill_to_user(messages: &mut [Value]) {
    let Some(last_message) = messages.last_mut().and_then(Value::as_object_mut) else {
        return;
    };

    if last_message.get("role").and_then(Value::as_str) == Some("assistant") {
        last_message.insert("role".to_string(), Value::String("user".to_string()));
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use super::build;

    #[test]
    fn claude_thinking_budget_updates_request_shape() {
        let payload = json!({
            "model": "claude-sonnet-4-5",
            "messages": [{"role": "user", "content": "hello"}],
            "assistant_prefill": "prefill",
            "max_tokens": 1000,
            "reasoning_effort": "medium",
            "temperature": 0.7,
            "top_p": 0.9,
            "top_k": 40,
            "stream": false,
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_, upstream) = build(payload).expect("build should succeed");
        let body = upstream.as_object().expect("body must be object");

        assert_eq!(
            body.get("max_tokens")
                .and_then(Value::as_i64)
                .unwrap_or_default(),
            2024
        );
        assert_eq!(
            body.get("thinking")
                .and_then(Value::as_object)
                .and_then(|thinking| thinking.get("budget_tokens"))
                .and_then(Value::as_i64)
                .unwrap_or_default(),
            1024
        );
        assert!(body.get("temperature").is_none());
        assert!(body.get("top_p").is_none());
        assert!(body.get("top_k").is_none());

        let last_role = body
            .get("messages")
            .and_then(Value::as_array)
            .and_then(|messages| messages.last())
            .and_then(Value::as_object)
            .and_then(|message| message.get("role"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        assert_eq!(last_role, "user");
    }

    #[test]
    fn claude_depth_cache_marks_two_turn_boundaries() {
        let payload = json!({
            "model": "claude-3-5-sonnet-latest",
            "messages": [
                {"role": "user", "content": "u1"},
                {"role": "assistant", "content": "a1"},
                {"role": "user", "content": "u2"},
                {"role": "assistant", "content": "a2"},
                {"role": "user", "content": "u3"}
            ],
            "caching_at_depth": 0,
            "extended_ttl": true
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_, upstream) = build(payload).expect("build should succeed");
        let body = upstream.as_object().expect("body must be object");
        let messages = body
            .get("messages")
            .and_then(Value::as_array)
            .expect("messages must be array");

        let cached_ttls = [2_usize, 4_usize]
            .iter()
            .map(|index| {
                messages
                    .get(*index)
                    .and_then(Value::as_object)
                    .and_then(|message| message.get("content"))
                    .and_then(Value::as_array)
                    .and_then(|parts| parts.last())
                    .and_then(Value::as_object)
                    .and_then(|part| part.get("cache_control"))
                    .and_then(Value::as_object)
                    .and_then(|cache| cache.get("ttl"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
            })
            .collect::<Vec<&str>>();

        assert_eq!(cached_ttls, vec!["1h", "1h"]);
    }

    #[test]
    fn claude_system_and_tools_cache_control_are_applied() {
        let payload = json!({
            "model": "claude-3-5-sonnet-latest",
            "use_sysprompt": true,
            "enable_system_prompt_cache": true,
            "messages": [
                {"role": "system", "content": "system rules"},
                {"role": "user", "content": "hello"}
            ],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "weather",
                    "description": "get weather",
                    "parameters": { "type": "object", "properties": {} }
                }
            }]
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_, upstream) = build(payload).expect("build should succeed");
        let body = upstream.as_object().expect("body must be object");

        let system_has_cache = body
            .get("system")
            .and_then(Value::as_array)
            .and_then(|parts| parts.last())
            .and_then(Value::as_object)
            .and_then(|part| part.get("cache_control"))
            .is_some();
        assert!(system_has_cache);

        let tool_has_cache = body
            .get("tools")
            .and_then(Value::as_array)
            .and_then(|tools| tools.last())
            .and_then(Value::as_object)
            .and_then(|tool| tool.get("cache_control"))
            .is_some();
        assert!(tool_has_cache);
    }

    #[test]
    fn claude_tool_calls_and_results_are_structured() {
        let payload = json!({
            "model": "claude-3-5-sonnet-latest",
            "messages": [
                {
                    "role": "assistant",
                    "tool_calls": [{
                        "id": "call_weather",
                        "type": "function",
                        "function": {
                            "name": "weather",
                            "arguments": "{\"city\":\"Paris\"}"
                        }
                    }]
                },
                {
                    "role": "tool",
                    "tool_call_id": "call_weather",
                    "content": "{\"temperature\":20}"
                }
            ]
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_, upstream) = build(payload).expect("build should succeed");
        let body = upstream.as_object().expect("body must be object");
        let messages = body
            .get("messages")
            .and_then(Value::as_array)
            .expect("messages must be array");

        let assistant_blocks = messages
            .first()
            .and_then(Value::as_object)
            .and_then(|message| message.get("content"))
            .and_then(Value::as_array)
            .expect("assistant content must be array");
        assert_eq!(
            assistant_blocks[0]["type"].as_str().unwrap_or_default(),
            "tool_use"
        );
        assert_eq!(
            assistant_blocks[0]["id"].as_str().unwrap_or_default(),
            "call_weather"
        );
        assert_eq!(
            assistant_blocks[0]["name"].as_str().unwrap_or_default(),
            "weather"
        );

        let tool_result_block = messages
            .get(1)
            .and_then(Value::as_object)
            .and_then(|message| message.get("content"))
            .and_then(Value::as_array)
            .and_then(|parts| parts.first())
            .and_then(Value::as_object)
            .expect("tool result block must be object");
        assert_eq!(
            tool_result_block
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            "tool_result"
        );
        assert_eq!(
            tool_result_block
                .get("tool_use_id")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            "call_weather"
        );
    }
}
