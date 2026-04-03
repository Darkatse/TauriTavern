use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{Value, json};
use tokio::sync::{RwLock, watch};

use crate::application::dto::chat_completion_dto::{
    ChatCompletionGenerateRequestDto, ChatCompletionStatusRequestDto,
};
use crate::application::errors::ApplicationError;
use crate::domain::models::settings::{PromptCacheTtl, TauriTavernSettings};
use crate::domain::repositories::chat_completion_repository::{
    ChatCompletionCancelReceiver, ChatCompletionRepository, ChatCompletionSource,
    ChatCompletionStreamSender,
};
use crate::domain::repositories::prompt_cache_repository::{
    PromptCacheKey, PromptCacheRepository,
};
use crate::domain::repositories::secret_repository::SecretRepository;
use crate::domain::repositories::settings_repository::SettingsRepository;

mod config;
mod custom_parameters;
mod payload;
mod prompt_caching;
mod vertexai_auth;

const OPENAI_SOURCE: &str = "openai";

pub struct ChatCompletionService {
    chat_completion_repository: Arc<dyn ChatCompletionRepository>,
    secret_repository: Arc<dyn SecretRepository>,
    settings_repository: Arc<dyn SettingsRepository>,
    prompt_cache_repository: Arc<dyn PromptCacheRepository>,
    active_streams: CancellationRegistry,
    active_generations: CancellationRegistry,
}

impl ChatCompletionService {
    pub fn new(
        chat_completion_repository: Arc<dyn ChatCompletionRepository>,
        secret_repository: Arc<dyn SecretRepository>,
        settings_repository: Arc<dyn SettingsRepository>,
        prompt_cache_repository: Arc<dyn PromptCacheRepository>,
    ) -> Self {
        Self {
            chat_completion_repository,
            secret_repository,
            settings_repository,
            prompt_cache_repository,
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
        if source == ChatCompletionSource::VertexAi {
            return Ok(json!({
                "bypass": true,
                "data": []
            }));
        }
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

        let settings = self.load_tauritavern_settings().await?;

        let config =
            config::resolve_generate_api_config(source, &dto, &self.secret_repository).await?;
        let payload = dto.payload;
        let (endpoint_path, mut upstream_payload) = payload::build_payload(source, payload)?;
        if let Err(error) =
            self.apply_tauritavern_prompt_caching(source, &settings, &mut upstream_payload)
                .await
        {
            tracing::warn!("Prompt caching failed: {}", error);
        }

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

        let settings = self.load_tauritavern_settings().await?;

        let config =
            config::resolve_generate_api_config(source, &dto, &self.secret_repository).await?;
        let payload = dto.payload;
        let (endpoint_path, mut upstream_payload) = payload::build_payload(source, payload)?;
        if let Err(error) =
            self.apply_tauritavern_prompt_caching(source, &settings, &mut upstream_payload)
                .await
        {
            tracing::warn!("Prompt caching failed: {}", error);
        }

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

    async fn load_tauritavern_settings(&self) -> Result<TauriTavernSettings, ApplicationError> {
        self.settings_repository
            .load_tauritavern_settings()
            .await
            .map_err(ApplicationError::from)
    }

    async fn apply_tauritavern_prompt_caching(
        &self,
        source: ChatCompletionSource,
        settings: &TauriTavernSettings,
        upstream_payload: &mut Value,
    ) -> Result<(), ApplicationError> {
        let cache_ttl = settings.models.claude.prompt_cache_ttl;
        if cache_ttl == PromptCacheTtl::Off {
            return Ok(());
        }

        let ttl = match cache_ttl {
            PromptCacheTtl::Off => return Ok(()),
            PromptCacheTtl::FiveMinutes => "5m",
            PromptCacheTtl::OneHour => "1h",
        };

        match source {
            ChatCompletionSource::Claude => {
                let previous = self
                    .prompt_cache_repository
                    .load_prompt_digests(PromptCacheKey::Claude)
                    .await
                    .map_err(ApplicationError::from)?;
                let snapshot = prompt_caching::apply_claude_prompt_caching(
                    upstream_payload,
                    previous.as_ref(),
                    ttl,
                );
                self.prompt_cache_repository
                    .save_prompt_digests(PromptCacheKey::Claude, snapshot)
                    .await
                    .map_err(ApplicationError::from)?;
            }
            ChatCompletionSource::OpenRouter => {
                let is_claude = upstream_payload
                    .as_object()
                    .and_then(|object| object.get("model"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .is_some_and(|model| model.to_ascii_lowercase().starts_with("anthropic/claude"));
                if !is_claude {
                    return Ok(());
                }

                let previous = self
                    .prompt_cache_repository
                    .load_prompt_digests(PromptCacheKey::OpenRouterClaude)
                    .await
                    .map_err(ApplicationError::from)?;
                let snapshot = prompt_caching::apply_openrouter_claude_prompt_caching(
                    upstream_payload,
                    previous.as_ref(),
                    ttl,
                );
                self.prompt_cache_repository
                    .save_prompt_digests(PromptCacheKey::OpenRouterClaude, snapshot)
                    .await
                    .map_err(ApplicationError::from)?;
            }
            _ => {}
        }

        Ok(())
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
