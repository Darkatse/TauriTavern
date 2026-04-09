use serde_json::{Map, Value, json};

use crate::application::errors::ApplicationError;

use super::openai;
use super::shared::insert_if_present;

const ONLINE_SUFFIX: &str = ":online";

pub(super) fn build(payload: Map<String, Value>) -> Result<(String, Value), ApplicationError> {
    let source_payload = payload.clone();
    let (endpoint, mut upstream_payload) = openai::build(payload);

    if endpoint != "/chat/completions" {
        return Err(ApplicationError::ValidationError(
            "NanoGPT only supports /chat/completions in this build.".to_string(),
        ));
    }

    let Some(body) = upstream_payload.as_object_mut() else {
        return Ok((endpoint, upstream_payload));
    };

    apply_nanogpt_overrides(body, &source_payload)?;

    Ok((endpoint, upstream_payload))
}

fn apply_nanogpt_overrides(
    body: &mut Map<String, Value>,
    source_payload: &Map<String, Value>,
) -> Result<(), ApplicationError> {
    for key in ["min_p", "top_a", "repetition_penalty"] {
        insert_if_present(body, source_payload, key);
    }

    if source_payload
        .get("enable_web_search")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        if let Some(model_value) = body.get_mut("model") {
            if let Some(model) = model_value
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                if !model.ends_with(ONLINE_SUFFIX) {
                    *model_value = Value::String(format!("{model}{ONLINE_SUFFIX}"));
                }
            }
        }
    }

    if let Some(reasoning_effort) = source_payload
        .get("reasoning_effort")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if !reasoning_effort.eq_ignore_ascii_case("auto") {
            body.insert(
                "reasoning".to_string(),
                json!({
                    "effort": map_reasoning_effort(reasoning_effort)?,
                }),
            );
        }
    }

    Ok(())
}

fn map_reasoning_effort(value: &str) -> Result<&'static str, ApplicationError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "min" => Ok("none"),
        "low" => Ok("minimal"),
        "medium" => Ok("low"),
        "high" => Ok("medium"),
        "max" => Ok("high"),
        other => Err(ApplicationError::ValidationError(format!(
            "Unsupported NanoGPT reasoning_effort: {other}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::build;

    #[test]
    fn nanogpt_payload_appends_online_suffix_when_web_search_enabled() {
        let payload = json!({
            "chat_completion_source": "nanogpt",
            "model": "gpt-4o-mini",
            "messages": [{"role": "user", "content": "hello"}],
            "enable_web_search": true
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (endpoint, upstream) = build(payload).expect("build must succeed");
        assert_eq!(endpoint, "/chat/completions");

        let model = upstream
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or_default();
        assert_eq!(model, "gpt-4o-mini:online");
    }

    #[test]
    fn nanogpt_payload_does_not_duplicate_online_suffix() {
        let payload = json!({
            "chat_completion_source": "nanogpt",
            "model": "gpt-4o-mini:online",
            "messages": [{"role": "user", "content": "hello"}],
            "enable_web_search": true
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_endpoint, upstream) = build(payload).expect("build must succeed");
        assert_eq!(
            upstream.get("model").and_then(Value::as_str),
            Some("gpt-4o-mini:online")
        );
    }

    #[test]
    fn nanogpt_payload_maps_reasoning_effort() {
        let payload = json!({
            "chat_completion_source": "nanogpt",
            "model": "gpt-4o-mini",
            "messages": [{"role": "user", "content": "hello"}],
            "reasoning_effort": "low"
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_endpoint, upstream) = build(payload).expect("build must succeed");
        assert_eq!(
            upstream
                .get("reasoning")
                .and_then(Value::as_object)
                .and_then(|reasoning| reasoning.get("effort"))
                .and_then(Value::as_str),
            Some("minimal")
        );
    }

    #[test]
    fn nanogpt_payload_ignores_auto_reasoning_effort() {
        let payload = json!({
            "chat_completion_source": "nanogpt",
            "model": "gpt-4o-mini",
            "messages": [{"role": "user", "content": "hello"}],
            "reasoning_effort": "auto"
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_endpoint, upstream) = build(payload).expect("build must succeed");
        assert!(upstream.get("reasoning").is_none());
    }
}
