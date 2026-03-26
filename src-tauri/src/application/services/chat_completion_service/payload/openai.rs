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

const OPENAI_REASONING_EFFORT_MODELS: &[&str] = &[
    "o1",
    "o3-mini",
    "o3-mini-2025-01-31",
    "o4-mini",
    "o4-mini-2025-04-16",
    "o3",
    "o3-2025-04-16",
    "gpt-5",
    "gpt-5-2025-08-07",
    "gpt-5-mini",
    "gpt-5-mini-2025-08-07",
    "gpt-5-nano",
    "gpt-5-nano-2025-08-07",
    "gpt-5.1",
    "gpt-5.1-2025-11-13",
    "gpt-5.1-chat-latest",
    "gpt-5.2",
    "gpt-5.2-2025-12-11",
    "gpt-5.2-chat-latest",
    "gpt-5.4",
    "gpt-5.4-2026-03-05",
    "gpt-5.4-chat-latest",
    "gpt-5.4-mini",
    "gpt-5.4-mini-2026-03-17",
    "gpt-5.4-nano",
    "gpt-5.4-nano-2026-03-17",
];

pub(super) fn build(payload: Map<String, Value>) -> (String, Value) {
    let mut payload = payload;
    let source = payload
        .get("chat_completion_source")
        .and_then(Value::as_str)
        .unwrap_or("openai")
        .trim()
        .to_ascii_lowercase();
    strip_internal_fields(&mut payload);
    build_clean(payload, &source)
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

fn build_clean(payload: Map<String, Value>, source: &str) -> (String, Value) {
    if is_text_completion(&payload) {
        (
            "/completions".to_string(),
            Value::Object(build_text_completion_payload(&payload)),
        )
    } else {
        (
            "/chat/completions".to_string(),
            Value::Object(build_chat_completion_payload(&payload, source)),
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

fn build_chat_completion_payload(payload: &Map<String, Value>, source: &str) -> Map<String, Value> {
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
        "user",
    ] {
        insert_if_present(&mut request, payload, key);
    }

    if let Some(model) = payload.get("model").and_then(Value::as_str) {
        if should_forward_openai_reasoning_effort(source, model) {
            if let Some(reasoning_effort) = payload
                .get("reasoning_effort")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                request.insert(
                    "reasoning_effort".to_string(),
                    Value::String(map_openai_reasoning_effort(reasoning_effort).to_string()),
                );
            }
        }

        if should_forward_openai_verbosity(source, model) {
            if let Some(verbosity) = payload
                .get("verbosity")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                request.insert(
                    "verbosity".to_string(),
                    Value::String(verbosity.to_string()),
                );
            }
        }
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

fn should_forward_openai_reasoning_effort(source: &str, model: &str) -> bool {
    matches!(source, "openai" | "custom") && OPENAI_REASONING_EFFORT_MODELS.contains(&model.trim())
}

fn should_forward_openai_verbosity(source: &str, model: &str) -> bool {
    matches!(source, "openai" | "custom") && model.trim().to_ascii_lowercase().starts_with("gpt-5")
}

fn map_openai_reasoning_effort(value: &str) -> &str {
    if value.eq_ignore_ascii_case("min") {
        "minimal"
    } else {
        value
    }
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

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::build;

    #[test]
    fn custom_payload_does_not_forward_reasoning_effort_for_non_openai_models() {
        let payload = json!({
            "chat_completion_source": "custom",
            "model": "claude-opus-4-5",
            "messages": [{"role": "user", "content": "hello"}],
            "reasoning_effort": "high",
            "verbosity": "high"
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (endpoint, upstream) = build(payload);
        assert_eq!(endpoint, "/chat/completions");

        let body = upstream.as_object().expect("payload must be object");
        assert!(body.get("reasoning_effort").is_none());
        assert!(body.get("verbosity").is_none());
    }

    #[test]
    fn custom_payload_forwards_reasoning_effort_for_supported_openai_models() {
        let payload = json!({
            "chat_completion_source": "custom",
            "model": "gpt-5-2025-08-07",
            "messages": [{"role": "user", "content": "hello"}],
            "reasoning_effort": "min"
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_endpoint, upstream) = build(payload);
        let body = upstream.as_object().expect("payload must be object");
        assert_eq!(
            body.get("reasoning_effort")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            "minimal"
        );
    }

    #[test]
    fn custom_payload_forwards_verbosity_only_for_gpt5_models() {
        let payload = json!({
            "chat_completion_source": "custom",
            "model": "gpt-5-mini",
            "messages": [{"role": "user", "content": "hello"}],
            "verbosity": "low"
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_endpoint, upstream) = build(payload);
        let body = upstream.as_object().expect("payload must be object");
        assert_eq!(
            body.get("verbosity")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            "low"
        );
    }

    #[test]
    fn non_custom_sources_do_not_forward_reasoning_effort_or_verbosity() {
        let payload = json!({
            "chat_completion_source": "openrouter",
            "model": "gpt-5",
            "messages": [{"role": "user", "content": "hello"}],
            "reasoning_effort": "high",
            "verbosity": "high"
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_endpoint, upstream) = build(payload);
        let body = upstream.as_object().expect("payload must be object");
        assert!(body.get("reasoning_effort").is_none());
        assert!(body.get("verbosity").is_none());
    }
}
