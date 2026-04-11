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

    Ok((endpoint, upstream_payload))
}
