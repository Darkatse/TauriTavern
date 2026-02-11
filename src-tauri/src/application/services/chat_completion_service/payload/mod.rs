use serde_json::{Map, Value};

use crate::application::errors::ApplicationError;
use crate::domain::repositories::chat_completion_repository::ChatCompletionSource;

mod claude;
mod custom;
mod makersuite;
mod openai;
mod shared;

pub(super) fn build_payload(
    source: ChatCompletionSource,
    payload: Map<String, Value>,
) -> Result<(String, Value), ApplicationError> {
    match source {
        ChatCompletionSource::OpenAi => Ok(openai::build(payload)),
        ChatCompletionSource::Custom => custom::build(payload),
        ChatCompletionSource::Claude => Ok(claude::build(payload)?),
        ChatCompletionSource::Makersuite => Ok(makersuite::build(payload)?),
    }
}
