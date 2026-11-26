use serde_json::{Map, Value};

use crate::errors::ApplicationError;

use super::openai;

pub(super) fn build(payload: Map<String, Value>) -> Result<(String, Value), ApplicationError> {
    let web_search = payload.get("enable_web_search").and_then(Value::as_bool) == Some(true);

    let (endpoint, mut body) = openai::build(payload)?;
    if web_search && let Some(object) = body.as_object_mut() {
        object.insert("web_search".to_string(), Value::Bool(true));
    }

    Ok((endpoint, body))
}
