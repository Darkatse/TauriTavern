use serde_json::{Map, Value};

const CLAUDE_DEFAULT_TEMPERATURE: f64 = 1.0;
const CLAUDE_DEFAULT_TOP_P: f64 = 1.0;
const CLAUDE_DEFAULT_TOP_K: f64 = 0.0;

pub(super) fn value_to_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|number| i64::try_from(number).ok()))
}

pub(super) fn value_to_f64(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_i64().map(|number| number as f64))
        .or_else(|| value.as_u64().map(|number| number as f64))
}

pub(super) fn collect_non_default_sampling_params(
    payload: &Map<String, Value>,
) -> Vec<&'static str> {
    let mut params = Vec::new();
    if has_non_default_temperature(payload) {
        params.push("temperature");
    }
    if has_non_default_top_p(payload) {
        params.push("top_p");
    }
    if has_non_default_top_k(payload) {
        params.push("top_k");
    }
    params
}

pub(super) fn has_non_default_temperature(payload: &Map<String, Value>) -> bool {
    numeric_field_differs_from_default(payload, "temperature", CLAUDE_DEFAULT_TEMPERATURE)
}

pub(super) fn has_non_default_top_p(payload: &Map<String, Value>) -> bool {
    numeric_field_differs_from_default(payload, "top_p", CLAUDE_DEFAULT_TOP_P)
}

pub(super) fn has_non_default_top_k(payload: &Map<String, Value>) -> bool {
    numeric_field_differs_from_default(payload, "top_k", CLAUDE_DEFAULT_TOP_K)
}

fn numeric_field_differs_from_default(
    payload: &Map<String, Value>,
    key: &str,
    default: f64,
) -> bool {
    payload
        .get(key)
        .and_then(value_to_f64)
        .is_some_and(|value| (value - default).abs() > f64::EPSILON)
}
