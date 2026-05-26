use std::collections::BTreeSet;
use std::sync::Arc;

use serde::Serialize;
use serde_json::{Map, Value};

use crate::application::errors::ApplicationError;
use crate::domain::models::llm_connection::{
    LLM_CONNECTION_KIND, LLM_CONNECTION_SCHEMA_VERSION, LlmConnectionDefinition, LlmConnectionId,
    LlmConnectionSecretRef, LlmConnectionSummary,
};
use crate::domain::models::secret::SecretKeys;
use crate::domain::repositories::chat_completion_repository::ChatCompletionSource;
use crate::domain::repositories::llm_connection_repository::LlmConnectionRepository;

const CONNECTION_PAYLOAD_KEYS: &[&str] = &[
    "chat_completion_source",
    "custom_api_format",
    "model",
    "custom_url",
    "secret_id",
    "reverse_proxy",
    "proxy_password",
    "custom_prompt_post_processing",
    "custom_include_headers",
    "custom_include_body",
    "custom_exclude_body",
    "vertexai_auth_mode",
    "vertexai_region",
    "vertexai_express_project_id",
    "zai_endpoint",
    "siliconflow_endpoint",
    "minimax_endpoint",
    "workers_ai_account_id",
    "nanogpt_provider",
    "nanogpt_payg_override",
];

const ALLOWED_CUSTOM_API_FORMATS: &[&str] = &[
    "openai_compat",
    "openai_responses",
    "claude_messages",
    "gemini_interactions",
];

const SOURCE_SPECIFIC_KEYS: &[&str] = &[
    "vertexai_auth_mode",
    "vertexai_region",
    "vertexai_express_project_id",
    "zai_endpoint",
    "siliconflow_endpoint",
    "minimax_endpoint",
    "workers_ai_account_id",
    "nanogpt_provider",
    "nanogpt_payg_override",
];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedLlmModelBinding {
    pub mode: String,
    pub connection_ref: String,
    pub connection_display_name: String,
    pub chat_completion_source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_api_format: Option<String>,
    pub model_id: String,
    pub secret_ref: ResolvedLlmSecretRef,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedLlmSecretRef {
    pub key: String,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label_snapshot: Option<String>,
}

pub struct LlmConnectionService {
    repository: Arc<dyn LlmConnectionRepository>,
}

impl LlmConnectionService {
    pub fn new(repository: Arc<dyn LlmConnectionRepository>) -> Self {
        Self { repository }
    }

    pub async fn list_connections(&self) -> Result<Vec<LlmConnectionSummary>, ApplicationError> {
        self.repository
            .list_connections()
            .await
            .map_err(ApplicationError::from)
    }

    pub async fn load_connection(
        &self,
        connection_id: &str,
    ) -> Result<Option<LlmConnectionDefinition>, ApplicationError> {
        let id =
            LlmConnectionId::parse(connection_id).map_err(ApplicationError::ValidationError)?;
        let Some(connection) = self.repository.load_connection(&id).await? else {
            return Ok(None);
        };
        self.validate_connection(&connection)?;
        Ok(Some(connection))
    }

    pub async fn save_connection(
        &self,
        connection: LlmConnectionDefinition,
    ) -> Result<(), ApplicationError> {
        self.validate_connection(&connection)?;
        self.repository
            .save_connection(&connection)
            .await
            .map_err(ApplicationError::from)
    }

    pub async fn delete_connection(&self, connection_id: &str) -> Result<(), ApplicationError> {
        let id =
            LlmConnectionId::parse(connection_id).map_err(ApplicationError::ValidationError)?;
        self.repository
            .delete_connection(&id)
            .await
            .map_err(ApplicationError::from)
    }

    pub async fn apply_connection_to_payload(
        &self,
        connection_ref: &str,
        model_id: &str,
        payload: &mut Map<String, Value>,
    ) -> Result<ResolvedLlmModelBinding, ApplicationError> {
        let connection_ref = connection_ref.trim();
        if connection_ref.is_empty() {
            return Err(ApplicationError::ValidationError(
                "agent.model_connection_ref_required: model.connectionRef cannot be empty"
                    .to_string(),
            ));
        }
        let model_id = model_id.trim();
        if model_id.is_empty() {
            return Err(ApplicationError::ValidationError(
                "agent.model_id_required: model.modelId cannot be empty".to_string(),
            ));
        }

        let id =
            LlmConnectionId::parse(connection_ref).map_err(ApplicationError::ValidationError)?;
        let connection = self.repository.load_connection(&id).await?.ok_or_else(|| {
            ApplicationError::NotFound(format!(
                "llm_connection.not_found: LLM connection `{}` does not exist",
                id.as_str()
            ))
        })?;
        let source = self.validate_connection(&connection)?;
        let secret_ref = secret_ref(&connection);
        let custom_api_format = normalized_custom_api_format(&connection);

        for key in CONNECTION_PAYLOAD_KEYS {
            payload.remove(*key);
        }

        payload.insert(
            "chat_completion_source".to_string(),
            Value::String(source.key().to_string()),
        );
        payload.insert("model".to_string(), Value::String(model_id.to_string()));

        if let Some(format) = custom_api_format.as_deref() {
            payload.insert(
                "custom_api_format".to_string(),
                Value::String(format.to_string()),
            );
        }

        if let Some(base_url) = trimmed_option(connection.endpoint.base_url.as_deref()) {
            payload.insert(
                "custom_url".to_string(),
                Value::String(base_url.to_string()),
            );
        }

        for (key, value) in &connection.endpoint.source_specific {
            payload.insert(key.clone(), value.clone());
        }

        payload.insert(
            "secret_id".to_string(),
            Value::String(secret_ref.id.trim().to_string()),
        );

        if let Some(reverse_proxy) = connection.routing.reverse_proxy.as_ref() {
            payload.insert(
                "reverse_proxy".to_string(),
                Value::String(reverse_proxy.url.trim().to_string()),
            );
        }

        if let Some(value) =
            trimmed_option(connection.adapter_hints.prompt_post_processing.as_deref())
        {
            payload.insert(
                "custom_prompt_post_processing".to_string(),
                Value::String(value.to_string()),
            );
        }
        if let Some(value) =
            trimmed_option(connection.adapter_hints.custom_include_headers.as_deref())
        {
            payload.insert(
                "custom_include_headers".to_string(),
                Value::String(value.to_string()),
            );
        }
        if let Some(value) = trimmed_option(connection.adapter_hints.custom_include_body.as_deref())
        {
            payload.insert(
                "custom_include_body".to_string(),
                Value::String(value.to_string()),
            );
        }
        if let Some(value) = trimmed_option(connection.adapter_hints.custom_exclude_body.as_deref())
        {
            payload.insert(
                "custom_exclude_body".to_string(),
                Value::String(value.to_string()),
            );
        }

        Ok(ResolvedLlmModelBinding {
            mode: "connectionRef".to_string(),
            connection_ref: id.as_str().to_string(),
            connection_display_name: connection.display_name.clone(),
            chat_completion_source: source.key().to_string(),
            custom_api_format,
            model_id: model_id.to_string(),
            secret_ref: ResolvedLlmSecretRef {
                key: secret_ref.key.trim().to_string(),
                id: secret_ref.id.trim().to_string(),
                label_snapshot: trimmed_option(secret_ref.label_snapshot.as_deref())
                    .map(str::to_string),
            },
        })
    }

    pub fn validate_connection(
        &self,
        connection: &LlmConnectionDefinition,
    ) -> Result<ChatCompletionSource, ApplicationError> {
        validate_connection(connection)
    }
}

fn validate_connection(
    connection: &LlmConnectionDefinition,
) -> Result<ChatCompletionSource, ApplicationError> {
    if connection.schema_version != LLM_CONNECTION_SCHEMA_VERSION {
        return Err(ApplicationError::ValidationError(format!(
            "llm_connection.schema_unsupported: schemaVersion {} is unsupported",
            connection.schema_version
        )));
    }
    if connection.kind != LLM_CONNECTION_KIND {
        return Err(ApplicationError::ValidationError(format!(
            "llm_connection.kind_invalid: kind must be {LLM_CONNECTION_KIND}"
        )));
    }
    if connection.display_name.trim().is_empty() {
        return Err(ApplicationError::ValidationError(
            "llm_connection.display_name_required: displayName cannot be empty".to_string(),
        ));
    }

    let source = ChatCompletionSource::parse(&connection.provider.chat_completion_source)
        .ok_or_else(|| {
            ApplicationError::ValidationError(format!(
                "llm_connection.source_unsupported: unsupported chatCompletionSource `{}`",
                connection.provider.chat_completion_source
            ))
        })?;

    validate_custom_api_format(connection, source)?;
    validate_endpoint(connection, source)?;
    validate_source_specific(connection, source)?;
    validate_auth(connection, source)?;
    validate_routing(connection, source)?;

    Ok(source)
}

fn validate_custom_api_format(
    connection: &LlmConnectionDefinition,
    source: ChatCompletionSource,
) -> Result<(), ApplicationError> {
    let Some(format) = normalized_custom_api_format(connection) else {
        return Ok(());
    };

    if source != ChatCompletionSource::Custom {
        return Err(ApplicationError::ValidationError(
            "llm_connection.custom_api_format_non_custom: customApiFormat is only valid for chatCompletionSource=custom"
                .to_string(),
        ));
    }
    if !ALLOWED_CUSTOM_API_FORMATS.contains(&format.as_str()) {
        return Err(ApplicationError::ValidationError(format!(
            "llm_connection.custom_api_format_unsupported: unsupported customApiFormat `{format}`"
        )));
    }
    Ok(())
}

fn validate_endpoint(
    connection: &LlmConnectionDefinition,
    source: ChatCompletionSource,
) -> Result<(), ApplicationError> {
    if let Some(base_url) = connection.endpoint.base_url.as_deref() {
        if base_url.trim().is_empty() {
            return Err(ApplicationError::ValidationError(
                "llm_connection.base_url_empty: endpoint.baseUrl cannot be empty".to_string(),
            ));
        }
        if source != ChatCompletionSource::Custom {
            return Err(ApplicationError::ValidationError(
                "llm_connection.base_url_non_custom: endpoint.baseUrl is only valid for chatCompletionSource=custom"
                    .to_string(),
            ));
        }
    } else if source == ChatCompletionSource::Custom {
        return Err(ApplicationError::ValidationError(
            "llm_connection.custom_base_url_required: custom connections require endpoint.baseUrl"
                .to_string(),
        ));
    }

    Ok(())
}

fn validate_source_specific(
    connection: &LlmConnectionDefinition,
    source: ChatCompletionSource,
) -> Result<(), ApplicationError> {
    let allowed = SOURCE_SPECIFIC_KEYS
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    for (key, value) in &connection.endpoint.source_specific {
        if !allowed.contains(key.as_str()) {
            return Err(ApplicationError::ValidationError(format!(
                "llm_connection.source_specific_unknown: unsupported sourceSpecific key `{key}`"
            )));
        }
        if !source_specific_key_matches_source(key, source) {
            return Err(ApplicationError::ValidationError(format!(
                "llm_connection.source_specific_source_mismatch: sourceSpecific.{key} is not valid for source `{}`",
                source.key()
            )));
        }
        match key.as_str() {
            "nanogpt_payg_override" => {
                if !value.is_boolean() {
                    return Err(ApplicationError::ValidationError(format!(
                        "llm_connection.source_specific_type_invalid: sourceSpecific.{key} must be boolean"
                    )));
                }
            }
            _ => {
                if !value.as_str().is_some_and(|text| !text.trim().is_empty()) {
                    return Err(ApplicationError::ValidationError(format!(
                        "llm_connection.source_specific_type_invalid: sourceSpecific.{key} must be a non-empty string"
                    )));
                }
            }
        }
    }

    if source == ChatCompletionSource::WorkersAi
        && !connection
            .endpoint
            .source_specific
            .contains_key("workers_ai_account_id")
    {
        return Err(ApplicationError::ValidationError(
            "llm_connection.workers_ai_account_required: sourceSpecific.workers_ai_account_id is required for workers_ai"
                .to_string(),
        ));
    }

    Ok(())
}

fn source_specific_key_matches_source(key: &str, source: ChatCompletionSource) -> bool {
    match key {
        "vertexai_auth_mode" | "vertexai_region" | "vertexai_express_project_id" => {
            source == ChatCompletionSource::VertexAi
        }
        "zai_endpoint" => source == ChatCompletionSource::Zai,
        "siliconflow_endpoint" => source == ChatCompletionSource::SiliconFlow,
        "minimax_endpoint" => source == ChatCompletionSource::MiniMax,
        "workers_ai_account_id" => source == ChatCompletionSource::WorkersAi,
        "nanogpt_provider" | "nanogpt_payg_override" => source == ChatCompletionSource::NanoGpt,
        _ => false,
    }
}

fn validate_auth(
    connection: &LlmConnectionDefinition,
    source: ChatCompletionSource,
) -> Result<(), ApplicationError> {
    let secret_ref = secret_ref(connection);
    if secret_ref.key.trim().is_empty() {
        return Err(ApplicationError::ValidationError(
            "llm_connection.secret_key_required: auth.secretRef.key cannot be empty".to_string(),
        ));
    }
    if secret_ref.id.trim().is_empty() {
        return Err(ApplicationError::ValidationError(
            "llm_connection.secret_id_required: auth.secretRef.id cannot be empty".to_string(),
        ));
    }

    let expected = expected_secret_key(connection, source)?;
    if secret_ref.key.trim() != expected {
        return Err(ApplicationError::ValidationError(format!(
            "llm_connection.secret_key_mismatch: expected auth.secretRef.key `{expected}` for source `{}`",
            source.key()
        )));
    }

    Ok(())
}

fn validate_routing(
    connection: &LlmConnectionDefinition,
    source: ChatCompletionSource,
) -> Result<(), ApplicationError> {
    let Some(reverse_proxy) = connection.routing.reverse_proxy.as_ref() else {
        return Ok(());
    };
    if reverse_proxy.url.trim().is_empty() {
        return Err(ApplicationError::ValidationError(
            "llm_connection.reverse_proxy_empty: routing.reverseProxy.url cannot be empty"
                .to_string(),
        ));
    }
    if !supports_reverse_proxy(source) {
        return Err(ApplicationError::ValidationError(format!(
            "llm_connection.reverse_proxy_unsupported: source `{}` does not support reverse proxy routing",
            source.key()
        )));
    }
    Ok(())
}

fn normalized_custom_api_format(connection: &LlmConnectionDefinition) -> Option<String> {
    connection
        .provider
        .custom_api_format
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase)
}

fn secret_ref(connection: &LlmConnectionDefinition) -> &LlmConnectionSecretRef {
    &connection.auth.secret_ref
}

fn expected_secret_key(
    connection: &LlmConnectionDefinition,
    source: ChatCompletionSource,
) -> Result<&'static str, ApplicationError> {
    if source == ChatCompletionSource::VertexAi {
        let mode = connection
            .endpoint
            .source_specific
            .get("vertexai_auth_mode")
            .and_then(Value::as_str)
            .unwrap_or("express")
            .trim()
            .to_ascii_lowercase();
        return match mode.as_str() {
            "" | "express" => Ok(SecretKeys::VERTEXAI),
            "full" => Ok(SecretKeys::VERTEXAI_SERVICE_ACCOUNT),
            other => Err(ApplicationError::ValidationError(format!(
                "llm_connection.vertexai_auth_mode_unsupported: unsupported vertexai_auth_mode `{other}`"
            ))),
        };
    }

    match source {
        ChatCompletionSource::OpenAi => Ok(SecretKeys::OPENAI),
        ChatCompletionSource::OpenRouter => Ok(SecretKeys::OPENROUTER),
        ChatCompletionSource::Custom => Ok(SecretKeys::CUSTOM),
        ChatCompletionSource::Claude => Ok(SecretKeys::CLAUDE),
        ChatCompletionSource::Makersuite => Ok(SecretKeys::MAKERSUITE),
        ChatCompletionSource::DeepSeek => Ok(SecretKeys::DEEPSEEK),
        ChatCompletionSource::Cohere => Ok(SecretKeys::COHERE),
        ChatCompletionSource::Groq => Ok(SecretKeys::GROQ),
        ChatCompletionSource::Moonshot => Ok(SecretKeys::MOONSHOT),
        ChatCompletionSource::NanoGpt => Ok(SecretKeys::NANOGPT),
        ChatCompletionSource::Chutes => Ok(SecretKeys::CHUTES),
        ChatCompletionSource::SiliconFlow => Ok(SecretKeys::SILICONFLOW),
        ChatCompletionSource::WorkersAi => Ok(SecretKeys::WORKERS_AI),
        ChatCompletionSource::Zai => Ok(SecretKeys::ZAI),
        ChatCompletionSource::MiniMax => Ok(SecretKeys::MINIMAX),
        ChatCompletionSource::VertexAi => unreachable!("Vertex AI handled above"),
    }
}

fn supports_reverse_proxy(source: ChatCompletionSource) -> bool {
    matches!(
        source,
        ChatCompletionSource::OpenAi
            | ChatCompletionSource::Claude
            | ChatCompletionSource::Makersuite
            | ChatCompletionSource::VertexAi
            | ChatCompletionSource::DeepSeek
            | ChatCompletionSource::Moonshot
            | ChatCompletionSource::Zai
    )
}

fn trimmed_option(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use serde_json::{Map, json};

    use super::{LlmConnectionService, validate_connection};
    use crate::domain::models::llm_connection::{
        LLM_CONNECTION_KIND, LLM_CONNECTION_SCHEMA_VERSION, LlmConnectionAdapterHints,
        LlmConnectionAuth, LlmConnectionCapabilities, LlmConnectionDefinition,
        LlmConnectionEndpoint, LlmConnectionId, LlmConnectionProvider, LlmConnectionRouting,
        LlmConnectionSecretRef,
    };

    fn openrouter_connection() -> LlmConnectionDefinition {
        LlmConnectionDefinition {
            schema_version: LLM_CONNECTION_SCHEMA_VERSION,
            kind: LLM_CONNECTION_KIND.to_string(),
            id: LlmConnectionId::parse("openrouter-main").unwrap(),
            display_name: "OpenRouter Main".to_string(),
            description: None,
            provider: LlmConnectionProvider {
                chat_completion_source: "openrouter".to_string(),
                custom_api_format: None,
            },
            endpoint: LlmConnectionEndpoint::default(),
            auth: LlmConnectionAuth {
                secret_ref: LlmConnectionSecretRef {
                    key: "api_key_openrouter".to_string(),
                    id: "secret-1".to_string(),
                    label_snapshot: Some("Main".to_string()),
                },
            },
            routing: LlmConnectionRouting::default(),
            adapter_hints: LlmConnectionAdapterHints::default(),
            capabilities: LlmConnectionCapabilities::default(),
        }
    }

    #[test]
    fn validate_rejects_secret_namespace_mismatch() {
        let mut connection = openrouter_connection();
        connection.auth.secret_ref.key = "api_key_openai".to_string();

        let error = validate_connection(&connection).unwrap_err();
        assert!(error.to_string().contains("secret_key_mismatch"));
    }

    #[test]
    fn validate_rejects_source_specific_provider_mismatch() {
        let mut connection = openrouter_connection();
        connection.endpoint.source_specific.insert(
            "workers_ai_account_id".to_string(),
            json!("cloudflare-account"),
        );

        let error = validate_connection(&connection).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("source_specific_source_mismatch")
        );
    }

    #[tokio::test]
    async fn apply_connection_overlays_and_clears_stale_payload_fields() {
        struct Repo {
            connection: LlmConnectionDefinition,
        }

        #[async_trait::async_trait]
        impl crate::domain::repositories::llm_connection_repository::LlmConnectionRepository for Repo {
            async fn list_connections(
                &self,
            ) -> Result<
                Vec<crate::domain::models::llm_connection::LlmConnectionSummary>,
                crate::domain::errors::DomainError,
            > {
                Ok(vec![self.connection.summary()])
            }

            async fn load_connection(
                &self,
                _id: &crate::domain::models::llm_connection::LlmConnectionId,
            ) -> Result<Option<LlmConnectionDefinition>, crate::domain::errors::DomainError>
            {
                Ok(Some(self.connection.clone()))
            }

            async fn save_connection(
                &self,
                _connection: &LlmConnectionDefinition,
            ) -> Result<(), crate::domain::errors::DomainError> {
                Ok(())
            }

            async fn delete_connection(
                &self,
                _id: &crate::domain::models::llm_connection::LlmConnectionId,
            ) -> Result<(), crate::domain::errors::DomainError> {
                Ok(())
            }
        }

        let service = LlmConnectionService::new(std::sync::Arc::new(Repo {
            connection: openrouter_connection(),
        }));
        let mut payload = json!({
            "chat_completion_source": "custom",
            "custom_api_format": "gemini_interactions",
            "custom_url": "https://old.example/v1",
            "model": "old-model",
            "messages": []
        })
        .as_object()
        .cloned()
        .unwrap_or_else(Map::new);

        let resolved = service
            .apply_connection_to_payload("openrouter-main", "anthropic/claude-sonnet", &mut payload)
            .await
            .expect("connection overlay");

        assert_eq!(resolved.chat_completion_source, "openrouter");
        assert_eq!(
            payload
                .get("chat_completion_source")
                .and_then(|v| v.as_str()),
            Some("openrouter")
        );
        assert_eq!(
            payload.get("model").and_then(|v| v.as_str()),
            Some("anthropic/claude-sonnet")
        );
        assert!(payload.get("custom_api_format").is_none());
        assert!(payload.get("custom_url").is_none());
        assert_eq!(
            payload.get("secret_id").and_then(|v| v.as_str()),
            Some("secret-1")
        );
    }
}
