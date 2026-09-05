use serde_json::{Map, Value, json};

use crate::errors::ApplicationError;

use super::super::tool_calls::OpenAiToolCall;
use super::super::tool_choice::{OpenAiToolChoice, parse_openai_tool_choice};

pub(super) fn convert_openai_tool_calls_to_claude_blocks(
    tool_calls: &[OpenAiToolCall],
) -> Vec<Value> {
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

pub(super) fn map_openai_tools_to_claude(tools: &Value, eager_input_streaming: bool) -> Vec<Value> {
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
            if eager_input_streaming {
                mapped.insert("eager_input_streaming".to_string(), Value::Bool(true));
            }

            Some(Value::Object(mapped))
        })
        .collect()
}

pub(super) fn map_tool_choice_to_claude(value: &Value) -> Result<Value, ApplicationError> {
    Ok(match parse_openai_tool_choice(value, "Claude")? {
        OpenAiToolChoice::None => json!({ "type": "none" }),
        OpenAiToolChoice::Auto => json!({ "type": "auto" }),
        OpenAiToolChoice::Required => json!({ "type": "any" }),
        OpenAiToolChoice::Specific(name) => json!({ "type": "tool", "name": name }),
    })
}
