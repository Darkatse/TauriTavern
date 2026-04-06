use serde_json::{Map, Value};

use crate::application::errors::ApplicationError;

use super::openai;

pub(super) fn build(payload: Map<String, Value>) -> Result<(String, Value), ApplicationError> {
    let min_p = payload.get("min_p").cloned();
    let repetition_penalty = payload.get("repetition_penalty").cloned();
    let reasoning_effort = payload.get("reasoning_effort").cloned();

    let (endpoint_path, mut upstream_payload) = openai::build(payload);

    if let Value::Object(object) = &mut upstream_payload {
        insert_value_if_present(object, "min_p", min_p);
        insert_value_if_present(object, "repetition_penalty", repetition_penalty);
        insert_value_if_present(object, "reasoning_effort", reasoning_effort);
    }

    Ok((endpoint_path, upstream_payload))
}

fn insert_value_if_present(object: &mut Map<String, Value>, key: &str, value: Option<Value>) {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return;
    };

    object.insert(key.to_string(), value);
}
