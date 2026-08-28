use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::watch;

use crate::errors::ApplicationError;
use crate::services::chat_completion_service::ChatCompletionService;
use tt_domain::models::agent::{AgentModelRequest, AgentModelResponse};
use tt_domain::models::tool::ToolId;
use tt_ports::repositories::chat_completion_repository::ChatCompletionToolCallDelta;

mod decode;
mod encode;
mod format;
mod provider_state;
mod providers;
mod schema;

#[cfg(test)]
mod tests;

#[cfg(feature = "test-support")]
pub use decode::decode_chat_completion_response;

#[async_trait]
pub trait AgentModelGateway: Send + Sync {
    async fn generate_with_cancel(
        &self,
        request: AgentModelRequest,
        on_tool_call_delta: Option<&mut (dyn FnMut(AgentToolCallDelta) + Send)>,
        cancel: watch::Receiver<bool>,
    ) -> Result<AgentModelExchange, ApplicationError>;

    async fn close_session(&self, session_id: &str);
}

#[derive(Debug, Clone)]
pub struct AgentModelExchange {
    pub response: AgentModelResponse,
    pub provider_state: Value,
}

#[derive(Debug, PartialEq, Eq)]
pub struct AgentToolCallDelta {
    pub tool_call_index: usize,
    pub tool_id: ToolId,
    pub arguments_fragment: String,
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
        on_tool_call_delta: Option<&mut (dyn FnMut(AgentToolCallDelta) + Send)>,
        cancel: watch::Receiver<bool>,
    ) -> Result<AgentModelExchange, ApplicationError> {
        let websocket_session_id =
            provider_state::responses_websocket_session_id(&request).map(str::to_string);
        let dto = encode::encode_chat_completion_request(&request, on_tool_call_delta.is_some())?;
        let exchange = match on_tool_call_delta {
            Some(on_tool_call_delta) => {
                let mut forward_delta = |delta: ChatCompletionToolCallDelta| {
                    let Some(tool) = decode::model_tool_for_alias(&request.tools, &delta.name)
                    else {
                        return;
                    };
                    on_tool_call_delta(AgentToolCallDelta {
                        tool_call_index: delta.tool_call_index,
                        tool_id: tool.tool_id.clone(),
                        arguments_fragment: delta.arguments_fragment,
                    });
                };
                self.chat_completion_service
                    .generate_exchange_with_cancel(dto, Some(&mut forward_delta), cancel)
                    .await
            }
            None => {
                self.chat_completion_service
                    .generate_exchange_with_cancel(dto, None, cancel)
                    .await
            }
        };
        let exchange = match exchange {
            Err(error @ ApplicationError::Cancelled(_)) => {
                if let Some(session_id) = websocket_session_id {
                    self.chat_completion_service
                        .close_provider_session(&session_id)
                        .await;
                }
                return Err(error);
            }
            result => result?,
        };
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
