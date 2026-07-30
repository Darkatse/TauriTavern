use serde_json::{Map, Value};

use crate::errors::ApplicationError;
use tt_domain::models::claude_model::{
    is_vertex_ai_claude_model_id, normalize_vertex_ai_claude_model_id,
};

use super::{claude, makersuite};

const VERTEXAI_ANTHROPIC_VERSION: &str = "vertex-2023-10-16";

pub(super) fn build(payload: Map<String, Value>) -> Result<(String, Value), ApplicationError> {
    let model_id = payload
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
        .to_string();

    if is_vertex_ai_claude_model_id(&model_id) {
        build_claude_on_vertex(payload, &model_id)
    } else {
        makersuite::build_vertexai(payload)
    }
}

fn build_claude_on_vertex(
    mut payload: Map<String, Value>,
    model_id: &str,
) -> Result<(String, Value), ApplicationError> {
    let raw_model_id = model_id.trim().to_ascii_lowercase();
    let normalized_model_id = normalize_vertex_ai_claude_model_id(&raw_model_id);
    payload.insert("model".to_string(), Value::String(normalized_model_id));

    let (_, request) = claude::build(payload)?;
    let mut request_object = match request {
        Value::Object(map) => map,
        _ => {
            return Err(ApplicationError::InternalError(
                "Claude payload builder returned a non-object request".to_string(),
            ));
        }
    };

    reject_non_base64_image_sources(&request_object)?;

    request_object.remove("model");
    request_object.insert(
        "anthropic_version".to_string(),
        Value::String(VERTEXAI_ANTHROPIC_VERSION.to_string()),
    );

    Ok((
        format!("/publishers/anthropic/models/{raw_model_id}:rawPredict"),
        Value::Object(request_object),
    ))
}

fn reject_non_base64_image_sources(request: &Map<String, Value>) -> Result<(), ApplicationError> {
    let Some(messages) = request.get("messages").and_then(Value::as_array) else {
        return Ok(());
    };

    for message in messages {
        let Some(content) = message.get("content").and_then(Value::as_array) else {
            continue;
        };

        for block in content {
            if !is_claude_image_block(block) {
                continue;
            }

            let source_type = block
                .get("source")
                .and_then(Value::as_object)
                .and_then(|source| source.get("type"))
                .and_then(Value::as_str)
                .map(str::trim);

            if source_type != Some("base64") {
                return Err(ApplicationError::ValidationError(
                    "Google Vertex AI Claude only supports base64 image sources; send a data URL instead of a remote URL or provider file reference."
                        .to_string(),
                ));
            }
        }
    }

    Ok(())
}

fn is_claude_image_block(value: &Value) -> bool {
    value
        .as_object()
        .and_then(|object| object.get("type"))
        .and_then(Value::as_str)
        .is_some_and(|entry| entry.trim() == "image")
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::build;

    #[test]
    fn vertex_claude_rewrites_to_raw_predict_and_injects_vertex_version() {
        let payload = json!({
            "chat_completion_source": "vertexai",
            "model": "Claude-Sonnet-4-5@20250929",
            "messages": [{ "role": "user", "content": "hello" }],
            "stream": true,
            "max_tokens": 1024,
        })
        .as_object()
        .cloned()
        .expect("payload should be object");

        let (endpoint_path, body) = build(payload).expect("payload should build");
        assert_eq!(
            endpoint_path,
            "/publishers/anthropic/models/claude-sonnet-4-5@20250929:rawPredict"
        );

        let body = body.as_object().expect("body should be object");
        assert!(body.get("model").is_none());
        assert_eq!(
            body.get("anthropic_version").and_then(Value::as_str),
            Some("vertex-2023-10-16"),
        );
        assert_eq!(body.get("stream").and_then(Value::as_bool), Some(true));
    }

    #[test]
    fn vertex_claude_unlocks_contract_features_via_normalization() {
        let payload = json!({
            "chat_completion_source": "vertexai",
            "model": "claude-opus-4-7",
            "messages": [{ "role": "user", "content": "hi" }],
            "max_tokens": 4096,
            "reasoning_effort": "xhigh",
        })
        .as_object()
        .cloned()
        .expect("payload should be object");

        let (_, body) = build(payload).expect("payload should build");
        assert_eq!(
            body.pointer("/thinking/type").and_then(Value::as_str),
            Some("adaptive")
        );
        assert_eq!(
            body.pointer("/output_config/effort")
                .and_then(Value::as_str),
            Some("xhigh")
        );
    }

    #[test]
    fn vertex_claude_rejects_non_base64_image_sources() {
        let payload = json!({
            "chat_completion_source": "vertexai",
            "model": "claude-sonnet-4-5@20250929",
            "messages": [{
                "role": "user",
                "content": [{
                    "type": "image_url",
                    "image_url": { "url": "https://example.test/cat.png" }
                }]
            }],
            "max_tokens": 1024,
        })
        .as_object()
        .cloned()
        .expect("payload should be object");

        let error = build(payload).expect_err("remote URL image source should fail");
        assert!(
            error
                .to_string()
                .contains("Google Vertex AI Claude only supports base64 image sources"),
            "{error}"
        );
    }
}
