use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{json, Value};
use tokio::sync::{watch, RwLock};

use crate::application::dto::chat_completion_dto::{
    ChatCompletionGenerateRequestDto, ChatCompletionStatusRequestDto,
};
use crate::application::errors::ApplicationError;
use crate::domain::repositories::chat_completion_repository::{
    ChatCompletionCancelReceiver, ChatCompletionRepository, ChatCompletionSource,
    ChatCompletionStreamSender,
};
use crate::domain::repositories::secret_repository::SecretRepository;

mod config;
mod custom_parameters;
mod payload;

const OPENAI_SOURCE: &str = "openai";

pub struct ChatCompletionService {
    chat_completion_repository: Arc<dyn ChatCompletionRepository>,
    secret_repository: Arc<dyn SecretRepository>,
    active_streams: CancellationRegistry,
    active_generations: CancellationRegistry,
}

impl ChatCompletionService {
    pub fn new(
        chat_completion_repository: Arc<dyn ChatCompletionRepository>,
        secret_repository: Arc<dyn SecretRepository>,
    ) -> Self {
        Self {
            chat_completion_repository,
            secret_repository,
            active_streams: CancellationRegistry::default(),
            active_generations: CancellationRegistry::default(),
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

    pub async fn generate_with_cancel(
        &self,
        dto: ChatCompletionGenerateRequestDto,
        mut cancel: ChatCompletionCancelReceiver,
    ) -> Result<Value, ApplicationError> {
        let generation = self.generate(dto);
        tokio::pin!(generation);

        tokio::select! {
            result = &mut generation => result,
            _ = cancel.changed() => {
                if *cancel.borrow() {
                    return Err(ApplicationError::InternalError(
                        "Generation cancelled by user".to_string(),
                    ));
                }

                generation.await
            }
        }
    }

    pub async fn generate_stream(
        &self,
        dto: ChatCompletionGenerateRequestDto,
        sender: ChatCompletionStreamSender,
        cancel: ChatCompletionCancelReceiver,
    ) -> Result<(), ApplicationError> {
        let source = self.resolve_source(
            dto.get_string("chat_completion_source")
                .unwrap_or(OPENAI_SOURCE),
        )?;

        let config =
            config::resolve_generate_api_config(source, &dto, &self.secret_repository).await?;
        let (endpoint_path, upstream_payload) = payload::build_payload(source, dto.payload)?;

        self.chat_completion_repository
            .generate_stream(
                source,
                &config,
                &endpoint_path,
                &upstream_payload,
                sender,
                cancel,
            )
            .await
            .map_err(ApplicationError::from)
    }

    pub async fn register_stream(&self, stream_id: &str) -> watch::Receiver<bool> {
        self.active_streams.register(stream_id).await
    }

    pub async fn cancel_stream(&self, stream_id: &str) -> bool {
        self.active_streams.cancel(stream_id).await
    }

    pub async fn complete_stream(&self, stream_id: &str) {
        self.active_streams.complete(stream_id).await;
    }

    pub async fn register_generation(&self, request_id: &str) -> watch::Receiver<bool> {
        self.active_generations.register(request_id).await
    }

    pub async fn cancel_generation(&self, request_id: &str) -> bool {
        self.active_generations.cancel(request_id).await
    }

    pub async fn complete_generation(&self, request_id: &str) {
        self.active_generations.complete(request_id).await;
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

#[derive(Default)]
struct CancellationRegistry {
    active: RwLock<HashMap<String, watch::Sender<bool>>>,
}

impl CancellationRegistry {
    async fn register(&self, request_id: &str) -> watch::Receiver<bool> {
        let (sender, receiver) = watch::channel(false);
        let mut active = self.active.write().await;

        if let Some(previous_sender) = active.insert(request_id.to_string(), sender) {
            let _ = previous_sender.send(true);
        }

        receiver
    }

    async fn cancel(&self, request_id: &str) -> bool {
        let mut active = self.active.write().await;
        let Some(sender) = active.remove(request_id) else {
            return false;
        };

        let _ = sender.send(true);
        true
    }

    async fn complete(&self, request_id: &str) {
        let mut active = self.active.write().await;
        active.remove(request_id);
    }
}
