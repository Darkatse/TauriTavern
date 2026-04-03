use serde_json::{Map, Value, json};

use super::openai;
use super::shared::insert_if_present;

pub(super) fn build(payload: Map<String, Value>) -> (String, Value) {
    let source_payload = payload.clone();
    let (_, mut upstream_payload) = openai::build(payload);

    if let Some(body) = upstream_payload.as_object_mut() {
        apply_openrouter_overrides(body, &source_payload);
    }

    ("/chat/completions".to_string(), upstream_payload)
}

fn apply_openrouter_overrides(body: &mut Map<String, Value>, source_payload: &Map<String, Value>) {
    for key in ["min_p", "top_a", "repetition_penalty"] {
        insert_if_present(body, source_payload, key);
    }

    body.insert(
        "include_reasoning".to_string(),
        Value::Bool(
            source_payload
                .get("include_reasoning")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        ),
    );

    if let Some(transforms) = map_middleout_transforms(source_payload.get("middleout")) {
        body.insert("transforms".to_string(), transforms);
    }

    if source_payload
        .get("enable_web_search")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        body.insert("plugins".to_string(), json!([{ "id": "web" }]));
    }

    if let Some(provider) = map_provider_preferences(source_payload) {
        body.insert("provider".to_string(), provider);
    }

    if source_payload
        .get("use_fallback")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        body.insert("route".to_string(), Value::String("fallback".to_string()));
    }

    if let Some(reasoning_effort) = source_payload
        .get("reasoning_effort")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        body.remove("reasoning_effort");
        body.insert(
            "reasoning".to_string(),
            json!({
                "effort": reasoning_effort,
            }),
        );
    }
}

fn map_middleout_transforms(value: Option<&Value>) -> Option<Value> {
    let value = value
        .and_then(Value::as_str)
        .map(str::trim)
        .map(str::to_ascii_lowercase)?;

    match value.as_str() {
        "on" => Some(Value::Array(vec![Value::String("middle-out".to_string())])),
        "off" => Some(Value::Array(Vec::new())),
        _ => None,
    }
}

fn map_provider_preferences(source_payload: &Map<String, Value>) -> Option<Value> {
    let order = source_payload
        .get("provider")
        .and_then(Value::as_array)
        .filter(|providers| !providers.is_empty())
        .cloned();
    let quantizations = source_payload
        .get("quantizations")
        .and_then(Value::as_array)
        .filter(|items| !items.is_empty())
        .cloned();

    if order.is_none() && quantizations.is_none() {
        return None;
    }

    let mut provider = Map::new();
    if let Some(order) = order {
        let allow_fallbacks = source_payload
            .get("allow_fallbacks")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        provider.insert("allow_fallbacks".to_string(), Value::Bool(allow_fallbacks));
        provider.insert("order".to_string(), Value::Array(order));
    }
    if let Some(quantizations) = quantizations {
        provider.insert("quantizations".to_string(), Value::Array(quantizations));
    }

    Some(Value::Object(provider))
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::build;

    #[test]
    fn openrouter_payload_includes_provider_specific_fields() {
        let payload = json!({
            "chat_completion_source": "openrouter",
            "model": "openai/gpt-4.1-mini",
            "messages": [{"role": "user", "content": "hello"}],
            "include_reasoning": true,
            "reasoning_effort": "high",
            "middleout": "on",
            "enable_web_search": true,
            "provider": ["anthropic", "openai"],
            "allow_fallbacks": false,
            "use_fallback": true,
            "min_p": 0.1,
            "top_a": 0.5,
            "repetition_penalty": 1.2
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (endpoint, upstream) = build(payload);
        assert_eq!(endpoint, "/chat/completions");

        let body = upstream.as_object().expect("payload must be object");
        assert_eq!(
            body.get("include_reasoning")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            true
        );
        assert_eq!(
            body.get("route")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            "fallback"
        );
        assert!(body.get("reasoning").is_some());
        assert!(body.get("reasoning_effort").is_none());
        assert!(body.get("plugins").is_some());

        let transforms = body
            .get("transforms")
            .and_then(Value::as_array)
            .expect("transforms must be array");
        assert_eq!(
            transforms.first().and_then(Value::as_str),
            Some("middle-out")
        );
    }

    #[test]
    fn openrouter_middleout_off_maps_to_empty_transforms() {
        let payload = json!({
            "chat_completion_source": "openrouter",
            "model": "openai/gpt-4.1-mini",
            "messages": [{"role": "user", "content": "hello"}],
            "middleout": "off"
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_, upstream) = build(payload);
        let transforms_len = upstream
            .as_object()
            .and_then(|body| body.get("transforms"))
            .and_then(Value::as_array)
            .map(|items| items.len())
            .unwrap_or_default();

        assert_eq!(transforms_len, 0);
    }

    #[test]
    fn openrouter_quantizations_are_forwarded_without_provider_order() {
        let payload = json!({
            "chat_completion_source": "openrouter",
            "model": "openai/gpt-4.1-mini",
            "messages": [{"role": "user", "content": "hello"}],
            "quantizations": ["int8", "fp16"]
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_, upstream) = build(payload);
        let quantizations = upstream
            .as_object()
            .and_then(|body| body.get("provider"))
            .and_then(Value::as_object)
            .and_then(|provider| provider.get("quantizations"))
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<&str>>()
            })
            .unwrap_or_default();

        assert_eq!(quantizations, vec!["int8", "fp16"]);
    }
}
