use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{Map, Value, json};
use tokio::sync::{RwLock, watch};

use crate::application::dto::chat_completion_dto::{
    ChatCompletionGenerateRequestDto, ChatCompletionStatusRequestDto,
};
use crate::application::errors::ApplicationError;
use crate::domain::ios_policy::{IosPolicyActivationReport, IosPolicyScope};
use crate::domain::models::settings::{PromptCacheTtl, TauriTavernSettings};
use crate::domain::repositories::chat_completion_repository::{
    ChatCompletionCancelReceiver, ChatCompletionRepository, ChatCompletionSource,
    ChatCompletionStreamSender,
};
use crate::domain::repositories::prompt_cache_repository::{PromptCacheKey, PromptCacheRepository};
use crate::domain::repositories::secret_repository::SecretRepository;
use crate::domain::repositories::settings_repository::SettingsRepository;

mod config;
mod custom_parameters;
mod payload;
mod prompt_caching;
mod vertexai_auth;

const OPENAI_SOURCE: &str = ChatCompletionSource::OpenAi.key();

pub struct ChatCompletionService {
    chat_completion_repository: Arc<dyn ChatCompletionRepository>,
    secret_repository: Arc<dyn SecretRepository>,
    settings_repository: Arc<dyn SettingsRepository>,
    prompt_cache_repository: Arc<dyn PromptCacheRepository>,
    ios_policy: IosPolicyActivationReport,
    active_streams: CancellationRegistry,
    active_generations: CancellationRegistry,
}

impl ChatCompletionService {
    pub fn new(
        chat_completion_repository: Arc<dyn ChatCompletionRepository>,
        secret_repository: Arc<dyn SecretRepository>,
        settings_repository: Arc<dyn SettingsRepository>,
        prompt_cache_repository: Arc<dyn PromptCacheRepository>,
        ios_policy: IosPolicyActivationReport,
    ) -> Self {
        Self {
            chat_completion_repository,
            secret_repository,
            settings_repository,
            prompt_cache_repository,
            ios_policy,
            active_streams: CancellationRegistry::default(),
            active_generations: CancellationRegistry::default(),
        }
    }

    fn ios_policy_is_active(&self) -> bool {
        self.ios_policy.scope == IosPolicyScope::Ios
    }

    fn ensure_chat_completion_source_allowed(
        &self,
        source: ChatCompletionSource,
    ) -> Result<(), ApplicationError> {
        if !self.ios_policy_is_active() {
            return Ok(());
        }

        if self
            .ios_policy
            .capabilities
            .llm
            .chat_completion_sources
            .allows_source(source)
        {
            return Ok(());
        }

        Err(ApplicationError::PermissionDenied(format!(
            "iOS policy disabled chat completion source: {}",
            source.key()
        )))
    }

    fn ensure_endpoint_overrides_allowed_for_status(
        &self,
        source: ChatCompletionSource,
        dto: &ChatCompletionStatusRequestDto,
    ) -> Result<(), ApplicationError> {
        if !self.ios_policy_is_active() {
            return Ok(());
        }

        if self.ios_policy.capabilities.llm.endpoint_overrides {
            return Ok(());
        }

        if source == ChatCompletionSource::Custom {
            return Err(ApplicationError::PermissionDenied(
                "iOS policy disabled capability: llm.endpoint_overrides".to_string(),
            ));
        }

        let mut overridden = Vec::new();
        if !dto.reverse_proxy.trim().is_empty() {
            overridden.push("reverse_proxy");
        }
        if !dto.proxy_password.trim().is_empty() {
            overridden.push("proxy_password");
        }
        if !dto.custom_url.trim().is_empty() {
            overridden.push("custom_url");
        }
        if !dto.custom_include_headers.trim().is_empty() {
            overridden.push("custom_include_headers");
        }

        if overridden.is_empty() {
            return Ok(());
        }

        Err(ApplicationError::PermissionDenied(format!(
            "iOS policy disabled capability: llm.endpoint_overrides (used: {})",
            overridden.join(", ")
        )))
    }

    fn ensure_endpoint_overrides_allowed_for_payload(
        &self,
        source: ChatCompletionSource,
        payload: &Map<String, Value>,
    ) -> Result<(), ApplicationError> {
        if !self.ios_policy_is_active() {
            return Ok(());
        }

        if self.ios_policy.capabilities.llm.endpoint_overrides {
            return Ok(());
        }

        if source == ChatCompletionSource::Custom {
            return Err(ApplicationError::PermissionDenied(
                "iOS policy disabled capability: llm.endpoint_overrides".to_string(),
            ));
        }

        let mut overridden = Vec::new();
        for key in [
            "reverse_proxy",
            "proxy_password",
            "custom_url",
            "custom_include_headers",
        ] {
            let Some(value) = payload.get(key) else {
                continue;
            };

            let value = value.as_str().ok_or_else(|| {
                ApplicationError::ValidationError(format!(
                    "Chat completion request field must be a string: {}",
                    key
                ))
            })?;

            if !value.trim().is_empty() {
                overridden.push(key);
            }
        }

        if overridden.is_empty() {
            return Ok(());
        }

        Err(ApplicationError::PermissionDenied(format!(
            "iOS policy disabled capability: llm.endpoint_overrides (used: {})",
            overridden.join(", ")
        )))
    }

    fn ensure_chat_completion_features_allowed(
        &self,
        payload: &Map<String, Value>,
    ) -> Result<(), ApplicationError> {
        if !self.ios_policy_is_active() {
            return Ok(());
        }

        if let Some(value) = payload.get("enable_web_search") {
            let enabled = value.as_bool().ok_or_else(|| {
                ApplicationError::ValidationError(
                    "Chat completion request field must be a boolean: enable_web_search"
                        .to_string(),
                )
            })?;

            if enabled
                && !self
                    .ios_policy
                    .capabilities
                    .llm
                    .chat_completion_features
                    .web_search
            {
                return Err(ApplicationError::PermissionDenied(
                    "iOS policy disabled capability: llm.chat_completion_features.web_search"
                        .to_string(),
                ));
            }
        }

        let request_images_enabled = match payload.get("request_images") {
            None => false,
            Some(value) => value.as_bool().ok_or_else(|| {
                ApplicationError::ValidationError(
                    "Chat completion request field must be a boolean: request_images".to_string(),
                )
            })?,
        };

        let request_image_resolution = payload.get("request_image_resolution");
        let request_image_aspect_ratio = payload.get("request_image_aspect_ratio");

        let mut request_image_overrides = Vec::new();
        for (key, value) in [
            ("request_image_resolution", request_image_resolution),
            ("request_image_aspect_ratio", request_image_aspect_ratio),
        ] {
            let Some(value) = value else {
                continue;
            };

            let value = value.as_str().ok_or_else(|| {
                ApplicationError::ValidationError(format!(
                    "Chat completion request field must be a string: {}",
                    key
                ))
            })?;

            if !value.trim().is_empty() {
                request_image_overrides.push(key);
            }
        }

        if (request_images_enabled || !request_image_overrides.is_empty())
            && !self
                .ios_policy
                .capabilities
                .llm
                .chat_completion_features
                .request_images
        {
            let suffix = if request_image_overrides.is_empty() {
                String::new()
            } else {
                format!(" (used: {})", request_image_overrides.join(", "))
            };

            return Err(ApplicationError::PermissionDenied(format!(
                "iOS policy disabled capability: llm.chat_completion_features.request_images{}",
                suffix
            )));
        }

        Ok(())
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
        self.ensure_chat_completion_source_allowed(source)?;
        self.ensure_endpoint_overrides_allowed_for_status(source, &dto)?;

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
        self.ensure_chat_completion_source_allowed(source)?;
        self.ensure_endpoint_overrides_allowed_for_payload(source, &dto.payload)?;
        self.ensure_chat_completion_features_allowed(&dto.payload)?;

        let settings = self.load_tauritavern_settings().await?;

        let config =
            config::resolve_generate_api_config(source, &dto, &self.secret_repository).await?;
        let payload = dto.payload;
        let (endpoint_path, mut upstream_payload) = payload::build_payload(source, payload)?;
        if let Err(error) = self
            .apply_tauritavern_prompt_caching(source, &settings, &mut upstream_payload)
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
        self.ensure_chat_completion_source_allowed(source)?;
        self.ensure_endpoint_overrides_allowed_for_payload(source, &dto.payload)?;
        self.ensure_chat_completion_features_allowed(&dto.payload)?;

        let settings = self.load_tauritavern_settings().await?;

        let config =
            config::resolve_generate_api_config(source, &dto, &self.secret_repository).await?;
        let payload = dto.payload;
        let (endpoint_path, mut upstream_payload) = payload::build_payload(source, payload)?;
        if let Err(error) = self
            .apply_tauritavern_prompt_caching(source, &settings, &mut upstream_payload)
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
                    .is_some_and(|model| {
                        model.to_ascii_lowercase().starts_with("anthropic/claude")
                    });
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
            ChatCompletionSource::NanoGpt => {
                apply_nanogpt_claude_cache_control(upstream_payload, ttl);
            }
            _ => {}
        }

        Ok(())
    }
}

fn is_nanogpt_claude_model(model: &str) -> bool {
    let model = model.trim().to_ascii_lowercase();
    model.starts_with("claude-")
        || model.starts_with("claude_")
        || model.contains("/claude-")
        || model.contains("/claude_")
}

fn apply_nanogpt_claude_cache_control(payload: &mut Value, ttl: &str) -> bool {
    let is_claude = payload
        .as_object()
        .and_then(|object| object.get("model"))
        .and_then(Value::as_str)
        .map(str::trim)
        .is_some_and(is_nanogpt_claude_model);
    if !is_claude {
        return false;
    }

    let Some(object) = payload.as_object_mut() else {
        return false;
    };

    object.insert(
        "cache_control".to_string(),
        json!({
            "enabled": true,
            "ttl": ttl,
        }),
    );

    true
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::apply_nanogpt_claude_cache_control;

    #[test]
    fn nanogpt_claude_cache_control_is_inserted_for_claude_models() {
        let mut payload = json!({
            "model": "anthropic/claude-3-5-sonnet-latest",
            "messages": [{"role": "user", "content": "hello"}]
        });

        assert!(apply_nanogpt_claude_cache_control(&mut payload, "5m"));

        assert_eq!(
            payload
                .get("cache_control")
                .and_then(Value::as_object)
                .and_then(|cache_control| cache_control.get("enabled"))
                .and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            payload
                .get("cache_control")
                .and_then(Value::as_object)
                .and_then(|cache_control| cache_control.get("ttl"))
                .and_then(Value::as_str),
            Some("5m")
        );
    }

    #[test]
    fn nanogpt_claude_cache_control_is_skipped_for_non_claude_models() {
        let mut payload = json!({
            "model": "gpt-4o-mini",
            "messages": [{"role": "user", "content": "hello"}]
        });

        assert!(!apply_nanogpt_claude_cache_control(&mut payload, "5m"));
        assert!(payload.get("cache_control").is_none());
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
