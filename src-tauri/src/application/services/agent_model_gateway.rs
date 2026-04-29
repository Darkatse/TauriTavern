use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::watch;

use crate::application::dto::chat_completion_dto::ChatCompletionGenerateRequestDto;
use crate::application::errors::ApplicationError;
use crate::application::services::chat_completion_service::ChatCompletionService;

#[async_trait]
pub trait AgentModelGateway: Send + Sync {
    async fn generate_with_cancel(
        &self,
        request: ChatCompletionGenerateRequestDto,
        cancel: watch::Receiver<bool>,
    ) -> Result<Value, ApplicationError>;
}

pub struct ChatCompletionAgentModelGateway {
    chat_completion_service: Arc<ChatCompletionService>,
}

impl ChatCompletionAgentModelGateway {
    pub fn new(chat_completion_service: Arc<ChatCompletionService>) -> Self {
        Self {
            chat_completion_service,
        }
    }
}

#[async_trait]
impl AgentModelGateway for ChatCompletionAgentModelGateway {
    async fn generate_with_cancel(
        &self,
        request: ChatCompletionGenerateRequestDto,
        cancel: watch::Receiver<bool>,
    ) -> Result<Value, ApplicationError> {
        self.chat_completion_service
            .generate_with_cancel(request, cancel)
            .await
    }
}
