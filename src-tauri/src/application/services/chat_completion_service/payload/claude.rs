use serde_json::{Map, Value};

use crate::application::errors::ApplicationError;

mod builder;
mod contract;
mod messages;
mod params;
mod tools;
mod validation;

pub(super) fn build(payload: Map<String, Value>) -> Result<(String, Value), ApplicationError> {
    let request = Value::Object(builder::build_claude_payload(&payload)?);
    validate_request(&request)?;

    Ok(("/messages".to_string(), request))
}

pub(super) fn validate_request(payload: &Value) -> Result<(), ApplicationError> {
    validation::validate_request(payload)
}

#[cfg(test)]
mod tests;
