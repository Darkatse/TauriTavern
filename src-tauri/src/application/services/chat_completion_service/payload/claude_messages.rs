use serde_json::{Map, Value};

use crate::application::errors::ApplicationError;

use super::claude;
use super::shared::apply_custom_body_overrides;

pub(super) fn build(payload: Map<String, Value>) -> Result<(String, Value), ApplicationError> {
    let include_raw = payload
        .get("custom_include_body")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let exclude_raw = payload
        .get("custom_exclude_body")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    let (endpoint, mut upstream_payload) = claude::build_passthrough(payload)?;
    apply_custom_body_overrides(&mut upstream_payload, &include_raw, &exclude_raw)?;

    Ok((endpoint, upstream_payload))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::build;

    #[test]
    fn claude_messages_exclude_runs_without_claude_contract_check() {
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

        assert!(body.get("top_p").is_none());
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
