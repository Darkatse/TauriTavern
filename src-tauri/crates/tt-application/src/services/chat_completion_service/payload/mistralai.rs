use std::iter;

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha512};

use crate::services::hashing::hex_lower;

use super::prompt_post_processing::{PromptNames, fold_speaker_name};
use super::shared::{insert_if_present, json_schema_fields};

const MISTRALAI_ENDPOINT_PATH: &str = "/chat/completions";
// Mistral rejects fields it does not define, in the request and in each message.
const MISTRALAI_REQUEST_FIELDS: &[&str] = &[
    "messages",
    "model",
    "temperature",
    "top_p",
    "frequency_penalty",
    "presence_penalty",
    "max_tokens",
    "stream",
    "safe_prompt",
];
const MISTRALAI_MESSAGE_FIELDS: &[&str] = &[
    "role",
    "content",
    "prefix",
    "tool_calls",
    "tool_call_id",
    "name",
];
const MISTRALAI_TOOL_CALL_FIELDS: &[&str] = &["id", "type", "function"];

pub(super) fn build(mut payload: Map<String, Value>) -> (String, Value) {
    let names = PromptNames::from_payload(&payload);
    if let Some(messages) = payload.get_mut("messages").and_then(Value::as_array_mut) {
        *messages = convert_messages(std::mem::take(messages), &names);
    }

    let mut request = Map::new();
    for key in MISTRALAI_REQUEST_FIELDS {
        insert_if_present(&mut request, &payload, key);
    }
    if let Some(seed) = payload.get("seed").filter(|seed| !seed.is_null()) {
        request.insert("random_seed".to_string(), seed.clone());
    }
    if let Some(stop) = payload
        .get("stop")
        .filter(|stop| stop.as_array().is_some_and(|stop| !stop.is_empty()))
    {
        request.insert("stop".to_string(), stop.clone());
    }
    if payload
        .get("tools")
        .and_then(Value::as_array)
        .is_some_and(|tools| !tools.is_empty())
    {
        insert_if_present(&mut request, &payload, "tools");
        insert_if_present(&mut request, &payload, "tool_choice");
    }
    if let Some(schema) = json_schema_fields(&payload) {
        request.insert(
            "response_format".to_string(),
            json!({ "type": "json_schema", "json_schema": schema }),
        );
    }

    (MISTRALAI_ENDPOINT_PATH.to_string(), Value::Object(request))
}

fn convert_messages(messages: Vec<Value>, names: &PromptNames) -> Vec<Value> {
    let mut converted: Vec<Value> = Vec::with_capacity(messages.len());
    for mut message in messages {
        if let Some(object) = message.as_object_mut() {
            fold_speaker_name(object, names);
            to_mistral_message(object);
        }

        // Mistral rejects system after assistant and user after a tool result.
        let previous_role = converted.last().and_then(message_role);
        match message_role(&message) {
            Some("system") if previous_role == Some("assistant") => {
                message["role"] = json!("user");
            }
            Some("user") if previous_role == Some("tool") => {
                converted.push(json!({ "role": "assistant", "content": " " }));
            }
            _ => {}
        }
        converted.push(message);
    }
    converted
}

fn message_role(message: &Value) -> Option<&str> {
    message.get("role").and_then(Value::as_str)
}

fn to_mistral_message(message: &mut Map<String, Value>) {
    let reasoning = message.remove("reasoning_content");
    message.retain(|key, _| MISTRALAI_MESSAGE_FIELDS.contains(&key.as_str()));
    if message.get("role").and_then(Value::as_str) == Some("assistant")
        && let Some(Value::String(reasoning)) = reasoning
        && !reasoning.is_empty()
    {
        prepend_thinking(message, reasoning);
    }

    if let Some(Value::String(id)) = message.get_mut("tool_call_id") {
        *id = mistral_tool_call_id(id);
    }
    for call in message
        .get_mut("tool_calls")
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object_mut)
    {
        call.retain(|key, _| MISTRALAI_TOOL_CALL_FIELDS.contains(&key.as_str()));
        if let Some(Value::String(id)) = call.get_mut("id") {
            *id = mistral_tool_call_id(id);
        }
    }
}

// Mistral expects replayed reasoning before the assistant text.
fn prepend_thinking(message: &mut Map<String, Value>, reasoning: String) {
    let thinking =
        json!({ "type": "thinking", "thinking": [{ "type": "text", "text": reasoning }] });
    let content = match message.remove("content") {
        Some(Value::Array(parts)) => iter::once(thinking).chain(parts).collect(),
        Some(Value::String(text)) if !text.is_empty() => {
            vec![thinking, json!({ "type": "text", "text": text })]
        }
        _ => vec![thinking],
    };
    message.insert("content".to_string(), Value::Array(content));
}

// Mistral accepts only nine alphanumeric characters as a tool call id.
fn mistral_tool_call_id(id: &str) -> String {
    hex_lower(&Sha512::digest(id.as_bytes()))[..9].to_string()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::build;

    #[test]
    fn mistral_gets_only_its_own_fields_speaker_names_reasoning_and_turn_order() {
        let image =
            json!({ "type": "image_url", "image_url": { "url": "data:image/png;base64,AAAA" } });
        let payload = json!({
            "chat_completion_source": "mistralai",
            "model": "mistral-large-latest",
            "char_name": "Aqua",
            "user_name": "Kazuma",
            "messages": [
                { "role": "system", "name": "example_user", "content": "hi" },
                { "role": "user", "name": "Kazuma", "content": [{ "type": "text", "text": "look" }, image] },
                { "role": "user", "name": "Darkness", "content": [image] },
                {
                    "role": "assistant",
                    "content": null,
                    "reasoning_content": "Check the weather.",
                    "reasoning_details": [],
                    "tool_calls": [{
                        "id": "call_1", "type": "function", "extra_content": {},
                        "function": { "name": "weather", "arguments": "{}" }
                    }]
                },
                { "role": "tool", "tool_call_id": "call_1", "name": "weather", "content": "sunny" },
                { "role": "user", "content": "thanks" },
                { "role": "assistant", "content": "welcome", "reasoning_content": "" },
                { "role": "system", "content": "stay brief" }
            ],
            "seed": 7,
            "stop": [],
            "logit_bias": {},
            "stream": true,
            "safe_prompt": false
        });

        let (endpoint, body) = build(payload.as_object().cloned().unwrap());

        assert_eq!(endpoint, "/chat/completions");
        assert_eq!(
            body,
            json!({
                "model": "mistral-large-latest",
                "messages": [
                    { "role": "system", "content": "Kazuma: hi" },
                    { "role": "user", "content": [{ "type": "text", "text": "Kazuma: look" }, image] },
                    { "role": "user", "content": [{ "type": "text", "text": "Darkness: " }, image] },
                    {
                        "role": "assistant",
                        "content": [{
                            "type": "thinking",
                            "thinking": [{ "type": "text", "text": "Check the weather." }]
                        }],
                        "tool_calls": [{
                            "id": "27d3f0eef", "type": "function",
                            "function": { "name": "weather", "arguments": "{}" }
                        }]
                    },
                    { "role": "tool", "tool_call_id": "27d3f0eef", "name": "weather", "content": "sunny" },
                    { "role": "assistant", "content": " " },
                    { "role": "user", "content": "thanks" },
                    { "role": "assistant", "content": "welcome" },
                    { "role": "user", "content": "stay brief" }
                ],
                "stream": true,
                "safe_prompt": false,
                "random_seed": 7
            })
        );
    }
}
