use serde_json::{Map, Value};

use crate::application::errors::ApplicationError;
use crate::domain::repositories::chat_completion_repository::ChatCompletionSource;

mod claude;
mod chutes;
mod cohere;
mod custom;
mod deepseek;
mod makersuite;
mod moonshot;
mod nanogpt;
mod openai;
mod openrouter;
mod prompt_post_processing;
mod shared;
mod tool_calls;
mod vertexai;
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
        | ChatCompletionSource::Groq
        | ChatCompletionSource::SiliconFlow => Ok(openai::build(payload)),
        ChatCompletionSource::DeepSeek => Ok(deepseek::build(payload)),
        ChatCompletionSource::Cohere => Ok(cohere::build(payload)?),
        ChatCompletionSource::Moonshot => Ok(moonshot::build(payload)),
        ChatCompletionSource::NanoGpt => nanogpt::build(payload),
        ChatCompletionSource::Chutes => chutes::build(payload),
        ChatCompletionSource::OpenRouter => Ok(openrouter::build(payload)),
        ChatCompletionSource::Zai => Ok(zai::build(payload)),
        ChatCompletionSource::Custom => custom::build(payload),
        ChatCompletionSource::Claude => Ok(claude::build(payload)?),
        ChatCompletionSource::Makersuite => Ok(makersuite::build(payload)?),
        ChatCompletionSource::VertexAi => Ok(vertexai::build(payload)?),
    }
}
