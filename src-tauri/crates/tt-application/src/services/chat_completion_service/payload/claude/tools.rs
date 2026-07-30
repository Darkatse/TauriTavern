use serde_json::{Map, Value, json};

use super::super::tool_calls::OpenAiToolCall;

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

pub(super) fn map_openai_tools_to_claude(tools: &Value) -> Vec<Value> {
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

pub(super) fn map_tool_choice_to_claude(value: &Value) -> Option<Value> {
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
