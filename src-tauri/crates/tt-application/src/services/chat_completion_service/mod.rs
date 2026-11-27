use std::sync::Arc;

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use url::Url;

use crate::dto::chat_completion_dto::{
    ChatCompletionGenerateRequestDto, ChatCompletionStatusRequestDto, ChatCompletionWireRequestDto,
};
use crate::errors::ApplicationError;
use crate::services::generation_runtime::{
    GenerationRuntime, RegisteredRequest, is_quiet, until_cancelled,
};
use crate::services::hashing::hex_lower;
use tt_domain::ios_policy::{IosPolicyActivationReport, IosPolicyScope};
use tt_domain::models::claude_model::supports_one_hour_prompt_cache;
use tt_domain::models::settings::{PromptCacheTtl, TauriTavernSettings};
use tt_ports::generation_stream::{GenerationCancelReceiver, GenerationStreamSender};
use tt_ports::repositories::chat_completion_repository::{
    CHAT_COMPLETION_PROVIDER_STATE_FIELD, ChatCompletionApiConfig,
    ChatCompletionNormalizationReport, ChatCompletionRepository, ChatCompletionSource,
    ChatCompletionStreamDelta, GenerationCall,
};
use tt_ports::repositories::prompt_cache_repository::{
    PromptCacheKey, PromptCacheRepository, PromptDigestSnapshot,
};
use tt_ports::repositories::secret_repository::SecretRepository;
use tt_ports::repositories::settings_repository::SettingsRepository;

mod additional_parameters;
mod config;
mod custom_api_format;
mod custom_parameters;
pub(crate) mod exchange;
mod model_capabilities;
pub(super) mod opencode;
mod payload;
mod prompt_caching;
mod prompt_caching_plan;

use self::additional_parameters::AdditionalParameters;
use self::exchange::{
    ChatCompletionExchange, ChatCompletionProviderFormat, NormalizedChatCompletionResponse,
};

const OPENAI_SOURCE: &str = ChatCompletionSource::OpenAi.key();
pub(crate) const STABLE_CHAT_ID_FIELD: &str = "_tauritavern_stable_chat_id";
const OPENCODE_SESSION_HEADER: &str = "x-opencode-session";
const FIREWORKS_SESSION_HEADER: &str = "x-session-affinity";
const VERTEXAI_PROMPT_CACHE_SESSION_HEADER: &str = "X-Vertex-Ai-Session-Id";
const AGENT_STRUCTURAL_BODY_OVERRIDE_KEYS: &[&str] = &[
    "messages",
    "input",
    "tools",
    "tool_choice",
    "stream",
    "n",
    "previous_response_id",
    CHAT_COMPLETION_PROVIDER_STATE_FIELD,
];

struct ChatCompletionExecution {
    source: ChatCompletionSource,
    provider_format: ChatCompletionProviderFormat,
    body: Value,
    normalization_report: ChatCompletionNormalizationReport,
}

struct PreparedChatCompletionRequest {
    source: ChatCompletionSource,
    config: ChatCompletionApiConfig,
    endpoint_path: String,
    upstream_payload: Value,
    provider_format: ChatCompletionProviderFormat,
    /// The prompt digests this request would leave behind; sending saves them, a preview
    /// drops them.
    prompt_digests: Option<(PromptCacheKey, PromptDigestSnapshot)>,
}

pub struct ChatCompletionService {
    chat_completion_repository: Arc<dyn ChatCompletionRepository>,
    secret_repository: Arc<dyn SecretRepository>,
    settings_repository: Arc<dyn SettingsRepository>,
    prompt_cache_repository: Arc<dyn PromptCacheRepository>,
    generation_runtime: Arc<GenerationRuntime>,
    ios_policy: IosPolicyActivationReport,
}

impl ChatCompletionService {
    pub fn new(
        chat_completion_repository: Arc<dyn ChatCompletionRepository>,
        secret_repository: Arc<dyn SecretRepository>,
        settings_repository: Arc<dyn SettingsRepository>,
        prompt_cache_repository: Arc<dyn PromptCacheRepository>,
        generation_runtime: Arc<GenerationRuntime>,
        ios_policy: IosPolicyActivationReport,
    ) -> Self {
        Self {
            chat_completion_repository,
            secret_repository,
            settings_repository,
            prompt_cache_repository,
            generation_runtime,
            ios_policy,
        }
    }

    fn ios_policy_is_active(&self) -> bool {
        self.ios_policy.scope == IosPolicyScope::Ios
    }

    fn resolve_status_source(
        &self,
        dto: &ChatCompletionStatusRequestDto,
    ) -> Result<Option<ChatCompletionSource>, ApplicationError> {
        if dto.bypass_status_check {
            return Ok(None);
        }

        let source = self.resolve_source(&dto.chat_completion_source)?;
        self.ensure_chat_completion_source_allowed(source)?;
        self.ensure_endpoint_overrides_allowed_for_status(dto)?;

        match source {
            ChatCompletionSource::VertexAi
            | ChatCompletionSource::MiniMax
            | ChatCompletionSource::Perplexity => Ok(None),
            _ => Ok(Some(source)),
        }
    }

    fn resolve_generate_source(
        &self,
        dto: &ChatCompletionGenerateRequestDto,
    ) -> Result<ChatCompletionSource, ApplicationError> {
        let source = self.resolve_source(
            dto.get_string("chat_completion_source")
                .unwrap_or(OPENAI_SOURCE),
        )?;
        self.ensure_chat_completion_source_allowed(source)?;
        self.ensure_endpoint_overrides_allowed_for_payload(&dto.payload)?;
        self.ensure_chat_completion_features_allowed(&dto.payload)?;
        Ok(source)
    }

    pub fn resolve_status_user_endpoint(
        &self,
        dto: &ChatCompletionStatusRequestDto,
    ) -> Result<Option<Url>, ApplicationError> {
        let Some(source) = self.resolve_status_source(dto)? else {
            return Ok(None);
        };

        config::resolve_user_configured_endpoint(source, &dto.reverse_proxy, &dto.custom_url)
    }

    pub fn resolve_generate_user_endpoint(
        &self,
        dto: &ChatCompletionGenerateRequestDto,
    ) -> Result<Option<Url>, ApplicationError> {
        let source = self.resolve_generate_source(dto)?;
        let custom_url = config::get_payload_string(&dto.payload, "custom_url")?;

        config::resolve_user_configured_endpoint(
            source,
            dto.get_string("reverse_proxy").unwrap_or_default(),
            &custom_url,
        )
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
            .allows_chat_completion_source(source)
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
        dto: &ChatCompletionStatusRequestDto,
    ) -> Result<(), ApplicationError> {
        if !self.ios_policy_is_active() {
            return Ok(());
        }

        if self.ios_policy.capabilities.llm.endpoint_overrides {
            return Ok(());
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
        let custom_include_headers = additional_parameters::normalize_custom_parameter_field(
            &dto.custom_include_headers,
            "custom_include_headers",
        )?;
        if !custom_include_headers.trim().is_empty() {
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
        payload: &Map<String, Value>,
    ) -> Result<(), ApplicationError> {
        if !self.ios_policy_is_active() {
            return Ok(());
        }

        if self.ios_policy.capabilities.llm.endpoint_overrides {
            return Ok(());
        }

        let mut overridden = Vec::new();
        for key in ["reverse_proxy", "proxy_password", "custom_url"] {
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

        for key in [
            "custom_include_body",
            "custom_exclude_body",
            "custom_include_headers",
        ] {
            let Some(value) = payload.get(key) else {
                continue;
            };

            let value = additional_parameters::normalize_custom_parameter_field(value, key)?;

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
        let Some(source) = self.resolve_status_source(&dto)? else {
            return Ok(json!({
                "bypass": true,
                "data": []
            }));
        };

        let model_list_source = resolve_status_model_list_source(source, &dto.custom_api_format)?;
        let config =
            config::resolve_status_api_config(source, &dto, &self.secret_repository).await?;

        self.chat_completion_repository
            .list_models(model_list_source, &config)
            .await
            .map_err(ApplicationError::from)
    }

    async fn execute_generate(
        &self,
        dto: ChatCompletionGenerateRequestDto,
        on_delta: Option<&mut (dyn FnMut(ChatCompletionStreamDelta) + Send)>,
    ) -> Result<ChatCompletionExecution, ApplicationError> {
        let prepared = self.prepare_send(dto).await?;
        let provider_format = prepared.provider_format;

        let response = match on_delta {
            Some(on_delta) => {
                self.chat_completion_repository
                    .generate_with_deltas(
                        prepared.source,
                        &prepared.config,
                        &prepared.endpoint_path,
                        &prepared.upstream_payload,
                        on_delta,
                    )
                    .await
            }
            None => {
                self.chat_completion_repository
                    .generate(
                        prepared.source,
                        &prepared.config,
                        &prepared.endpoint_path,
                        &prepared.upstream_payload,
                    )
                    .await
            }
        }
        .map_err(ApplicationError::from)?;

        Ok(ChatCompletionExecution {
            source: prepared.source,
            provider_format,
            body: response.body,
            normalization_report: response.normalization_report,
        })
    }

    pub async fn preview_request(
        &self,
        dto: ChatCompletionGenerateRequestDto,
    ) -> Result<ChatCompletionWireRequestDto, ApplicationError> {
        let stream = dto
            .payload
            .get("stream")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let prepared = self.prepare_generate_request(dto).await?;
        let request = self
            .chat_completion_repository
            .describe_request(
                prepared.source,
                &prepared.config,
                &prepared.endpoint_path,
                &prepared.upstream_payload,
                if stream {
                    GenerationCall::GenerateStream
                } else {
                    GenerationCall::Generate
                },
            )
            .map_err(ApplicationError::from)?;
        let (http, body) = (request.http, request.body.into_owned());
        Ok(ChatCompletionWireRequestDto {
            source: prepared.source,
            base_url: prepared.config.base_url,
            endpoint_path: prepared.endpoint_path,
            payload: prepared.upstream_payload,
            body,
            http,
        })
    }

    async fn prepare_send(
        &self,
        dto: ChatCompletionGenerateRequestDto,
    ) -> Result<PreparedChatCompletionRequest, ApplicationError> {
        let mut prepared = self.prepare_generate_request(dto).await?;
        if let Some((key, snapshot)) = prepared.prompt_digests.take() {
            self.prompt_cache_repository
                .save_prompt_digests(key, snapshot)
                .await
                .map_err(ApplicationError::from)?;
        }
        Ok(prepared)
    }

    async fn prepare_generate_request(
        &self,
        dto: ChatCompletionGenerateRequestDto,
    ) -> Result<PreparedChatCompletionRequest, ApplicationError> {
        let source = self.resolve_generate_source(&dto)?;
        let additional_parameters = AdditionalParameters::from_payload(&dto.payload)?;
        Self::ensure_agent_body_overrides_allowed(&dto.payload, &additional_parameters)?;
        let provider_format = ChatCompletionProviderFormat::from_payload(source, &dto.payload)?;
        let settings = self.load_tauritavern_settings().await?;
        let prompt_caching_hints =
            prompt_caching_plan::PromptCachingRequestHints::from_payload(&dto.payload)?;
        let mut config = config::resolve_generate_api_config(
            source,
            &dto,
            &additional_parameters,
            &self.secret_repository,
        )
        .await?;
        apply_chat_session_header(source, &dto.payload, &mut config)?;
        let (endpoint_path, mut upstream_payload) = payload::build_payload(source, dto.payload)?;
        additional_parameters.apply_body_overrides(&mut upstream_payload)?;
        let mut prepared = PreparedChatCompletionRequest {
            source,
            config,
            endpoint_path,
            upstream_payload,
            provider_format,
            prompt_digests: None,
        };
        self.apply_tauritavern_prompt_caching(&mut prepared, &settings, prompt_caching_hints)
            .await?;
        Ok(prepared)
    }

    pub(crate) async fn generate_exchange(
        &self,
        dto: ChatCompletionGenerateRequestDto,
        on_delta: Option<&mut (dyn FnMut(ChatCompletionStreamDelta) + Send)>,
    ) -> Result<ChatCompletionExchange, ApplicationError> {
        let execution = self.execute_generate(dto, on_delta).await?;
        let normalized_response = NormalizedChatCompletionResponse::from_value(execution.body)?;

        Ok(ChatCompletionExchange {
            source: execution.source,
            provider_format: execution.provider_format,
            normalized_response,
            normalization_report: execution.normalization_report,
        })
    }

    pub async fn generate_request(
        &self,
        request: RegisteredRequest<'_>,
        dto: ChatCompletionGenerateRequestDto,
    ) -> Result<Value, ApplicationError> {
        let quiet = is_quiet(&dto.payload);
        self.generation_runtime
            .run_request(request, quiet, None, async {
                Ok(self.execute_generate(dto, None).await?.body)
            })
            .await
    }

    pub(crate) async fn generate_exchange_with_cancel(
        &self,
        dto: ChatCompletionGenerateRequestDto,
        on_delta: Option<&mut (dyn FnMut(ChatCompletionStreamDelta) + Send)>,
        cancel: GenerationCancelReceiver,
    ) -> Result<ChatCompletionExchange, ApplicationError> {
        until_cancelled(self.generate_exchange(dto, on_delta), cancel).await
    }

    pub async fn generate_stream(
        &self,
        dto: ChatCompletionGenerateRequestDto,
        sender: GenerationStreamSender,
        cancel: GenerationCancelReceiver,
    ) -> Result<(), ApplicationError> {
        let prepared = self.prepare_send(dto).await?;

        self.chat_completion_repository
            .generate_stream(
                prepared.source,
                &prepared.config,
                &prepared.endpoint_path,
                &prepared.upstream_payload,
                sender,
                cancel,
            )
            .await
            .map_err(ApplicationError::from)
    }

    pub async fn start_stream(
        self: &Arc<Self>,
        stream_id: String,
        dto: ChatCompletionGenerateRequestDto,
    ) -> Result<(), ApplicationError> {
        let quiet = is_quiet(&dto.payload);
        let service = self.clone();
        self.generation_runtime
            .start_stream(stream_id, quiet, None, move |sender, cancel| async move {
                service.generate_stream(dto, sender, cancel).await
            })
            .await
    }

    pub async fn close_provider_session(&self, session_id: &str) {
        self.chat_completion_repository
            .close_provider_session(session_id)
            .await;
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

    fn ensure_agent_body_overrides_allowed(
        payload: &Map<String, Value>,
        additional_parameters: &AdditionalParameters,
    ) -> Result<(), ApplicationError> {
        if !payload.contains_key(CHAT_COMPLETION_PROVIDER_STATE_FIELD) {
            return Ok(());
        }

        additional_parameters
            .ensure_body_overrides_do_not_touch(AGENT_STRUCTURAL_BODY_OVERRIDE_KEYS)
    }

    async fn apply_tauritavern_prompt_caching(
        &self,
        prepared: &mut PreparedChatCompletionRequest,
        settings: &TauriTavernSettings,
        hints: prompt_caching_plan::PromptCachingRequestHints,
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

        let plan = prompt_caching_plan::resolve_prompt_caching_plan(
            prepared.source,
            &prepared.endpoint_path,
            &prepared.config,
            &prepared.upstream_payload,
            hints,
        )?;
        let Some(plan) = plan else {
            return Ok(());
        };

        if prompt_caching::contains_cache_control(&prepared.upstream_payload) {
            // User-supplied cache_control wins over automatic prompt caching.
            if let prompt_caching_plan::PromptCachingPlan::VertexClaude { key, .. } = &plan {
                apply_vertexai_prompt_cache_session_header(&mut prepared.config, key);
            }
            return Ok(());
        }

        match plan {
            prompt_caching_plan::PromptCachingPlan::Claude {
                key,
                anthropic_beta_header_mode,
            } => {
                let previous = self
                    .prompt_cache_repository
                    .load_prompt_digests(key.clone())
                    .await
                    .map_err(ApplicationError::from)?;
                let snapshot = prompt_caching::apply_claude_prompt_caching(
                    &mut prepared.upstream_payload,
                    previous.as_ref(),
                    ttl,
                );
                prepared.prompt_digests = Some((key, snapshot));
                prepared.config.anthropic_beta_header_mode = anthropic_beta_header_mode;
            }
            prompt_caching_plan::PromptCachingPlan::OpenAiCompatibleClaude { key } => {
                let previous = self
                    .prompt_cache_repository
                    .load_prompt_digests(key.clone())
                    .await
                    .map_err(ApplicationError::from)?;
                let snapshot = prompt_caching::apply_openai_compatible_claude_prompt_caching(
                    &mut prepared.upstream_payload,
                    previous.as_ref(),
                    ttl,
                );
                prepared.prompt_digests = Some((key, snapshot));
            }
            prompt_caching_plan::PromptCachingPlan::VertexClaude { key, model } => {
                ensure_vertexai_claude_prompt_cache_ttl(ttl, &model)?;
                let previous = self
                    .prompt_cache_repository
                    .load_prompt_digests(key.clone())
                    .await
                    .map_err(ApplicationError::from)?;
                let snapshot = prompt_caching::apply_claude_explicit_prompt_caching(
                    &mut prepared.upstream_payload,
                    previous.as_ref(),
                    ttl,
                );
                apply_vertexai_prompt_cache_session_header(&mut prepared.config, &key);
                prepared.prompt_digests = Some((key, snapshot));
            }
            prompt_caching_plan::PromptCachingPlan::NanoGptClaude => {
                apply_nanogpt_claude_cache_control(&mut prepared.upstream_payload, ttl);
            }
        }

        Ok(())
    }
}

fn ensure_vertexai_claude_prompt_cache_ttl(ttl: &str, model: &str) -> Result<(), ApplicationError> {
    if ttl == "1h" && !supports_one_hour_prompt_cache(model) {
        return Err(ApplicationError::ValidationError(format!(
            "Google Vertex AI Claude model {model} does not support 1h prompt cache TTL; use 5m or disable prompt caching."
        )));
    }

    Ok(())
}

fn apply_vertexai_prompt_cache_session_header(
    config: &mut ChatCompletionApiConfig,
    key: &PromptCacheKey,
) {
    if !is_vertexai_global_location_base_url(&config.base_url)
        || has_configured_header(config, VERTEXAI_PROMPT_CACHE_SESSION_HEADER)
    {
        return;
    }

    let digest_input = format!("{}|{:?}", config.base_url.trim(), key);
    let digest = Sha256::digest(digest_input.as_bytes());
    let session_id = format!("tt-pc-{}", &hex_lower(&digest)[..32]);
    config
        .extra_headers
        .insert(VERTEXAI_PROMPT_CACHE_SESSION_HEADER.to_string(), session_id);
}

fn is_vertexai_global_location_base_url(base_url: &str) -> bool {
    base_url
        .trim()
        .to_ascii_lowercase()
        .contains("/locations/global")
}

fn has_configured_header(config: &ChatCompletionApiConfig, header_name: &str) -> bool {
    config
        .extra_headers
        .keys()
        .chain(config.additional_headers.keys())
        .any(|key| key.eq_ignore_ascii_case(header_name))
}

fn apply_chat_session_header(
    source: ChatCompletionSource,
    payload: &Map<String, Value>,
    config: &mut ChatCompletionApiConfig,
) -> Result<(), ApplicationError> {
    let stable_chat_id = payload
        .get(STABLE_CHAT_ID_FIELD)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty());

    match source {
        ChatCompletionSource::OpenCode
            if !has_configured_header(config, OPENCODE_SESSION_HEADER) =>
        {
            let stable_chat_id = stable_chat_id.ok_or_else(|| {
                ApplicationError::ValidationError(
                    "OpenCode requires a stable chat id for x-opencode-session".to_string(),
                )
            })?;
            const ENCODE_SET: &AsciiSet = &NON_ALPHANUMERIC
                .remove(b'-')
                .remove(b'.')
                .remove(b'_')
                .remove(b'~');
            config.extra_headers.insert(
                OPENCODE_SESSION_HEADER.to_string(),
                utf8_percent_encode(stable_chat_id, ENCODE_SET).to_string(),
            );
        }
        // A group chat id is its file name, so Fireworks receives a digest instead.
        ChatCompletionSource::Fireworks
            if !has_configured_header(config, FIREWORKS_SESSION_HEADER) =>
        {
            if let Some(stable_chat_id) = stable_chat_id {
                let digest = Sha256::digest(stable_chat_id.as_bytes());
                config.extra_headers.insert(
                    FIREWORKS_SESSION_HEADER.to_string(),
                    hex_lower(&digest)[..16].to_string(),
                );
            }
        }
        _ => {}
    }
    Ok(())
}

fn resolve_status_model_list_source(
    source: ChatCompletionSource,
    custom_api_format: &str,
) -> Result<ChatCompletionSource, ApplicationError> {
    if source != ChatCompletionSource::Custom {
        return Ok(source);
    }

    Ok(custom_api_format::CustomApiFormat::parse(custom_api_format)?.model_list_source())
}

fn apply_nanogpt_claude_cache_control(payload: &mut Value, ttl: &str) -> bool {
    let is_claude = payload
        .as_object()
        .and_then(|object| object.get("model"))
        .and_then(Value::as_str)
        .map(str::trim)
        .is_some_and(prompt_caching_plan::is_nanogpt_claude_model_name);
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

    use super::{
        AdditionalParameters, ChatCompletionService, apply_chat_session_header,
        apply_nanogpt_claude_cache_control, apply_vertexai_prompt_cache_session_header,
        ensure_vertexai_claude_prompt_cache_ttl, resolve_status_model_list_source,
    };
    use tt_ports::repositories::chat_completion_repository::{
        AnthropicBetaHeaderMode, CHAT_COMPLETION_PROVIDER_STATE_FIELD, ChatCompletionApiConfig,
        ChatCompletionSource,
    };
    use tt_ports::repositories::prompt_cache_repository::PromptCacheKey;

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
    fn vertexai_claude_rejects_one_hour_cache_for_old_models() {
        let error = ensure_vertexai_claude_prompt_cache_ttl("1h", "claude-3-7-sonnet@20250219")
            .expect_err("old model should reject 1h ttl");

        assert!(error.to_string().contains("does not support 1h"));
        ensure_vertexai_claude_prompt_cache_ttl("1h", "claude-sonnet-4-5@20250929")
            .expect("modern model should allow 1h ttl");
    }

    #[test]
    fn vertexai_global_prompt_cache_adds_session_header() {
        let mut config = ChatCompletionApiConfig {
            base_url: "https://aiplatform.googleapis.com/v1/projects/p/locations/global"
                .to_string(),
            user_configured_endpoint: false,
            api_key: String::new(),
            authorization_header: None,
            vertexai_service_account_json: None,
            extra_headers: Default::default(),
            additional_headers: Default::default(),
            anthropic_beta_header_mode: AnthropicBetaHeaderMode::None,
            aws_bedrock_custom_response_path: None,
            aws_bedrock_custom_stream_path: None,
        };
        let key = PromptCacheKey::VertexAiClaude {
            scope: "scope".to_string(),
        };

        apply_vertexai_prompt_cache_session_header(&mut config, &key);

        assert!(config.extra_headers.contains_key("X-Vertex-Ai-Session-Id"));
    }

    #[test]
    fn chat_sessions_derive_their_headers_from_the_stable_chat_id() {
        let config = || ChatCompletionApiConfig {
            base_url: "https://example.test/v1".to_string(),
            user_configured_endpoint: false,
            api_key: "secret".to_string(),
            authorization_header: None,
            vertexai_service_account_json: None,
            extra_headers: Default::default(),
            additional_headers: Default::default(),
            anthropic_beta_header_mode: AnthropicBetaHeaderMode::None,
            aws_bedrock_custom_response_path: None,
            aws_bedrock_custom_stream_path: None,
        };
        let missing_id = json!({});
        let payload = json!({ "_tauritavern_stable_chat_id": " stable-空% \n" });

        let mut opencode = config();
        assert!(
            apply_chat_session_header(
                ChatCompletionSource::OpenCode,
                missing_id.as_object().unwrap(),
                &mut opencode,
            )
            .is_err()
        );
        apply_chat_session_header(
            ChatCompletionSource::OpenCode,
            payload.as_object().unwrap(),
            &mut opencode,
        )
        .unwrap();
        assert_eq!(
            opencode.extra_headers["x-opencode-session"],
            "%20stable-%E7%A9%BA%25%20%0A"
        );

        let mut fireworks = config();
        apply_chat_session_header(
            ChatCompletionSource::Fireworks,
            missing_id.as_object().unwrap(),
            &mut fireworks,
        )
        .unwrap();
        assert!(fireworks.extra_headers.is_empty());
        apply_chat_session_header(
            ChatCompletionSource::Fireworks,
            payload.as_object().unwrap(),
            &mut fireworks,
        )
        .unwrap();
        let affinity = &fireworks.extra_headers["x-session-affinity"];
        assert_eq!(affinity.len(), 16);
        assert!(affinity.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }

    #[test]
    fn custom_claude_messages_status_uses_claude_transport() {
        let source =
            resolve_status_model_list_source(ChatCompletionSource::Custom, "claude_messages")
                .expect("status transport should resolve");
        assert_eq!(source, ChatCompletionSource::Claude);
    }

    #[test]
    fn custom_gemini_interactions_status_uses_makersuite_transport() {
        let source =
            resolve_status_model_list_source(ChatCompletionSource::Custom, "gemini_interactions")
                .expect("status transport should resolve");
        assert_eq!(source, ChatCompletionSource::Makersuite);
    }

    #[test]
    fn agent_body_overrides_cannot_change_stream_or_choice_count() {
        for (override_fields, expected_error) in [
            (
                json!({
                    (CHAT_COMPLETION_PROVIDER_STATE_FIELD): {},
                    "custom_include_body": { "stream": true }
                }),
                true,
            ),
            (
                json!({
                    (CHAT_COMPLETION_PROVIDER_STATE_FIELD): {},
                    "custom_exclude_body": ["n"]
                }),
                true,
            ),
            (
                json!({
                    "custom_include_body": { "stream": true },
                    "custom_exclude_body": ["n"]
                }),
                false,
            ),
        ] {
            let payload = override_fields
                .as_object()
                .expect("payload must be an object");
            let parameters = AdditionalParameters::from_payload(payload).expect("override parses");
            assert_eq!(
                ChatCompletionService::ensure_agent_body_overrides_allowed(payload, &parameters)
                    .is_err(),
                expected_error
            );
        }
    }

    #[tokio::test]
    async fn preview_request_matches_the_sent_request_without_sending_or_saving_digests() {
        use std::borrow::Cow;
        use std::collections::HashMap;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        use async_trait::async_trait;
        use tokio::sync::Mutex;
        use tt_contracts::observability::{RequestDescriptionDto, RequestTransportDto};
        use tt_domain::errors::DomainError;
        use tt_domain::ios_policy::{IosPolicyScope, resolve_ios_policy_activation_report};
        use tt_domain::models::secret::Secrets;
        use tt_domain::models::settings::{PromptCacheTtl, TauriTavernSettings};
        use tt_ports::generation_stream::{GenerationCancelReceiver, GenerationStreamSender};
        use tt_ports::repositories::chat_completion_repository::{
            ChatCompletionNormalizationReport, ChatCompletionRepository,
            ChatCompletionRepositoryGenerateResponse, ChatCompletionStreamDelta, GenerationCall,
            WireRequest,
        };
        use tt_ports::repositories::prompt_cache_repository::{
            PromptCacheRepository, PromptDigestSnapshot,
        };
        use tt_ports::repositories::secret_repository::SecretRepository;

        use super::prompt_caching;
        use crate::dto::chat_completion_dto::ChatCompletionGenerateRequestDto;
        use crate::services::agent_run_retention_test_support::TestSettingsRepository;
        use crate::services::generation_runtime::GenerationRuntime;

        #[derive(Debug, Clone, PartialEq)]
        struct RecordedCall {
            source: ChatCompletionSource,
            base_url: String,
            endpoint_path: String,
            payload: Value,
        }

        #[derive(Default)]
        struct RecordingChatRepo {
            calls: Mutex<Vec<RecordedCall>>,
        }

        #[async_trait]
        impl ChatCompletionRepository for RecordingChatRepo {
            async fn list_models(
                &self,
                _source: ChatCompletionSource,
                _config: &ChatCompletionApiConfig,
            ) -> Result<Value, DomainError> {
                unreachable!()
            }

            async fn generate(
                &self,
                source: ChatCompletionSource,
                config: &ChatCompletionApiConfig,
                endpoint_path: &str,
                payload: &Value,
            ) -> Result<ChatCompletionRepositoryGenerateResponse, DomainError> {
                self.calls.lock().await.push(RecordedCall {
                    source,
                    base_url: config.base_url.clone(),
                    endpoint_path: endpoint_path.to_string(),
                    payload: payload.clone(),
                });
                Ok(ChatCompletionRepositoryGenerateResponse::new(
                    json!({
                        "choices": [{
                            "finish_reason": "stop",
                            "message": { "role": "assistant", "content": "ok" }
                        }]
                    }),
                    ChatCompletionNormalizationReport::default(),
                ))
            }

            async fn generate_stream(
                &self,
                _source: ChatCompletionSource,
                _config: &ChatCompletionApiConfig,
                _endpoint_path: &str,
                _payload: &Value,
                _sender: GenerationStreamSender,
                _cancel: GenerationCancelReceiver,
            ) -> Result<(), DomainError> {
                unreachable!()
            }

            async fn generate_with_deltas(
                &self,
                _source: ChatCompletionSource,
                _config: &ChatCompletionApiConfig,
                _endpoint_path: &str,
                _payload: &Value,
                _on_delta: &mut (dyn FnMut(ChatCompletionStreamDelta) + Send),
            ) -> Result<ChatCompletionRepositoryGenerateResponse, DomainError> {
                unreachable!()
            }

            async fn close_provider_session(&self, _session_id: &str) {}

            fn describe_request<'a>(
                &self,
                _source: ChatCompletionSource,
                config: &ChatCompletionApiConfig,
                endpoint_path: &str,
                payload: &'a Value,
                _call: GenerationCall,
            ) -> Result<WireRequest<'a>, DomainError> {
                Ok(WireRequest {
                    http: RequestDescriptionDto {
                        transport: RequestTransportDto::Http,
                        method: "POST".to_string(),
                        url: format!("{}{endpoint_path}", config.base_url),
                        headers: Vec::new(),
                    },
                    body: Cow::Borrowed(payload),
                })
            }
        }

        struct StaticSecretRepo;

        #[async_trait]
        impl SecretRepository for StaticSecretRepo {
            async fn save(&self, _secrets: &Secrets) -> Result<(), DomainError> {
                unreachable!()
            }
            async fn load(&self) -> Result<Secrets, DomainError> {
                unreachable!()
            }
            async fn clear_cache(&self) -> Result<(), DomainError> {
                Ok(())
            }
            async fn write_secret(&self, _: &str, _: &str, _: &str) -> Result<String, DomainError> {
                unreachable!()
            }
            async fn read_secret(
                &self,
                _key: &str,
                _id: Option<&str>,
            ) -> Result<Option<String>, DomainError> {
                Ok(Some("sk-ant-test".to_string()))
            }
            async fn delete_secret(&self, _: &str, _: Option<&str>) -> Result<(), DomainError> {
                unreachable!()
            }
            async fn rotate_secret(&self, _: &str, _: &str) -> Result<(), DomainError> {
                unreachable!()
            }
            async fn rename_secret(&self, _: &str, _: &str, _: &str) -> Result<(), DomainError> {
                unreachable!()
            }
        }

        #[derive(Default)]
        struct RecordingPromptCacheRepo {
            snapshots: Mutex<HashMap<PromptCacheKey, PromptDigestSnapshot>>,
            saves: AtomicUsize,
        }

        #[async_trait]
        impl PromptCacheRepository for RecordingPromptCacheRepo {
            async fn load_prompt_digests(
                &self,
                key: PromptCacheKey,
            ) -> Result<Option<PromptDigestSnapshot>, DomainError> {
                Ok(self.snapshots.lock().await.get(&key).cloned())
            }

            async fn save_prompt_digests(
                &self,
                key: PromptCacheKey,
                snapshot: PromptDigestSnapshot,
            ) -> Result<(), DomainError> {
                self.saves.fetch_add(1, Ordering::SeqCst);
                self.snapshots.lock().await.insert(key, snapshot);
                Ok(())
            }
        }

        let chat_repo = Arc::new(RecordingChatRepo::default());
        let cache_repo = Arc::new(RecordingPromptCacheRepo::default());
        let settings_repo = TestSettingsRepository::new();
        let mut settings = TauriTavernSettings::default();
        settings.models.claude.prompt_cache_ttl = PromptCacheTtl::FiveMinutes;
        settings_repo.store_tauritavern_settings(settings).await;

        let runtime = Arc::new(GenerationRuntime::new(None, false));
        let ios_policy =
            resolve_ios_policy_activation_report(IosPolicyScope::Ignored, None, None).unwrap();
        let service = ChatCompletionService::new(
            chat_repo.clone(),
            Arc::new(StaticSecretRepo),
            settings_repo,
            cache_repo.clone(),
            runtime.clone(),
            ios_policy,
        );

        let dto_from = |value: Value| ChatCompletionGenerateRequestDto {
            payload: value.as_object().unwrap().clone(),
        };

        let turn1 = dto_from(json!({
            "chat_completion_source": "claude",
            "model": "claude-sonnet-4-5",
            "messages": [
                { "role": "system", "content": "System rules" },
                { "role": "user", "content": "Context block" },
                { "role": "assistant", "content": "Reply 1" },
                { "role": "user", "content": "Turn 1 user" }
            ],
            "stream": false
        }));
        let req1 = runtime.register_request("req_1").unwrap();
        service.generate_request(req1, turn1).await.unwrap();
        assert_eq!(chat_repo.calls.lock().await.len(), 1);
        assert_eq!(cache_repo.saves.load(Ordering::SeqCst), 1);

        let turn2_json = json!({
            "chat_completion_source": "claude",
            "model": "claude-sonnet-4-5",
            "top_k": 40,
            "custom_include_body": "{\"metadata\":{\"user_id\":\"u_42\"}}",
            "custom_exclude_body": "[\"top_k\"]",
            "messages": [
                { "role": "system", "content": "System rules" },
                { "role": "user", "content": "Context block" },
                { "role": "assistant", "content": "Reply 1" },
                { "role": "user", "content": "Turn 1 user" },
                { "role": "assistant", "content": "Reply 2" },
                { "role": "user", "content": "Turn 2 user" }
            ],
            "stream": false
        });

        let preview = service
            .preview_request(dto_from(turn2_json.clone()))
            .await
            .unwrap();
        assert_eq!(chat_repo.calls.lock().await.len(), 1);
        assert_eq!(cache_repo.saves.load(Ordering::SeqCst), 1);
        assert_eq!(
            preview.payload.pointer("/metadata/user_id"),
            Some(&json!("u_42"))
        );
        assert!(preview.payload.get("top_k").is_none());
        assert!(prompt_caching::contains_cache_control(&preview.payload));

        let req2 = runtime.register_request("req_2").unwrap();
        service
            .generate_request(req2, dto_from(turn2_json))
            .await
            .unwrap();
        assert_eq!(cache_repo.saves.load(Ordering::SeqCst), 2);

        let sent = chat_repo.calls.lock().await[1].clone();
        assert_eq!(sent.source, preview.source);
        assert_eq!(sent.base_url, preview.base_url);
        assert_eq!(sent.endpoint_path, preview.endpoint_path);
        assert_eq!(sent.payload, preview.payload);
        assert_eq!(sent.payload, preview.body);
        assert_eq!(
            preview.http.url,
            format!("{}{}", sent.base_url, sent.endpoint_path)
        );
    }
}
