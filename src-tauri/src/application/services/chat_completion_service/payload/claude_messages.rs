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

    let (endpoint, mut upstream_payload) = claude::build(payload)?;
    apply_custom_body_overrides(&mut upstream_payload, &include_raw, &exclude_raw)?;
    claude::validate_request(&upstream_payload)?;

    Ok((endpoint, upstream_payload))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::build;

    #[test]
    fn claude_messages_overrides_are_revalidated_against_claude_contract() {
        let payload = json!({
            "model": "claude-opus-4-7",
            "messages": [{"role": "user", "content": "hello"}],
            "custom_include_body": "{\"top_k\":40}"
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let error = build(payload).expect_err("build should fail");
        assert!(
            error
                .to_string()
                .contains("does not support non-default sampling parameters: top_k")
        );
    }
}
