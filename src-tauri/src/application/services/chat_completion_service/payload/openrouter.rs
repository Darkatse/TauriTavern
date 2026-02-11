use serde_json::{json, Map, Value};

use super::openai;
use super::prompt_cache::{
    apply_depth_cache_for_openrouter_claude, apply_system_prompt_cache_for_openrouter,
    PromptCacheConfig, PromptCacheProvider,
};
use super::shared::insert_if_present;

pub(super) fn build(payload: Map<String, Value>) -> (String, Value) {
    let source_payload = payload.clone();
    let (_, mut upstream_payload) = openai::build(payload);

    if let Some(body) = upstream_payload.as_object_mut() {
        apply_openrouter_overrides(body, &source_payload);
        apply_openrouter_prompt_caching(body, &source_payload);
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

fn apply_openrouter_prompt_caching(
    body: &mut Map<String, Value>,
    source_payload: &Map<String, Value>,
) {
    let Some(model) = body
        .get("model")
        .and_then(Value::as_str)
        .or_else(|| source_payload.get("model").and_then(Value::as_str))
    else {
        return;
    };

    if !is_openrouter_claude_model(model) {
        return;
    }

    let cache_config =
        PromptCacheConfig::from_payload(source_payload, PromptCacheProvider::OpenRouter);
    if !cache_config.enable_system_prompt_cache && cache_config.caching_at_depth.is_none() {
        return;
    }

    let Some(messages) = body.get_mut("messages").and_then(Value::as_array_mut) else {
        return;
    };

    if cache_config.enable_system_prompt_cache {
        apply_system_prompt_cache_for_openrouter(messages, cache_config.ttl);
    }

    if let Some(caching_at_depth) = cache_config.caching_at_depth {
        apply_depth_cache_for_openrouter_claude(messages, caching_at_depth, cache_config.ttl);
    }
}

fn is_openrouter_claude_model(model: &str) -> bool {
    model
        .trim()
        .to_ascii_lowercase()
        .starts_with("anthropic/claude")
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
        .filter(|providers| !providers.is_empty())?
        .clone();

    let allow_fallbacks = source_payload
        .get("allow_fallbacks")
        .and_then(Value::as_bool)
        .unwrap_or(true);

    Some(json!({
        "allow_fallbacks": allow_fallbacks,
        "order": order,
    }))
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

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
    fn openrouter_claude_cache_is_applied_for_system_and_depth() {
        let payload = json!({
            "chat_completion_source": "openrouter",
            "model": "anthropic/claude-3.5-sonnet",
            "messages": [
                { "role": "system", "content": "System prompt" },
                { "role": "user", "content": "u1" },
                { "role": "assistant", "content": "a1" },
                { "role": "user", "content": "u2" }
            ],
            "openrouter_enable_system_prompt_cache": true,
            "openrouter_caching_at_depth": 0
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_, upstream) = build(payload);
        let messages = upstream
            .as_object()
            .and_then(|body| body.get("messages"))
            .and_then(Value::as_array)
            .expect("messages should be an array");

        let system_has_cache = messages
            .first()
            .and_then(Value::as_object)
            .and_then(|message| message.get("content"))
            .and_then(Value::as_array)
            .and_then(|parts| parts.first())
            .and_then(|part| part.get("cache_control"))
            .is_some();
        assert!(system_has_cache);

        let depth_has_cache = messages
            .get(3)
            .and_then(Value::as_object)
            .and_then(|message| message.get("content"))
            .and_then(Value::as_array)
            .and_then(|parts| parts.last())
            .and_then(|part| part.get("cache_control"))
            .is_some();
        assert!(depth_has_cache);
    }

    #[test]
    fn openrouter_cache_is_not_applied_for_non_claude_models() {
        let payload = json!({
            "chat_completion_source": "openrouter",
            "model": "openai/gpt-4.1-mini",
            "messages": [
                { "role": "system", "content": "System prompt" },
                { "role": "user", "content": "hello" }
            ],
            "openrouter_enable_system_prompt_cache": true,
            "openrouter_caching_at_depth": 0
        })
        .as_object()
        .cloned()
        .expect("payload must be object");

        let (_, upstream) = build(payload);
        let system_has_cache = upstream
            .as_object()
            .and_then(|body| body.get("messages"))
            .and_then(Value::as_array)
            .and_then(|messages| messages.first())
            .and_then(Value::as_object)
            .and_then(|message| message.get("content"))
            .and_then(Value::as_str)
            .is_some();
        assert!(system_has_cache);
    }
}
