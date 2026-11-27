use std::sync::Arc;

use crate::dto::provider_metadata_dto::{
    OpenRouterCodeExchangeRequestDto, ProviderModelProvidersRequestDto,
};
use crate::errors::ApplicationError;
use tt_domain::ios_policy::{IosPolicyActivationReport, IosPolicyScope};
use tt_domain::models::secret::SecretKeys;
use tt_ports::repositories::chat_completion_repository::ChatCompletionSource;
use tt_ports::repositories::provider_metadata_repository::{
    ModelProvider, NanoGptCredits, NanoGptModelProviders, OpenRouterCredits,
    ProviderMetadataRepository,
};
use tt_ports::repositories::secret_repository::SecretRepository;

pub struct ProviderMetadataService {
    provider_metadata_repository: Arc<dyn ProviderMetadataRepository>,
    secret_repository: Arc<dyn SecretRepository>,
    ios_policy: IosPolicyActivationReport,
}

impl ProviderMetadataService {
    pub fn new(
        provider_metadata_repository: Arc<dyn ProviderMetadataRepository>,
        secret_repository: Arc<dyn SecretRepository>,
        ios_policy: IosPolicyActivationReport,
    ) -> Self {
        Self {
            provider_metadata_repository,
            secret_repository,
            ios_policy,
        }
    }

    pub async fn openrouter_providers(&self) -> Result<Vec<ModelProvider>, ApplicationError> {
        self.ensure_chat_completion_source_allowed(ChatCompletionSource::OpenRouter)?;

        self.provider_metadata_repository
            .openrouter_providers()
            .await
            .map_err(ApplicationError::from)
    }

    pub async fn openrouter_model_providers(
        &self,
        dto: ProviderModelProvidersRequestDto,
    ) -> Result<Vec<ModelProvider>, ApplicationError> {
        self.ensure_chat_completion_source_allowed(ChatCompletionSource::OpenRouter)?;
        let model = required_string(&dto.model, "model")?;

        self.provider_metadata_repository
            .openrouter_model_providers(&model)
            .await
            .map_err(ApplicationError::from)
    }

    pub async fn openrouter_credits(&self) -> Result<OpenRouterCredits, ApplicationError> {
        self.ensure_chat_completion_source_allowed(ChatCompletionSource::OpenRouter)?;
        let api_key = self
            .read_required_secret(SecretKeys::OPENROUTER, "OpenRouter")
            .await?;

        self.provider_metadata_repository
            .openrouter_credits(&api_key)
            .await
            .map_err(ApplicationError::from)
    }

    pub async fn exchange_openrouter_code(
        &self,
        dto: OpenRouterCodeExchangeRequestDto,
    ) -> Result<String, ApplicationError> {
        self.ensure_chat_completion_source_allowed(ChatCompletionSource::OpenRouter)?;
        let code = required_string(&dto.code, "code")?;
        let code_verifier = required_string(&dto.code_verifier, "code_verifier")?;

        self.provider_metadata_repository
            .exchange_openrouter_code(&code, &code_verifier)
            .await
            .map_err(ApplicationError::from)
    }

    pub async fn nanogpt_model_providers(
        &self,
        dto: ProviderModelProvidersRequestDto,
    ) -> Result<NanoGptModelProviders, ApplicationError> {
        self.ensure_chat_completion_source_allowed(ChatCompletionSource::NanoGpt)?;
        let model = required_string(&dto.model, "model")?;

        self.provider_metadata_repository
            .nanogpt_model_providers(&model)
            .await
            .map_err(ApplicationError::from)
    }

    pub async fn nanogpt_credits(&self) -> Result<NanoGptCredits, ApplicationError> {
        self.ensure_chat_completion_source_allowed(ChatCompletionSource::NanoGpt)?;
        let api_key = self
            .read_required_secret(SecretKeys::NANOGPT, "NanoGPT")
            .await?;

        self.provider_metadata_repository
            .nanogpt_credits(&api_key)
            .await
            .map_err(ApplicationError::from)
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
            .allows_chat_completion_source(source)
        {
            return Ok(());
        }

        Err(ApplicationError::PermissionDenied(format!(
            "iOS policy disabled chat completion source: {}",
            source.key()
        )))
    }

    async fn read_required_secret(
        &self,
        secret_key: &str,
        source_name: &str,
    ) -> Result<String, ApplicationError> {
        self.secret_repository
            .read_secret(secret_key, None)
            .await?
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                ApplicationError::ValidationError(format!(
                    "{} API key is missing. Please configure {}.",
                    source_name, secret_key
                ))
            })
    }
}

fn required_string(value: &str, field_name: &str) -> Result<String, ApplicationError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(ApplicationError::ValidationError(format!(
            "{} is required",
            field_name
        )));
    }

    Ok(value.to_string())
}
