use std::sync::Arc;

use serde_json::Value;

use crate::application::dto::provider_metadata_dto::{
    ProviderModelProvidersRequestDto, SiliconFlowEmbeddingModelsRequestDto,
    WorkersAiModelsRequestDto,
};
use crate::application::errors::ApplicationError;
use crate::domain::ios_policy::{IosPolicyActivationReport, IosPolicyScope};
use crate::domain::models::secret::SecretKeys;
use crate::domain::repositories::chat_completion_repository::ChatCompletionSource;
use crate::domain::repositories::provider_metadata_repository::{
    NanoGptCredits, NanoGptModelProviders, OpenRouterCredits, ProviderMetadataRepository,
    SiliconFlowEndpoint,
};
use crate::domain::repositories::secret_repository::SecretRepository;

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

    pub async fn openrouter_model_providers(
        &self,
        dto: ProviderModelProvidersRequestDto,
    ) -> Result<Vec<String>, ApplicationError> {
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

    pub async fn siliconflow_embedding_models(
        &self,
        dto: SiliconFlowEmbeddingModelsRequestDto,
    ) -> Result<Vec<Value>, ApplicationError> {
        self.ensure_chat_completion_source_allowed(ChatCompletionSource::SiliconFlow)?;
        let api_key = self
            .read_required_secret(SecretKeys::SILICONFLOW, "SiliconFlow")
            .await?;
        let endpoint = parse_siliconflow_endpoint(&dto.siliconflow_endpoint)?;

        self.provider_metadata_repository
            .siliconflow_embedding_models(&api_key, endpoint)
            .await
            .map_err(ApplicationError::from)
    }

    pub async fn workers_ai_embedding_models(
        &self,
        dto: WorkersAiModelsRequestDto,
    ) -> Result<Vec<Value>, ApplicationError> {
        self.ensure_chat_completion_source_allowed(ChatCompletionSource::WorkersAi)?;
        let api_key = self
            .read_required_secret(SecretKeys::WORKERS_AI, "Cloudflare Workers AI")
            .await?;
        let account_id = required_string(&dto.workers_ai_account_id, "workers_ai_account_id")?;

        self.provider_metadata_repository
            .workers_ai_embedding_models(&api_key, &account_id)
            .await
            .map_err(ApplicationError::from)
    }

    pub async fn workers_ai_multimodal_models(
        &self,
        dto: WorkersAiModelsRequestDto,
    ) -> Result<Vec<String>, ApplicationError> {
        self.ensure_chat_completion_source_allowed(ChatCompletionSource::WorkersAi)?;
        let api_key = self
            .read_required_secret(SecretKeys::WORKERS_AI, "Cloudflare Workers AI")
            .await?;
        let account_id = required_string(&dto.workers_ai_account_id, "workers_ai_account_id")?;

        self.provider_metadata_repository
            .workers_ai_multimodal_models(&api_key, &account_id)
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

fn parse_siliconflow_endpoint(value: &str) -> Result<SiliconFlowEndpoint, ApplicationError> {
    SiliconFlowEndpoint::parse_frontend(value).map_err(ApplicationError::ValidationError)
}

#[cfg(test)]
mod tests {
    use super::{parse_siliconflow_endpoint, required_string};
    use crate::domain::repositories::provider_metadata_repository::SiliconFlowEndpoint;

    #[test]
    fn siliconflow_endpoint_accepts_upstream_values() {
        assert_eq!(
            parse_siliconflow_endpoint("global").unwrap(),
            SiliconFlowEndpoint::Global
        );
        assert_eq!(
            parse_siliconflow_endpoint("cn").unwrap(),
            SiliconFlowEndpoint::China
        );
    }

    #[test]
    fn siliconflow_endpoint_rejects_unknown_values() {
        let error = parse_siliconflow_endpoint("edge").unwrap_err();
        assert!(
            error
                .to_string()
                .contains("Unsupported SiliconFlow endpoint")
        );
    }

    #[test]
    fn required_string_trims_and_rejects_empty() {
        assert_eq!(required_string("  abc  ", "field").unwrap(), "abc");
        assert!(required_string("   ", "field").is_err());
    }
}
