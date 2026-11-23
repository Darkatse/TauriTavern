mod chat_completion_repository;
mod files;
mod readable;
mod recording;
mod request;
mod store;
mod text_completion_repository;
mod types;

pub use chat_completion_repository::LoggingChatCompletionRepository;
pub use store::LlmApiLogStore;
pub use text_completion_repository::LoggingTextCompletionRepository;

pub const LLM_API_LOG_EVENT: &str = "tauritavern-llm-api-log";
