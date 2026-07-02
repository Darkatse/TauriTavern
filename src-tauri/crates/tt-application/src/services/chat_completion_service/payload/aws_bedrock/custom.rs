//! AWS Bedrock custom invoke template — escape hatch for the long tail of
//! Bedrock-hosted models we haven't wired a first-class builder for (Titan
//! Text, Stability, Writer, Twelve Labs custom variants, future Anthropic
//! / Nova add-ons, ...).
//!
//! Activated by setting `aws_bedrock_use_custom_template = true` and
//! supplying a body template plus response JSON path on the request. Streaming
//! requests must also provide a stream JSON path. The template is plain JSON
//! with five placeholders that get
//! substituted with the current request's data:
//!
//! - `{{messages}}` — JSON-encoded `[{"role":"...","content":"..."}, ...]`
//!   array produced by flattening OpenAI-style content blocks.
//! - `{{system}}` — JSON-encoded string with the concatenated system prompt
//!   (empty string when no system message is present).
//! - `{{max_tokens}}` — JSON number (defaults to `1024` when unset).
//! - `{{temperature}}` — JSON number (defaults to `0.7` when unset).
//! - `{{model}}` — JSON-encoded model id string.
//!
//! The endpoint remains the regular `/model/{model_id}/invoke` path the
//! infrastructure layer already speaks. Response / stream extraction is then
//! driven by required user-provided JSON paths surfaced through
//! [`tt_ports::repositories::chat_completion_repository::ChatCompletionApiConfig`].

use serde_json::{Map, Value, json};

use super::super::shared::message_content_to_text;
use super::shared::{
    BEDROCK_INVOKE_SUFFIX, FlatMessage, flatten_openai_messages, value_to_positive_i64,
};
use crate::errors::ApplicationError;

/// Default `max_tokens` baked into the template when the request omits one.
/// Mirrors the application-layer baseline used by the Anthropic builder so
/// the custom path doesn't surprise users with a different cap.
const DEFAULT_MAX_TOKENS: i64 = 1024;
/// Default `temperature` baked into the template when the request omits one.
/// Matches the OpenAI / Bedrock default so the placeholder never resolves
/// to `null`.
const DEFAULT_TEMPERATURE: f64 = 0.7;

pub(super) fn build(
    payload: Map<String, Value>,
    model_id: &str,
) -> Result<(String, Value), ApplicationError> {
    let template = payload
        .get("aws_bedrock_custom_template")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            ApplicationError::ValidationError(
                "AWS Bedrock custom template is enabled but `aws_bedrock_custom_template` is empty.".to_string(),
            )
        })?;
    require_non_empty_path(&payload, "aws_bedrock_custom_response_path")?;
    if payload
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        require_non_empty_path(&payload, "aws_bedrock_custom_stream_path")?;
    }

    let messages_json = render_messages(payload.get("messages"));
    let system_json = render_system(payload.get("messages"));
    let max_tokens_json = render_max_tokens(payload.get("max_tokens"));
    let temperature_json = render_temperature(payload.get("temperature"));
    let model_json = serde_json::to_string(model_id).map_err(|error| {
        ApplicationError::InternalError(format!(
            "Failed to JSON-encode AWS Bedrock model id: {error}"
        ))
    })?;

    let rendered = template
        .replace("{{messages}}", &messages_json)
        .replace("{{system}}", &system_json)
        .replace("{{max_tokens}}", &max_tokens_json)
        .replace("{{temperature}}", &temperature_json)
        .replace("{{model}}", &model_json);

    let body: Value = serde_json::from_str(&rendered).map_err(|error| {
        ApplicationError::ValidationError(format!(
            "AWS Bedrock custom template rendered to invalid JSON: {error}. Rendered body: {rendered}"
        ))
    })?;

    let endpoint = format!("/model/{model_id}/{BEDROCK_INVOKE_SUFFIX}");
    Ok((endpoint, body))
}

fn render_messages(messages: Option<&Value>) -> String {
    let (_, turns) = flatten_openai_messages(messages);
    let payload: Vec<Value> = turns
        .into_iter()
        .map(|FlatMessage { role, text }| json!({ "role": role, "content": text }))
        .collect();
    serde_json::to_string(&payload).expect("flattened messages are always JSON-serializable")
}

fn require_non_empty_path(payload: &Map<String, Value>, key: &str) -> Result<(), ApplicationError> {
    match payload.get(key) {
        Some(Value::String(value)) if !value.trim().is_empty() => Ok(()),
        Some(Value::String(_)) | None => Err(ApplicationError::ValidationError(format!(
            "AWS Bedrock custom template is enabled but `{key}` is empty."
        ))),
        Some(_) => Err(ApplicationError::ValidationError(format!(
            "AWS Bedrock custom template field `{key}` must be a string."
        ))),
    }
}

fn render_system(messages: Option<&Value>) -> String {
    let system_text = messages
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter(|entry| {
                    entry
                        .get("role")
                        .and_then(Value::as_str)
                        .map(|role| {
                            let lower = role.trim().to_ascii_lowercase();
                            lower == "system" || lower == "developer"
                        })
                        .unwrap_or(false)
                })
                .map(|entry| message_content_to_text(entry.get("content")))
                .filter(|text| !text.is_empty())
                .collect::<Vec<_>>()
                .join("\n\n")
        })
        .unwrap_or_default();
    serde_json::to_string(&system_text).expect("system prompt is always JSON-serializable")
}

fn render_max_tokens(value: Option<&Value>) -> String {
    let resolved = value_to_positive_i64(value).unwrap_or(DEFAULT_MAX_TOKENS);
    resolved.to_string()
}

fn render_temperature(value: Option<&Value>) -> String {
    let resolved = value
        .and_then(Value::as_f64)
        .filter(|number| number.is_finite() && *number >= 0.0)
        .unwrap_or(DEFAULT_TEMPERATURE);
    // serde_json keeps trailing `.0` for whole-number floats; we want the
    // most-compact JSON-legal form so the resulting body parses cleanly even
    // for `0` / `1` / `2`. `Value::Number::from_f64` does the right thing for
    // both flavours.
    Value::Number(serde_json::Number::from_f64(resolved).unwrap_or_else(|| {
        serde_json::Number::from_f64(DEFAULT_TEMPERATURE)
            .expect("0.7 is always representable as JSON number")
    }))
    .to_string()
}

/// Check whether the request opted into the custom-template path. Lives next
/// to the builder so dispatch logic doesn't need to re-implement the
/// truthiness rule (`true` / `"true"` / `1`).
pub(super) fn is_enabled(payload: &Map<String, Value>) -> bool {
    let Some(value) = payload.get("aws_bedrock_use_custom_template") else {
        return false;
    };
    match value {
        Value::Bool(flag) => *flag,
        Value::String(text) => matches!(text.trim().to_ascii_lowercase().as_str(), "true" | "1"),
        Value::Number(number) => number.as_u64().is_some_and(|n| n != 0),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{build, is_enabled};

    #[test]
    fn is_enabled_treats_truthy_variants_uniformly() {
        let mut payload = serde_json::Map::new();
        assert!(!is_enabled(&payload));
        payload.insert(
            "aws_bedrock_use_custom_template".to_string(),
            Value::Bool(false),
        );
        assert!(!is_enabled(&payload));
        payload.insert(
            "aws_bedrock_use_custom_template".to_string(),
            Value::Bool(true),
        );
        assert!(is_enabled(&payload));
        payload.insert(
            "aws_bedrock_use_custom_template".to_string(),
            Value::String("true".to_string()),
        );
        assert!(is_enabled(&payload));
        payload.insert(
            "aws_bedrock_use_custom_template".to_string(),
            Value::String("0".to_string()),
        );
        assert!(!is_enabled(&payload));
    }

    #[test]
    fn build_substitutes_placeholders_and_returns_invoke_endpoint() {
        let payload = json!({
            "model": "writer.palmyra-x-004-v1:0",
            "aws_bedrock_use_custom_template": true,
            "aws_bedrock_custom_template":
                "{\"model\":{{model}},\"system\":{{system}},\"messages\":{{messages}},\"max_tokens\":{{max_tokens}},\"temperature\":{{temperature}}}",
            "aws_bedrock_custom_response_path": "output.text",
            "messages": [
                { "role": "system", "content": "be terse" },
                { "role": "user", "content": "ping" },
                { "role": "assistant", "content": "pong" },
                { "role": "user", "content": "again" }
            ],
            "max_tokens": 256,
            "temperature": 0.4
        })
        .as_object()
        .cloned()
        .unwrap();

        let (endpoint, body) = build(payload, "writer.palmyra-x-004-v1:0")
            .expect("custom template must build successfully");

        assert_eq!(endpoint, "/model/writer.palmyra-x-004-v1:0/invoke");
        assert_eq!(body["model"], "writer.palmyra-x-004-v1:0");
        assert_eq!(body["system"], "be terse");
        assert_eq!(body["max_tokens"], 256);
        assert!(body["temperature"].as_f64().unwrap().eq(&0.4));

        let messages = body["messages"].as_array().expect("messages must be array");
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[0]["content"], "ping");
        assert_eq!(messages[1]["role"], "assistant");
        assert_eq!(messages[2]["role"], "user");
        assert_eq!(messages[2]["content"], "again");
    }

    #[test]
    fn build_fails_when_template_is_missing_or_empty() {
        let payload = json!({
            "model": "writer.palmyra-x-004-v1:0",
            "aws_bedrock_use_custom_template": true,
            "messages": [{ "role": "user", "content": "hi" }],
        })
        .as_object()
        .cloned()
        .unwrap();
        let err =
            build(payload, "writer.palmyra-x-004-v1:0").expect_err("missing template must fail");
        assert!(err.to_string().contains("custom template is enabled but"));
    }

    #[test]
    fn build_fails_when_rendered_template_is_not_valid_json() {
        let payload = json!({
            "model": "writer.palmyra-x-004-v1:0",
            "aws_bedrock_use_custom_template": true,
            // Unbalanced braces, even after substitution.
            "aws_bedrock_custom_template": "{\"messages\":{{messages}}",
            "aws_bedrock_custom_response_path": "output.text",
            "messages": [{ "role": "user", "content": "hi" }],
        })
        .as_object()
        .cloned()
        .unwrap();

        let err = build(payload, "writer.palmyra-x-004-v1:0")
            .expect_err("invalid JSON must fail with a clear error");
        assert!(err.to_string().contains("rendered to invalid JSON"));
    }

    #[test]
    fn build_uses_defaults_for_missing_max_tokens_and_temperature() {
        let payload = json!({
            "model": "writer.palmyra-x-004-v1:0",
            "aws_bedrock_use_custom_template": true,
            "aws_bedrock_custom_template":
                "{\"max_tokens\":{{max_tokens}},\"temperature\":{{temperature}}}",
            "aws_bedrock_custom_response_path": "output.text",
            "messages": [{ "role": "user", "content": "hi" }],
        })
        .as_object()
        .cloned()
        .unwrap();

        let (_, body) =
            build(payload, "writer.palmyra-x-004-v1:0").expect("template must build with defaults");
        assert_eq!(body["max_tokens"], 1024);
        assert!(body["temperature"].as_f64().unwrap().eq(&0.7));
    }

    #[test]
    fn build_requires_response_path_when_custom_template_is_enabled() {
        let payload = json!({
            "model": "writer.palmyra-x-004-v1:0",
            "aws_bedrock_use_custom_template": true,
            "aws_bedrock_custom_template": "{\"messages\":{{messages}}}",
            "messages": [{ "role": "user", "content": "hi" }],
        })
        .as_object()
        .cloned()
        .unwrap();

        let err = build(payload, "writer.palmyra-x-004-v1:0")
            .expect_err("missing response path must fail");
        assert!(err.to_string().contains("aws_bedrock_custom_response_path"));
    }

    #[test]
    fn build_requires_stream_path_for_custom_streaming_requests() {
        let payload = json!({
            "model": "writer.palmyra-x-004-v1:0",
            "aws_bedrock_use_custom_template": true,
            "aws_bedrock_custom_template": "{\"messages\":{{messages}}}",
            "aws_bedrock_custom_response_path": "output.text",
            "stream": true,
            "messages": [{ "role": "user", "content": "hi" }],
        })
        .as_object()
        .cloned()
        .unwrap();

        let err =
            build(payload, "writer.palmyra-x-004-v1:0").expect_err("missing stream path must fail");
        assert!(err.to_string().contains("aws_bedrock_custom_stream_path"));
    }
}
