use serde_json::{Map, Value, json};

const CACHE_TTL_DEFAULT: &str = "5m";
const CACHE_TTL_EXTENDED: &str = "1h";

const COMMON_ENABLE_SYSTEM_PROMPT_KEYS: &[&str] =
    &["enableSystemPromptCache", "enable_system_prompt_cache"];
const COMMON_CACHING_AT_DEPTH_KEYS: &[&str] = &["cachingAtDepth", "caching_at_depth"];
const COMMON_EXTENDED_TTL_KEYS: &[&str] = &["extendedTTL", "extended_ttl"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PromptCacheProvider {
    Claude,
    OpenRouter,
}

impl PromptCacheProvider {
    fn namespace(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::OpenRouter => "openrouter",
        }
    }

    fn enable_system_prompt_keys(self) -> &'static [&'static str] {
        match self {
            Self::Claude => &["claude_enable_system_prompt_cache"],
            Self::OpenRouter => &["openrouter_enable_system_prompt_cache"],
        }
    }

    fn caching_at_depth_keys(self) -> &'static [&'static str] {
        match self {
            Self::Claude => &["claude_caching_at_depth"],
            Self::OpenRouter => &["openrouter_caching_at_depth"],
        }
    }

    fn extended_ttl_keys(self) -> &'static [&'static str] {
        match self {
            Self::Claude => &["claude_extended_ttl"],
            Self::OpenRouter => &["openrouter_extended_ttl"],
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct PromptCacheConfig {
    pub(super) enable_system_prompt_cache: bool,
    pub(super) caching_at_depth: Option<usize>,
    pub(super) ttl: &'static str,
}

impl PromptCacheConfig {
    pub(super) fn from_payload(
        payload: &Map<String, Value>,
        provider: PromptCacheProvider,
    ) -> Self {
        let namespace = provider.namespace();

        let enable_system_prompt_cache = payload_bool(payload, COMMON_ENABLE_SYSTEM_PROMPT_KEYS)
            .or_else(|| payload_bool(payload, provider.enable_system_prompt_keys()))
            .or_else(|| {
                payload_namespace_bool(payload, namespace, COMMON_ENABLE_SYSTEM_PROMPT_KEYS)
            })
            .or_else(|| {
                payload_namespace_bool(payload, namespace, provider.enable_system_prompt_keys())
            })
            .unwrap_or(false);

        let caching_at_depth = payload_i64(payload, COMMON_CACHING_AT_DEPTH_KEYS)
            .or_else(|| payload_i64(payload, provider.caching_at_depth_keys()))
            .or_else(|| payload_namespace_i64(payload, namespace, COMMON_CACHING_AT_DEPTH_KEYS))
            .or_else(|| payload_namespace_i64(payload, namespace, provider.caching_at_depth_keys()))
            .filter(|value| *value >= 0)
            .and_then(|value| usize::try_from(value).ok());

        let use_extended_ttl = payload_bool(payload, COMMON_EXTENDED_TTL_KEYS)
            .or_else(|| payload_bool(payload, provider.extended_ttl_keys()))
            .or_else(|| payload_namespace_bool(payload, namespace, COMMON_EXTENDED_TTL_KEYS))
            .or_else(|| payload_namespace_bool(payload, namespace, provider.extended_ttl_keys()))
            .unwrap_or(false);

        let ttl = if use_extended_ttl {
            CACHE_TTL_EXTENDED
        } else {
            CACHE_TTL_DEFAULT
        };

        Self {
            enable_system_prompt_cache,
            caching_at_depth,
            ttl,
        }
    }
}

pub(super) fn append_cache_control_to_last(blocks: &mut [Value], ttl: &str) {
    let Some(last_block) = blocks.last_mut().and_then(Value::as_object_mut) else {
        return;
    };

    last_block.insert("cache_control".to_string(), cache_control(ttl));
}

pub(super) fn apply_depth_cache_for_claude(
    messages: &mut [Value],
    caching_at_depth: usize,
    ttl: &str,
) {
    let mut passed_prefill = false;
    let mut depth: usize = 0;
    let mut previous_role = String::new();

    for message in messages.iter_mut().rev() {
        let Some(message_object) = message.as_object_mut() else {
            continue;
        };

        let role = message_object
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();

        if !passed_prefill && role == "assistant" {
            continue;
        }

        passed_prefill = true;

        if role != previous_role {
            if depth == caching_at_depth || depth == caching_at_depth + 2 {
                if let Some(content) = message_object
                    .get_mut("content")
                    .and_then(Value::as_array_mut)
                {
                    append_cache_control_to_last(content, ttl);
                }
            }

            if depth == caching_at_depth + 2 {
                break;
            }

            depth += 1;
            previous_role = role;
        }
    }
}

pub(super) fn apply_depth_cache_for_openrouter_claude(
    messages: &mut [Value],
    caching_at_depth: usize,
    ttl: &str,
) {
    let mut passed_prefill = false;
    let mut depth: usize = 0;
    let mut previous_role = String::new();

    for message in messages.iter_mut().rev() {
        let Some(message_object) = message.as_object_mut() else {
            continue;
        };

        let role = message_object
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();

        if !passed_prefill && role == "assistant" {
            continue;
        }

        passed_prefill = true;

        if role != previous_role {
            if depth == caching_at_depth || depth == caching_at_depth + 2 {
                ensure_openrouter_message_content_parts(message_object);
                if let Some(content) = message_object
                    .get_mut("content")
                    .and_then(Value::as_array_mut)
                {
                    append_cache_control_to_last(content, ttl);
                }
            }

            if depth == caching_at_depth + 2 {
                break;
            }

            depth += 1;
            previous_role = role;
        }
    }
}

pub(super) fn apply_system_prompt_cache_for_openrouter(messages: &mut [Value], ttl: &str) {
    let Some(system_message) = messages.iter_mut().find_map(|message| {
        let object = message.as_object_mut()?;
        if object.get("role").and_then(Value::as_str) == Some("system") {
            Some(object)
        } else {
            None
        }
    }) else {
        return;
    };

    if system_message.contains_key("cache_control") {
        return;
    }

    let Some(content) = system_message.get_mut("content") else {
        return;
    };

    match content {
        Value::Array(parts) => {
            if parts.iter().any(|part| {
                part.as_object()
                    .is_some_and(|part| part.contains_key("cache_control"))
            }) {
                return;
            }

            for part in parts.iter_mut().rev() {
                let Some(part_object) = part.as_object_mut() else {
                    continue;
                };

                if part_object.get("type").and_then(Value::as_str) == Some("text") {
                    part_object.insert("cache_control".to_string(), cache_control(ttl));
                    return;
                }
            }
        }
        Value::String(text) => {
            let text = std::mem::take(text);
            *content = Value::Array(vec![json!({
                "type": "text",
                "text": text,
                "cache_control": cache_control(ttl),
            })]);
        }
        _ => {}
    }
}

fn ensure_openrouter_message_content_parts(message: &mut Map<String, Value>) {
    let Some(content) = message.get_mut("content") else {
        return;
    };

    if let Value::String(text) = content {
        let text = std::mem::take(text);
        *content = Value::Array(vec![json!({
            "type": "text",
            "text": text,
        })]);
    }
}

fn payload_bool(payload: &Map<String, Value>, keys: &[&str]) -> Option<bool> {
    payload_value(payload, keys).and_then(Value::as_bool)
}

fn payload_i64(payload: &Map<String, Value>, keys: &[&str]) -> Option<i64> {
    payload_value(payload, keys).and_then(value_to_i64)
}

fn payload_namespace_bool(
    payload: &Map<String, Value>,
    namespace: &str,
    keys: &[&str],
) -> Option<bool> {
    payload_namespace_value(payload, namespace, keys).and_then(Value::as_bool)
}

fn payload_namespace_i64(
    payload: &Map<String, Value>,
    namespace: &str,
    keys: &[&str],
) -> Option<i64> {
    payload_namespace_value(payload, namespace, keys).and_then(value_to_i64)
}

fn payload_value<'a>(payload: &'a Map<String, Value>, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|key| payload.get(*key))
}

fn payload_namespace_value<'a>(
    payload: &'a Map<String, Value>,
    namespace: &str,
    keys: &[&str],
) -> Option<&'a Value> {
    let namespaced_object = payload.get(namespace).and_then(Value::as_object)?;
    keys.iter().find_map(|key| namespaced_object.get(*key))
}

fn value_to_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|number| i64::try_from(number).ok()))
}

fn cache_control(ttl: &str) -> Value {
    json!({
        "type": "ephemeral",
        "ttl": ttl,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        PromptCacheConfig, PromptCacheProvider, apply_depth_cache_for_openrouter_claude,
        apply_system_prompt_cache_for_openrouter,
    };

    #[test]
    fn openrouter_provider_specific_cache_config_is_supported() {
        let payload = json!({
            "openrouter_enable_system_prompt_cache": true,
            "openrouter_caching_at_depth": 1,
            "openrouter_extended_ttl": true
        })
        .as_object()
        .cloned()
        .expect("payload should be object");

        let config = PromptCacheConfig::from_payload(&payload, PromptCacheProvider::OpenRouter);
        assert!(config.enable_system_prompt_cache);
        assert_eq!(config.caching_at_depth, Some(1));
        assert_eq!(config.ttl, "1h");
    }

    #[test]
    fn openrouter_system_prompt_cache_wraps_string_content() {
        let mut messages = vec![
            json!({ "role": "system", "content": "System rules" }),
            json!({ "role": "user", "content": "hello" }),
        ];

        apply_system_prompt_cache_for_openrouter(&mut messages, "5m");

        let has_cache = messages
            .first()
            .and_then(|message| message.get("content"))
            .and_then(|content| content.as_array())
            .and_then(|parts| parts.first())
            .and_then(|part| part.get("cache_control"))
            .is_some();
        assert!(has_cache);
    }

    #[test]
    fn openrouter_depth_cache_marks_expected_role_boundaries() {
        let mut messages = vec![
            json!({ "role": "user", "content": "u1" }),
            json!({ "role": "assistant", "content": "a1" }),
            json!({ "role": "user", "content": "u2" }),
            json!({ "role": "assistant", "content": "a2" }),
            json!({ "role": "user", "content": "u3" }),
        ];

        apply_depth_cache_for_openrouter_claude(&mut messages, 0, "5m");

        for index in [2_usize, 4_usize] {
            let has_cache = messages
                .get(index)
                .and_then(|message| message.get("content"))
                .and_then(|content| content.as_array())
                .and_then(|parts| parts.last())
                .and_then(|part| part.get("cache_control"))
                .is_some();
            assert!(has_cache, "message at index {index} should be cacheable");
        }
    }
}
