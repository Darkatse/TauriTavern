use serde_json::{Map, Value};

use crate::application::errors::ApplicationError;

use super::claude;

pub(super) fn build(payload: Map<String, Value>) -> Result<(String, Value), ApplicationError> {
    claude::build_passthrough(payload)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::build;

    #[test]
    fn claude_messages_leaves_exclude_to_service_layer() {
        let payload = json!({
            "model": "claude-opus-4.6",
            "messages": [{"role": "user", "content": "hello"}],
            "top_p": 0.8,
            "custom_exclude_body": "- top_p"
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_endpoint, upstream) = build(payload).expect("build should succeed");
        let body = upstream.as_object().expect("body must be object");

        assert_eq!(
            body.get("top_p").and_then(serde_json::Value::as_f64),
            Some(0.8)
        );
    }

    #[test]
    fn claude_messages_passthrough_keeps_user_sampling_params() {
        let payload = json!({
            "model": "claude-opus-4-7",
            "messages": [{"role": "user", "content": "hello"}],
            "top_k": 40
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_endpoint, upstream) = build(payload).expect("build should succeed");
        let body = upstream.as_object().expect("body must be object");

        assert_eq!(
            body.get("top_k").and_then(serde_json::Value::as_i64),
            Some(40)
        );
    }
}
