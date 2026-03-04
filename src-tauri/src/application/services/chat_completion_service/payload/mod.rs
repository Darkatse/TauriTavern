use serde_json::{Map, Value};

use crate::application::errors::ApplicationError;
use crate::domain::repositories::chat_completion_repository::ChatCompletionSource;

mod claude;
mod custom;
mod deepseek;
mod makersuite;
mod moonshot;
mod openai;
mod openrouter;
mod prompt_cache;
mod prompt_post_processing;
mod shared;
mod tool_calls;
mod zai;

pub(super) fn build_payload(
    source: ChatCompletionSource,
    payload: Map<String, Value>,
) -> Result<(String, Value), ApplicationError> {
    let mut payload = payload;

    if source != ChatCompletionSource::DeepSeek {
        prompt_post_processing::apply_custom_prompt_post_processing(&mut payload);
    }

    match source {
        ChatCompletionSource::OpenAi
        | ChatCompletionSource::SiliconFlow => Ok(openai::build(payload)),
        ChatCompletionSource::DeepSeek => Ok(deepseek::build(payload)),
        ChatCompletionSource::Moonshot => Ok(moonshot::build(payload)),
        ChatCompletionSource::OpenRouter => Ok(openrouter::build(payload)),
        ChatCompletionSource::Zai => Ok(zai::build(payload)),
        ChatCompletionSource::Custom => custom::build(payload),
        ChatCompletionSource::Claude => Ok(claude::build(payload)?),
        ChatCompletionSource::Makersuite => Ok(makersuite::build(payload)?),
    }
}
