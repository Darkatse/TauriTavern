use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::watch;

use crate::application::errors::ApplicationError;
use crate::application::services::chat_completion_service::ChatCompletionService;
use crate::domain::models::agent::{AgentModelRequest, AgentModelResponse};

mod decode;
mod encode;
mod format;
mod provider_state;
mod providers;
mod schema;

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) use decode::decode_chat_completion_response;

#[async_trait]
pub trait AgentModelGateway: Send + Sync {
    async fn generate_with_cancel(
        &self,
        request: AgentModelRequest,
        cancel: watch::Receiver<bool>,
    ) -> Result<AgentModelExchange, ApplicationError>;

    async fn close_session(&self, session_id: &str);
}

#[derive(Debug, Clone)]
pub struct AgentModelExchange {
    pub response: AgentModelResponse,
    pub provider_state: Value,
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
        request: AgentModelRequest,
        cancel: watch::Receiver<bool>,
    ) -> Result<AgentModelExchange, ApplicationError> {
        let dto = encode::encode_chat_completion_request(&request)?;
        let exchange = self
            .chat_completion_service
            .generate_exchange_with_cancel(dto, cancel)
            .await?;
        let source = exchange.source;
        let adapter = providers::AgentProviderAdapter::from_format(exchange.provider_format);
        let response = decode::decode_chat_completion_exchange(exchange, &request.tools)?;
        let provider_state =
            provider_state::next_provider_state(&request, source, adapter, &response)?;

        Ok(AgentModelExchange {
            response,
            provider_state,
        })
    }

    async fn close_session(&self, session_id: &str) {
        self.chat_completion_service
            .close_provider_session(session_id)
            .await;
    }
}
