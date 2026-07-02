use serde_json::{Map, Value, json};

use crate::errors::ApplicationError;

use super::openai;
use super::shared::insert_if_present;

pub(super) fn build(payload: Map<String, Value>) -> Result<(String, Value), ApplicationError> {
    let source_payload = payload.clone();
    let (endpoint, mut upstream_payload) = openai::build(payload);

    if endpoint != "/chat/completions" {
        return Err(ApplicationError::ValidationError(
            "Cloudflare Workers AI only supports /chat/completions in this build.".to_string(),
        ));
    }

    let Some(body) = upstream_payload.as_object_mut() else {
        return Ok((endpoint, upstream_payload));
    };

    insert_if_present(body, &source_payload, "repetition_penalty");

    if let Some(schema_value) = source_payload
        .get("json_schema")
        .and_then(Value::as_object)
        .and_then(|schema| schema.get("value"))
        .cloned()
        .filter(|value| !value.is_null())
    {
        body.insert(
            "response_format".to_string(),
            json!({
                "type": "json_schema",
                "json_schema": schema_value,
            }),
        );
    }

    Ok((endpoint, upstream_payload))
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::build;

    #[test]
    fn workers_ai_uses_cloudflare_json_schema_shape() {
        let payload = json!({
            "chat_completion_source": "workers_ai",
            "model": "@cf/meta/llama",
            "messages": [{"role": "user", "content": "hello"}],
            "json_schema": {
                "name": "ignored",
                "value": {
                    "type": "object",
                    "properties": {
                        "answer": { "type": "string" }
                    }
                }
            }
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (endpoint, upstream) = build(payload).expect("build must succeed");
        assert_eq!(endpoint, "/chat/completions");
        assert_eq!(
            upstream.pointer("/response_format/json_schema/type"),
            Some(&Value::String("object".to_string()))
        );
    }

    #[test]
    fn workers_ai_forwards_repetition_penalty() {
        let payload = json!({
            "chat_completion_source": "workers_ai",
            "model": "@cf/meta/llama",
            "messages": [{"role": "user", "content": "hello"}],
            "repetition_penalty": 1.1
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_endpoint, upstream) = build(payload).expect("build must succeed");
        assert_eq!(
            upstream.get("repetition_penalty").and_then(Value::as_f64),
            Some(1.1)
        );
    }
}
