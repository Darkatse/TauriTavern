use serde_json::{Map, Value, json};

use crate::application::errors::ApplicationError;

use super::prompt_post_processing::PromptNames;
use super::shared::insert_if_present;
use super::shared::message_content_to_text;

const PROMPT_PLACEHOLDER: &str = "Let's get started.";

pub(super) fn build(payload: Map<String, Value>) -> Result<(String, Value), ApplicationError> {
    let names = PromptNames::from_payload(&payload);
    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            ApplicationError::ValidationError("Cohere request is missing model".to_string())
        })?;

    let messages = convert_messages(payload.get("messages"), &names)?;

    let mut request = Map::new();
    request.insert("model".to_string(), Value::String(model.to_string()));
    request.insert("messages".to_string(), Value::Array(messages));
    request.insert("documents".to_string(), Value::Array(Vec::new()));

    for key in [
        "stream",
        "temperature",
        "max_tokens",
        "seed",
        "frequency_penalty",
        "presence_penalty",
    ] {
        insert_if_present(&mut request, &payload, key);
    }

    if let Some(top_k) = payload.get("top_k").filter(|value| !value.is_null()) {
        request.insert("k".to_string(), top_k.clone());
    }
    if let Some(top_p) = payload.get("top_p").filter(|value| !value.is_null()) {
        request.insert("p".to_string(), top_p.clone());
    }
    if let Some(stop) = payload.get("stop").filter(|value| !value.is_null()) {
        request.insert("stop_sequences".to_string(), stop.clone());
    }

    if let Some(tools) = payload
        .get("tools")
        .and_then(Value::as_array)
        .filter(|tools| !tools.is_empty())
    {
        let mut tools = tools.clone();
        sanitize_openai_tools(&mut tools);
        request.insert("tools".to_string(), Value::Array(tools));
    }

    if model.ends_with("08-2024") {
        request.insert(
            "safety_mode".to_string(),
            Value::String("OFF".to_string()),
        );
    }

    if let Some(schema_value) = payload
        .get("json_schema")
        .and_then(Value::as_object)
        .and_then(|schema| schema.get("value"))
        .cloned()
        .filter(|value| !value.is_null())
    {
        request.insert(
            "response_format".to_string(),
            json!({
                "type": "json_schema",
                "schema": schema_value,
            }),
        );
    }

    Ok(("/chat".to_string(), Value::Object(request)))
}

fn convert_messages(
    messages: Option<&Value>,
    names: &PromptNames,
) -> Result<Vec<Value>, ApplicationError> {
    let mut messages = match messages {
        Some(Value::String(prompt)) => vec![json!({
            "role": "user",
            "content": prompt,
        })],
        Some(Value::Array(items)) => items.clone(),
        None | Some(Value::Null) => Vec::new(),
        Some(_) => {
            return Err(ApplicationError::ValidationError(
                "Cohere messages must be an array".to_string(),
            ));
        }
    };

    if messages.is_empty() {
        messages.push(json!({
            "role": "user",
            "content": PROMPT_PLACEHOLDER,
        }));
    }

    apply_tool_call_primer(&mut messages);
    strip_unsupported_names(&mut messages, names);

    Ok(messages)
}

fn apply_tool_call_primer(messages: &mut Vec<Value>) {
    let mut index = 0_usize;

    while index < messages.len() {
        let has_tool_calls = messages
            .get(index)
            .and_then(Value::as_object)
            .and_then(|object| object.get("tool_calls"))
            .is_some_and(Value::is_array);

        if !has_tool_calls {
            index += 1;
            continue;
        }

        if index > 0 {
            let previous_role = messages
                .get(index - 1)
                .and_then(Value::as_object)
                .and_then(|object| object.get("role"))
                .and_then(Value::as_str)
                .unwrap_or_default();

            if previous_role.eq_ignore_ascii_case("assistant") {
                let previous_content = messages
                    .get(index - 1)
                    .and_then(Value::as_object)
                    .and_then(|object| object.get("content"))
                    .cloned()
                    .unwrap_or(Value::String(String::new()));

                if let Some(message_object) =
                    messages.get_mut(index).and_then(Value::as_object_mut)
                {
                    message_object.insert("content".to_string(), previous_content);
                }

                messages.remove(index - 1);
                continue;
            }
        }

        let primer = messages
            .get(index)
            .and_then(Value::as_object)
            .and_then(|object| object.get("tool_calls"))
            .map(build_tool_call_primer)
            .unwrap_or_else(|| "I'm going to call a tool for that.".to_string());

        if let Some(message_object) = messages.get_mut(index).and_then(Value::as_object_mut) {
            message_object.insert("content".to_string(), Value::String(primer));
        }

        index += 1;
    }
}

fn build_tool_call_primer(tool_calls: &Value) -> String {
    let Some(tool_calls) = tool_calls.as_array() else {
        return "I'm going to call a tool for that.".to_string();
    };

    let mut names = Vec::new();
    for tool_call in tool_calls {
        let Some(function_name) = tool_call
            .get("function")
            .and_then(Value::as_object)
            .and_then(|function| function.get("name"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };

        names.push(function_name.to_string());
    }

    if names.is_empty() {
        "I'm going to call a tool for that.".to_string()
    } else {
        format!("I'm going to call a tool for that: {}", names.join(", "))
    }
}

fn strip_unsupported_names(messages: &mut [Value], names: &PromptNames) {
    for message in messages.iter_mut() {
        let Some(message_object) = message.as_object_mut() else {
            continue;
        };

        let role = message_object
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("user")
            .trim()
            .to_ascii_lowercase();

        let name = message_object
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);

        let Some(name) = name else {
            continue;
        };

        let content_text = message_content_to_text(message_object.get("content"));

        if role == "system" && name == "example_assistant" {
            let prefix = format!("{}: ", names.char_name);
            if !names.char_name.is_empty()
                && !content_text.starts_with(&prefix)
                && !names.starts_with_group_name(&content_text)
            {
                prefix_content(
                    message_object.entry("content").or_insert(Value::Null),
                    &prefix,
                );
            }
        } else if role == "system" && name == "example_user" {
            let prefix = format!("{}: ", names.user_name);
            if !names.user_name.is_empty() && !content_text.starts_with(&prefix) {
                prefix_content(
                    message_object.entry("content").or_insert(Value::Null),
                    &prefix,
                );
            }
        } else if role != "system" {
            let prefix = format!("{name}: ");
            if !content_text.starts_with(&prefix) {
                prefix_content(
                    message_object.entry("content").or_insert(Value::Null),
                    &prefix,
                );
            }
        }

        message_object.remove("name");
    }
}

fn prefix_content(content: &mut Value, prefix: &str) {
    match content {
        Value::String(text) => {
            if !text.starts_with(prefix) {
                *text = format!("{prefix}{text}");
            }
        }
        Value::Array(parts) => {
            for part in parts.iter_mut() {
                match part {
                    Value::String(fragment) => {
                        if !fragment.starts_with(prefix) {
                            *fragment = format!("{prefix}{fragment}");
                        }
                        return;
                    }
                    Value::Object(object) => {
                        if object.get("type").and_then(Value::as_str) != Some("text") {
                            continue;
                        }

                        let Some(text_value) = object.get_mut("text") else {
                            continue;
                        };

                        if let Value::String(fragment) = text_value {
                            if !fragment.starts_with(prefix) {
                                *fragment = format!("{prefix}{fragment}");
                            }
                            return;
                        }
                    }
                    _ => {}
                }
            }

            parts.insert(
                0,
                json!({
                    "type": "text",
                    "text": prefix,
                }),
            );
        }
        Value::Null => {
            *content = Value::String(prefix.to_string());
        }
        _ => {
            let text = content.to_string();
            *content = Value::String(format!("{prefix}{text}"));
        }
    }
}

fn sanitize_openai_tools(tools: &mut [Value]) {
    for tool in tools.iter_mut() {
        let Some(function) = tool
            .as_object_mut()
            .and_then(|object| object.get_mut("function"))
            .and_then(Value::as_object_mut)
        else {
            continue;
        };

        let Some(parameters) = function
            .get_mut("parameters")
            .and_then(Value::as_object_mut)
        else {
            continue;
        };

        parameters.remove("$schema");
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::build;

    #[test]
    fn cohere_build_maps_tools_and_json_schema() {
        let payload = json!({
            "model": "command-r-plus",
            "stream": false,
            "messages": [{"role": "user", "content": "hi"}],
            "top_k": 12,
            "top_p": 0.5,
            "stop": ["END"],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "weather",
                    "parameters": {
                        "$schema": "https://example.com",
                        "type": "object"
                    }
                }
            }],
            "json_schema": { "name": "response", "value": { "type": "object" } }
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (endpoint, upstream) = build(payload).expect("build should succeed");
        assert_eq!(endpoint, "/chat");

        let body = upstream.as_object().expect("upstream must be object");
        assert_eq!(body.get("k"), Some(&json!(12)));
        assert_eq!(body.get("p"), Some(&json!(0.5)));
        assert_eq!(body.get("stop_sequences"), Some(&json!(["END"])));
        assert_eq!(body.get("documents"), Some(&json!([])));

        assert_eq!(body["response_format"]["type"], "json_schema");
        assert!(body["response_format"]["schema"].is_object());

        let parameters = &body["tools"][0]["function"]["parameters"];
        assert!(parameters.get("$schema").is_none());
        assert_eq!(parameters["type"], "object");
    }

    #[test]
    fn cohere_build_sets_safety_mode_off_for_2024_models() {
        let payload = json!({
            "model": "command-r-plus-08-2024",
            "messages": [{"role": "user", "content": "hi"}],
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_endpoint, upstream) = build(payload).expect("build should succeed");
        assert_eq!(
            upstream
                .get("safety_mode")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            "OFF"
        );
    }

    #[test]
    fn cohere_build_forwards_string_stop_sequences() {
        let payload = json!({
            "model": "command-r-plus",
            "messages": [{"role": "user", "content": "hi"}],
            "stop": "END"
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_endpoint, upstream) = build(payload).expect("build should succeed");
        assert_eq!(upstream.get("stop_sequences"), Some(&json!("END")));
    }

    #[test]
    fn cohere_tool_calls_use_previous_assistant_content() {
        let payload = json!({
            "model": "command-r-plus",
            "messages": [
                { "role": "assistant", "content": "previous" },
                { "role": "assistant", "tool_calls": [{ "function": { "name": "weather" } }] }
            ],
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_endpoint, upstream) = build(payload).expect("build should succeed");
        let messages = upstream
            .get("messages")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        assert_eq!(messages.len(), 1);
        assert_eq!(
            messages[0].get("content").and_then(Value::as_str),
            Some("previous")
        );
    }

    #[test]
    fn cohere_tool_calls_fallback_to_primer_string() {
        let payload = json!({
            "model": "command-r-plus",
            "messages": [
                { "role": "assistant", "tool_calls": [{ "function": { "name": "weather" } }] }
            ],
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_endpoint, upstream) = build(payload).expect("build should succeed");
        let messages = upstream
            .get("messages")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        assert_eq!(messages.len(), 1);
        let content = messages[0].get("content").and_then(Value::as_str).unwrap_or("");
        assert!(content.contains("call a tool"));
        assert!(content.contains("weather"));
    }
}
