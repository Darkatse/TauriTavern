use std::collections::HashMap;

use serde_json::{Map, Value};

use crate::services::client_attribution;

pub(crate) fn attribution_headers() -> HashMap<String, String> {
    client_attribution::headers()
        .into_iter()
        .chain([
            ("X-OpenRouter-Title", client_attribution::TITLE),
            ("X-OpenRouter-Categories", "roleplay,general-chat"),
        ])
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect()
}

pub(crate) fn provider_preferences(payload: &Map<String, Value>) -> Option<Value> {
    let order = payload
        .get("provider")
        .and_then(Value::as_array)
        .filter(|providers| !providers.is_empty())
        .cloned();
    let quantizations = payload
        .get("quantizations")
        .and_then(Value::as_array)
        .filter(|items| !items.is_empty())
        .cloned();

    if order.is_none() && quantizations.is_none() {
        return None;
    }

    let mut provider = Map::new();
    if let Some(order) = order {
        let allow_fallbacks = payload
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
