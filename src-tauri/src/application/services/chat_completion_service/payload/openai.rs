use serde_json::{Map, Value};

use super::shared::{insert_if_present, message_content_to_text};

const TEXT_COMPLETION_MODELS: &[&str] = &[
    "gpt-3.5-turbo-instruct",
    "gpt-3.5-turbo-instruct-0914",
    "text-davinci-003",
    "text-davinci-002",
    "text-davinci-001",
    "text-curie-001",
    "text-babbage-001",
    "text-ada-001",
    "code-davinci-002",
    "code-davinci-001",
    "code-cushman-002",
    "code-cushman-001",
    "text-davinci-edit-001",
    "code-davinci-edit-001",
    "text-embedding-ada-002",
    "text-similarity-davinci-001",
    "text-similarity-curie-001",
    "text-similarity-babbage-001",
    "text-similarity-ada-001",
    "text-search-davinci-doc-001",
    "text-search-curie-doc-001",
    "text-search-babbage-doc-001",
    "text-search-ada-doc-001",
    "code-search-babbage-code-001",
    "code-search-ada-code-001",
];

pub(super) fn build(payload: Map<String, Value>) -> (String, Value) {
    let mut payload = payload;
    strip_internal_fields(&mut payload);
    build_clean(payload)
}

pub(super) fn strip_internal_fields(payload: &mut Map<String, Value>) {
    for key in [
        "chat_completion_source",
        "reverse_proxy",
        "proxy_password",
        "custom_prompt_post_processing",
        "custom_include_body",
        "custom_exclude_body",
        "custom_include_headers",
        "custom_url",
        "bypass_status_check",
    ] {
        payload.remove(key);
    }
}

fn build_clean(payload: Map<String, Value>) -> (String, Value) {
    if is_text_completion(&payload) {
        (
            "/completions".to_string(),
            Value::Object(build_text_completion_payload(&payload)),
        )
    } else {
        (
            "/chat/completions".to_string(),
            Value::Object(build_chat_completion_payload(&payload)),
        )
    }
}

fn build_text_completion_payload(payload: &Map<String, Value>) -> Map<String, Value> {
    let mut request = Map::new();

    for key in [
        "model",
        "temperature",
        "max_tokens",
        "stream",
        "presence_penalty",
        "frequency_penalty",
        "top_p",
        "stop",
        "logit_bias",
        "seed",
        "n",
        "logprobs",
    ] {
        insert_if_present(&mut request, payload, key);
    }

    if let Some(prompt) = payload
        .get("prompt")
        .cloned()
        .filter(|value| !value.is_null())
    {
        request.insert("prompt".to_string(), prompt);
        return request;
    }

    if let Some(messages) = payload.get("messages") {
        if let Some(prompt) = convert_text_completion_prompt(messages) {
            request.insert("prompt".to_string(), Value::String(prompt));
        }
    }

    request
}

fn build_chat_completion_payload(payload: &Map<String, Value>) -> Map<String, Value> {
    let mut request = Map::new();

    for key in [
        "messages",
        "model",
        "temperature",
        "max_tokens",
        "max_completion_tokens",
        "stream",
        "presence_penalty",
        "frequency_penalty",
        "top_p",
        "top_k",
        "stop",
        "logit_bias",
        "seed",
        "n",
        "reasoning_effort",
        "verbosity",
        "user",
    ] {
        insert_if_present(&mut request, payload, key);
    }

    if let Some(tools) = payload.get("tools").filter(|value| value.is_array()) {
        request.insert("tools".to_string(), tools.clone());
        insert_if_present(&mut request, payload, "tool_choice");
    }

    map_chat_logprobs(&mut request, payload);

    if let Some(response_format) = resolve_response_format(payload) {
        request.insert("response_format".to_string(), response_format);
    }

    request
}

fn map_chat_logprobs(request: &mut Map<String, Value>, payload: &Map<String, Value>) {
    let Some(logprobs) = payload.get("logprobs") else {
        return;
    };

    if let Some(raw_number) = logprobs.as_i64() {
        if raw_number > 0 {
            request.insert("logprobs".to_string(), Value::Bool(true));
            request.insert(
                "top_logprobs".to_string(),
                Value::Number(serde_json::Number::from(raw_number)),
            );
        }
        return;
    }

    if let Some(raw_number) = logprobs.as_u64() {
        if raw_number > 0 {
            request.insert("logprobs".to_string(), Value::Bool(true));
            request.insert(
                "top_logprobs".to_string(),
                Value::Number(serde_json::Number::from(raw_number)),
            );
        }
        return;
    }

    if let Some(raw_number) = logprobs.as_f64() {
        if raw_number > 0.0 {
            request.insert("logprobs".to_string(), Value::Bool(true));
            if let Some(number) = serde_json::Number::from_f64(raw_number) {
                request.insert("top_logprobs".to_string(), Value::Number(number));
            }
        }
        return;
    }

    if let Some(enabled) = logprobs.as_bool() {
        request.insert("logprobs".to_string(), Value::Bool(enabled));
        if enabled {
            insert_if_present(request, payload, "top_logprobs");
        }
    }
}

fn resolve_response_format(payload: &Map<String, Value>) -> Option<Value> {
    payload
        .get("response_format")
        .cloned()
        .filter(|value| !value.is_null())
        .or_else(|| build_response_format_from_json_schema(payload))
}

fn build_response_format_from_json_schema(payload: &Map<String, Value>) -> Option<Value> {
    let json_schema = payload.get("json_schema")?.as_object()?;
    let schema_value = json_schema.get("value")?.clone();
    if schema_value.is_null() {
        return None;
    }

    let mut json_schema_object = Map::new();
    json_schema_object.insert(
        "name".to_string(),
        json_schema
            .get("name")
            .cloned()
            .unwrap_or_else(|| Value::String("response".to_string())),
    );
    json_schema_object.insert(
        "strict".to_string(),
        json_schema
            .get("strict")
            .cloned()
            .unwrap_or(Value::Bool(true)),
    );
    json_schema_object.insert("schema".to_string(), schema_value);

    let mut response_format = Map::new();
    response_format.insert("type".to_string(), Value::String("json_schema".to_string()));
    response_format.insert("json_schema".to_string(), Value::Object(json_schema_object));

    Some(Value::Object(response_format))
}

fn convert_text_completion_prompt(messages: &Value) -> Option<String> {
    if let Some(prompt) = messages.as_str() {
        return Some(prompt.to_string());
    }

    let entries = messages.as_array()?;
    if entries.is_empty() {
        return None;
    }

    let mut lines = Vec::with_capacity(entries.len());
    for entry in entries {
        let Some(message) = entry.as_object() else {
            continue;
        };

        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("user")
            .trim();
        let name = message.get("name").and_then(Value::as_str).map(str::trim);
        let content = message_content_to_text(message.get("content"));

        if role.eq_ignore_ascii_case("system") {
            match name {
                Some(value) if !value.is_empty() => {
                    lines.push(format!("{value}: {content}"));
                }
                _ => {
                    lines.push(format!("System: {content}"));
                }
            }
        } else {
            lines.push(format!("{role}: {content}"));
        }
    }

    if lines.is_empty() {
        None
    } else {
        Some(format!("{}\nassistant:", lines.join("\n")))
    }
}

fn is_text_completion(payload: &Map<String, Value>) -> bool {
    let messages_is_string = payload.get("messages").is_some_and(Value::is_string);
    if messages_is_string {
        return true;
    }

    payload
        .get("model")
        .and_then(Value::as_str)
        .is_some_and(|model| TEXT_COMPLETION_MODELS.contains(&model))
}
