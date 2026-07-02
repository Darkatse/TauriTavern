use serde_json::{Map, Value};

use crate::errors::ApplicationError;

use super::makersuite;

pub(super) fn build(payload: Map<String, Value>) -> Result<(String, Value), ApplicationError> {
    makersuite::build_vertexai(payload)
}
