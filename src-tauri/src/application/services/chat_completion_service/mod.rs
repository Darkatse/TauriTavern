use std::sync::Arc;

use serde_json::{json, Value};

use crate::application::dto::chat_completion_dto::{
    ChatCompletionGenerateRequestDto, ChatCompletionStatusRequestDto,
};
use crate::application::errors::ApplicationError;
use crate::domain::repositories::chat_completion_repository::{
    ChatCompletionRepository, ChatCompletionSource,
};
use crate::domain::repositories::secret_repository::SecretRepository;

mod config;
mod custom_parameters;
mod payload;

const OPENAI_SOURCE: &str = "openai";

pub struct ChatCompletionService {
    chat_completion_repository: Arc<dyn ChatCompletionRepository>,
    secret_repository: Arc<dyn SecretRepository>,
}

impl ChatCompletionService {
    pub fn new(
        chat_completion_repository: Arc<dyn ChatCompletionRepository>,
        secret_repository: Arc<dyn SecretRepository>,
    ) -> Self {
        Self {
            chat_completion_repository,
            secret_repository,
        }
    }

    pub async fn get_status(
        &self,
        dto: ChatCompletionStatusRequestDto,
    ) -> Result<Value, ApplicationError> {
        if dto.bypass_status_check {
            return Ok(json!({
                "bypass": true,
                "data": []
            }));
        }

        let source = self.resolve_source(&dto.chat_completion_source)?;
        let config =
            config::resolve_status_api_config(source, &dto, &self.secret_repository).await?;

        self.chat_completion_repository
            .list_models(source, &config)
            .await
            .map_err(ApplicationError::from)
    }

    pub async fn generate(
        &self,
        dto: ChatCompletionGenerateRequestDto,
    ) -> Result<Value, ApplicationError> {
        let source = self.resolve_source(
            dto.get_string("chat_completion_source")
                .unwrap_or(OPENAI_SOURCE),
        )?;

        let config =
            config::resolve_generate_api_config(source, &dto, &self.secret_repository).await?;
        let (endpoint_path, upstream_payload) = payload::build_payload(source, dto.payload)?;

        self.chat_completion_repository
            .generate(source, &config, &endpoint_path, &upstream_payload)
            .await
            .map_err(ApplicationError::from)
    }

    fn resolve_source(&self, raw: &str) -> Result<ChatCompletionSource, ApplicationError> {
        ChatCompletionSource::parse(raw).ok_or_else(|| {
            ApplicationError::ValidationError(format!(
                "Unsupported chat completion source: {}",
                raw
            ))
        })
    }
}
