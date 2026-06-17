use std::collections::HashMap;
use std::sync::Arc;

use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde_json::{Map, Value};

use crate::application::dto::chat_completion_dto::{
    ChatCompletionGenerateRequestDto, ChatCompletionStatusRequestDto,
};
use crate::application::errors::ApplicationError;
use crate::domain::models::secret::SecretKeys;
use crate::domain::repositories::chat_completion_repository::{
    AnthropicBetaHeaderMode, ChatCompletionApiConfig, ChatCompletionSource,
};
use crate::domain::repositories::provider_metadata_repository::SiliconFlowEndpoint;
use crate::domain::repositories::secret_repository::SecretRepository;

use super::additional_parameters::AdditionalParameters;
use super::vertexai_auth;

const OPENAI_API_BASE: &str = "https://api.openai.com/v1";
const OPENROUTER_API_BASE: &str = "https://openrouter.ai/api/v1";
const CLAUDE_API_BASE: &str = "https://api.anthropic.com/v1";
const GEMINI_API_BASE: &str = "https://generativelanguage.googleapis.com";
const VERTEXAI_GLOBAL_BASE: &str = "https://aiplatform.googleapis.com";
const DEEPSEEK_API_BASE: &str = "https://api.deepseek.com/beta";
const DEEPSEEK_STATUS_API_BASE: &str = "https://api.deepseek.com";
const COHERE_STATUS_API_BASE: &str = "https://api.cohere.ai/v1";
const COHERE_API_BASE: &str = "https://api.cohere.ai/v2";
const GROQ_API_BASE: &str = "https://api.groq.com/openai/v1";
const MOONSHOT_API_BASE: &str = "https://api.moonshot.ai/v1";
const NANOGPT_API_BASE: &str = "https://nano-gpt.com/api/v1";
const CHUTES_API_BASE: &str = "https://llm.chutes.ai/v1";
const SILICONFLOW_API_BASE: &str = "https://api.siliconflow.com/v1";
const SILICONFLOW_API_BASE_CN: &str = "https://api.siliconflow.cn/v1";
const WORKERS_AI_API_BASE: &str = "https://api.cloudflare.com/client/v4/accounts";
const ZAI_API_BASE_COMMON: &str = "https://api.z.ai/api/paas/v4";
const ZAI_API_BASE_CODING: &str = "https://api.z.ai/api/coding/paas/v4";
const MINIMAX_API_BASE: &str = "https://api.minimax.io/v1";
const MINIMAX_API_BASE_CN: &str = "https://api.minimaxi.com/v1";
const AWS_BEDROCK_DEFAULT_REGION: &str = "us-east-1";
const OPENROUTER_REFERER: &str = "https://tauritavern.github.io";
const OPENROUTER_TITLE: &str = "TauriTavern";
const OPENROUTER_CATEGORIES: &str = "roleplay,general-chat";

const ZAI_ENDPOINT_CODING: &str = "coding";
const MINIMAX_ENDPOINT_CN: &str = "cn";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ApiConfigPurpose {
    Status,
    Generate,
}

#[derive(Default)]
struct ApiConfigHints<'a> {
    zai_endpoint: &'a str,
    siliconflow_endpoint: &'a str,
    minimax_endpoint: &'a str,
    workers_ai_account_id: &'a str,
    nanogpt_provider: &'a str,
    nanogpt_payg_override: bool,
    aws_bedrock_region: &'a str,
    /// Dotted JSON path applied to non-stream Bedrock responses when the
    /// custom-invoke-template escape hatch is enabled (e.g.
    /// `output.message.content.0.text`).
    aws_bedrock_custom_response_path: Option<&'a str>,
    /// Same as [`aws_bedrock_custom_response_path`] but applied to each
    /// streaming chunk JSON (e.g. `delta.text`).
    aws_bedrock_custom_stream_path: Option<&'a str>,
    secret_id: Option<&'a str>,
}

pub(super) async fn resolve_status_api_config(
    source: ChatCompletionSource,
    dto: &ChatCompletionStatusRequestDto,
    secret_repository: &Arc<dyn SecretRepository>,
) -> Result<ChatCompletionApiConfig, ApplicationError> {
    let reverse_proxy = dto.reverse_proxy.trim();
    let proxy_password = dto.proxy_password.trim();

    let custom_url = dto.custom_url.trim();
    let additional_parameters =
        AdditionalParameters::from_status_headers(&dto.custom_include_headers)?;
    let additional_headers = additional_parameters.headers()?;

    resolve_api_config(
        source,
        reverse_proxy,
        proxy_password,
        custom_url,
        additional_headers,
        ApiConfigHints {
            siliconflow_endpoint: dto.siliconflow_endpoint.trim(),
            minimax_endpoint: dto.minimax_endpoint.trim(),
            workers_ai_account_id: dto.workers_ai_account_id.trim(),
            aws_bedrock_region: dto.aws_bedrock_region.trim(),
            secret_id: normalize_secret_id(dto.secret_id.as_deref()),
            ..Default::default()
        },
        ApiConfigPurpose::Status,
        secret_repository,
    )
    .await
}

pub(super) async fn resolve_generate_api_config(
    source: ChatCompletionSource,
    dto: &ChatCompletionGenerateRequestDto,
    additional_parameters: &AdditionalParameters,
    secret_repository: &Arc<dyn SecretRepository>,
) -> Result<ChatCompletionApiConfig, ApplicationError> {
    let reverse_proxy = dto.get_string("reverse_proxy").unwrap_or_default().trim();
    let proxy_password = dto.get_string("proxy_password").unwrap_or_default().trim();
    let custom_url_raw = get_payload_string(&dto.payload, "custom_url")?;
    let custom_url = custom_url_raw.trim();
    let zai_endpoint = get_payload_string(&dto.payload, "zai_endpoint")?;
    let siliconflow_endpoint = get_payload_string(&dto.payload, "siliconflow_endpoint")?;
    let minimax_endpoint = get_payload_string(&dto.payload, "minimax_endpoint")?;
    let workers_ai_account_id = get_payload_string(&dto.payload, "workers_ai_account_id")?;
    let nanogpt_provider = get_payload_string(&dto.payload, "nanogpt_provider")?;
    let nanogpt_payg_override = get_payload_bool(&dto.payload, "nanogpt_payg_override")?;
    let aws_bedrock_region = get_payload_string(&dto.payload, "aws_bedrock_region")?;
    let aws_bedrock_use_custom_template =
        get_payload_bool(&dto.payload, "aws_bedrock_use_custom_template")?;
    let aws_bedrock_custom_response_path = if aws_bedrock_use_custom_template {
        get_payload_string(&dto.payload, "aws_bedrock_custom_response_path")?
    } else {
        String::new()
    };
    let aws_bedrock_custom_stream_path = if aws_bedrock_use_custom_template {
        get_payload_string(&dto.payload, "aws_bedrock_custom_stream_path")?
    } else {
        String::new()
    };
    let secret_id = get_payload_optional_string(&dto.payload, "secret_id")?;
    let additional_headers = additional_parameters.headers()?;

    if source == ChatCompletionSource::VertexAi {
        return resolve_vertexai_generate_api_config(
            &dto.payload,
            reverse_proxy,
            proxy_password,
            additional_headers,
            secret_id.as_deref(),
            secret_repository,
        )
        .await;
    }

    resolve_api_config(
        source,
        reverse_proxy,
        proxy_password,
        custom_url,
        additional_headers,
        ApiConfigHints {
            zai_endpoint: &zai_endpoint,
            siliconflow_endpoint: &siliconflow_endpoint,
            minimax_endpoint: &minimax_endpoint,
            workers_ai_account_id: &workers_ai_account_id,
            nanogpt_provider: &nanogpt_provider,
            nanogpt_payg_override,
            aws_bedrock_region: &aws_bedrock_region,
            aws_bedrock_custom_response_path: aws_bedrock_custom_path_hint(
                &aws_bedrock_custom_response_path,
            ),
            aws_bedrock_custom_stream_path: aws_bedrock_custom_path_hint(
                &aws_bedrock_custom_stream_path,
            ),
            secret_id: secret_id.as_deref(),
        },
        ApiConfigPurpose::Generate,
        secret_repository,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn resolve_api_config(
    source: ChatCompletionSource,
    reverse_proxy: &str,
    proxy_password: &str,
    custom_url: &str,
    additional_headers: HashMap<String, String>,
    hints: ApiConfigHints<'_>,
    purpose: ApiConfigPurpose,
    secret_repository: &Arc<dyn SecretRepository>,
) -> Result<ChatCompletionApiConfig, ApplicationError> {
    match source {
        ChatCompletionSource::Custom => {
            let base_url = resolve_custom_base_url(custom_url, reverse_proxy)?;
            let extra_headers = source_extra_headers(source);
            let uses_reverse_proxy = custom_url.is_empty() && !reverse_proxy.is_empty();

            let api_key = if uses_reverse_proxy {
                proxy_password.to_string()
            } else {
                read_optional_secret(secret_repository, SecretKeys::CUSTOM, hints.secret_id)
                    .await?
                    .unwrap_or_default()
            };

            Ok(ChatCompletionApiConfig {
                base_url,
                api_key,
                authorization_header: None,
                extra_headers,
                additional_headers,
                anthropic_beta_header_mode: AnthropicBetaHeaderMode::None,
                aws_bedrock_custom_response_path: None,
                aws_bedrock_custom_stream_path: None,
            })
        }
        _ => {
            let base_url = if supports_reverse_proxy(source) && !reverse_proxy.is_empty() {
                reverse_proxy.to_string()
            } else {
                default_base_url(source, purpose, &hints)?
            };

            let api_key = if supports_reverse_proxy(source) && !reverse_proxy.is_empty() {
                proxy_password.to_string()
            } else {
                let secret_key = source_secret_key(source).ok_or_else(|| {
                    ApplicationError::InternalError(
                        "Secret key mapping is missing for chat completion source".to_string(),
                    )
                })?;

                read_required_secret(
                    secret_repository,
                    secret_key,
                    hints.secret_id,
                    source.display_name(),
                )
                .await?
            };

            let mut extra_headers = source_extra_headers(source);
            apply_dynamic_headers(source, &hints, &mut extra_headers);

            let (aws_bedrock_custom_response_path, aws_bedrock_custom_stream_path) =
                aws_bedrock_custom_paths(source, &hints);

            Ok(ChatCompletionApiConfig {
                base_url,
                api_key,
                authorization_header: None,
                extra_headers,
                additional_headers,
                anthropic_beta_header_mode: source_anthropic_beta_header_mode(source),
                aws_bedrock_custom_response_path,
                aws_bedrock_custom_stream_path,
            })
        }
    }
}

/// Trim and discard empty AWS Bedrock custom JSON paths so callers never see
/// an empty-string hint (treated identically to "not configured").
fn aws_bedrock_custom_path_hint(raw: &str) -> Option<&str> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// AWS Bedrock-only escape hatch: when the user opted into the custom invoke
/// template, surface the optional response/stream paths from
/// [`ApiConfigHints`] so the infrastructure layer can extract assistant text
/// from arbitrary upstream shapes. Returns `(None, None)` for every other
/// source.
fn aws_bedrock_custom_paths(
    source: ChatCompletionSource,
    hints: &ApiConfigHints<'_>,
) -> (Option<String>, Option<String>) {
    if source != ChatCompletionSource::AwsBedrock {
        return (None, None);
    }
    let response = hints
        .aws_bedrock_custom_response_path
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let stream = hints
        .aws_bedrock_custom_stream_path
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    (response, stream)
}

fn source_anthropic_beta_header_mode(source: ChatCompletionSource) -> AnthropicBetaHeaderMode {
    match source {
        ChatCompletionSource::Claude => AnthropicBetaHeaderMode::ClaudeDefaults,
        _ => AnthropicBetaHeaderMode::None,
    }
}

fn resolve_custom_base_url(
    custom_url: &str,
    reverse_proxy: &str,
) -> Result<String, ApplicationError> {
    if !custom_url.is_empty() {
        return Ok(custom_url.to_string());
    }

    if !reverse_proxy.is_empty() {
        return Ok(reverse_proxy.to_string());
    }

    Err(ApplicationError::ValidationError(
        "Custom endpoint is missing. Please configure custom_url.".to_string(),
    ))
}

fn get_payload_string(
    payload: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<String, ApplicationError> {
    match payload.get(key) {
        None => Ok(String::new()),
        Some(Value::String(value)) => Ok(value.to_string()),
        Some(_) => Err(ApplicationError::ValidationError(format!(
            "Chat completion request field must be a string: {}",
            key
        ))),
    }
}

fn get_payload_optional_string(
    payload: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<Option<String>, ApplicationError> {
    match payload.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(normalize_secret_id(Some(value)).map(str::to_string)),
        Some(_) => Err(ApplicationError::ValidationError(format!(
            "Chat completion request field must be a string: {}",
            key
        ))),
    }
}

fn get_payload_bool(
    payload: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<bool, ApplicationError> {
    match payload.get(key) {
        None => Ok(false),
        Some(value) => value.as_bool().ok_or_else(|| {
            ApplicationError::ValidationError(format!(
                "Chat completion request field must be a boolean: {}",
                key
            ))
        }),
    }
}

fn normalize_secret_id(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

async fn read_required_secret(
    secret_repository: &Arc<dyn SecretRepository>,
    secret_key: &str,
    secret_id: Option<&str>,
    source_name: &str,
) -> Result<String, ApplicationError> {
    read_selected_secret(secret_repository, secret_key, secret_id)
        .await?
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            let selector = secret_id
                .map(|id| format!(" for secret_id {id}"))
                .unwrap_or_default();
            ApplicationError::ValidationError(format!(
                "{} API key is missing{}. Please configure {}.",
                source_name, selector, secret_key
            ))
        })
}

async fn read_optional_secret(
    secret_repository: &Arc<dyn SecretRepository>,
    secret_key: &str,
    secret_id: Option<&str>,
) -> Result<Option<String>, ApplicationError> {
    Ok(
        read_selected_secret(secret_repository, secret_key, secret_id)
            .await?
            .filter(|value| !value.trim().is_empty()),
    )
}

async fn read_selected_secret(
    secret_repository: &Arc<dyn SecretRepository>,
    secret_key: &str,
    secret_id: Option<&str>,
) -> Result<Option<String>, ApplicationError> {
    let secret = secret_repository.read_secret(secret_key, secret_id).await?;
    if let (Some(id), None) = (secret_id, secret.as_ref()) {
        return Err(ApplicationError::ValidationError(format!(
            "Secret id not found for {}: {}",
            secret_key, id
        )));
    }

    Ok(secret)
}

fn default_base_url(
    source: ChatCompletionSource,
    purpose: ApiConfigPurpose,
    hints: &ApiConfigHints<'_>,
) -> Result<String, ApplicationError> {
    let base_url = match source {
        ChatCompletionSource::OpenAi => OPENAI_API_BASE.to_string(),
        ChatCompletionSource::OpenRouter => OPENROUTER_API_BASE.to_string(),
        ChatCompletionSource::Claude => CLAUDE_API_BASE.to_string(),
        ChatCompletionSource::Makersuite => GEMINI_API_BASE.to_string(),
        ChatCompletionSource::VertexAi => VERTEXAI_GLOBAL_BASE.to_string(),
        ChatCompletionSource::DeepSeek => match purpose {
            ApiConfigPurpose::Status => DEEPSEEK_STATUS_API_BASE.to_string(),
            ApiConfigPurpose::Generate => DEEPSEEK_API_BASE.to_string(),
        },
        ChatCompletionSource::Cohere => match purpose {
            ApiConfigPurpose::Status => COHERE_STATUS_API_BASE.to_string(),
            ApiConfigPurpose::Generate => COHERE_API_BASE.to_string(),
        },
        ChatCompletionSource::Groq => GROQ_API_BASE.to_string(),
        ChatCompletionSource::Moonshot => MOONSHOT_API_BASE.to_string(),
        ChatCompletionSource::NanoGpt => NANOGPT_API_BASE.to_string(),
        ChatCompletionSource::Chutes => CHUTES_API_BASE.to_string(),
        ChatCompletionSource::SiliconFlow => {
            siliconflow_base_url(hints.siliconflow_endpoint)?.to_string()
        }
        ChatCompletionSource::WorkersAi => {
            workers_ai_base_url(hints.workers_ai_account_id, purpose)?
        }
        ChatCompletionSource::Zai => {
            if is_zai_coding_endpoint(hints.zai_endpoint) {
                ZAI_API_BASE_CODING.to_string()
            } else {
                ZAI_API_BASE_COMMON.to_string()
            }
        }
        ChatCompletionSource::MiniMax => minimax_base_url(hints.minimax_endpoint)?.to_string(),
        ChatCompletionSource::AwsBedrock => aws_bedrock_base_url(hints.aws_bedrock_region),
        ChatCompletionSource::Custom => OPENAI_API_BASE.to_string(),
    };

    Ok(base_url)
}

fn source_secret_key(source: ChatCompletionSource) -> Option<&'static str> {
    match source {
        ChatCompletionSource::OpenAi => Some(SecretKeys::OPENAI),
        ChatCompletionSource::OpenRouter => Some(SecretKeys::OPENROUTER),
        ChatCompletionSource::Claude => Some(SecretKeys::CLAUDE),
        ChatCompletionSource::Makersuite => Some(SecretKeys::MAKERSUITE),
        ChatCompletionSource::VertexAi => Some(SecretKeys::VERTEXAI),
        ChatCompletionSource::DeepSeek => Some(SecretKeys::DEEPSEEK),
        ChatCompletionSource::Cohere => Some(SecretKeys::COHERE),
        ChatCompletionSource::Groq => Some(SecretKeys::GROQ),
        ChatCompletionSource::Moonshot => Some(SecretKeys::MOONSHOT),
        ChatCompletionSource::NanoGpt => Some(SecretKeys::NANOGPT),
        ChatCompletionSource::Chutes => Some(SecretKeys::CHUTES),
        ChatCompletionSource::SiliconFlow => Some(SecretKeys::SILICONFLOW),
        ChatCompletionSource::WorkersAi => Some(SecretKeys::WORKERS_AI),
        ChatCompletionSource::Zai => Some(SecretKeys::ZAI),
        ChatCompletionSource::MiniMax => Some(SecretKeys::MINIMAX),
        ChatCompletionSource::AwsBedrock => Some(SecretKeys::AWS_BEDROCK),
        ChatCompletionSource::Custom => Some(SecretKeys::CUSTOM),
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

fn siliconflow_base_url(endpoint: &str) -> Result<&'static str, ApplicationError> {
    match SiliconFlowEndpoint::parse_frontend(endpoint)
        .map_err(ApplicationError::ValidationError)?
    {
        SiliconFlowEndpoint::Global => Ok(SILICONFLOW_API_BASE),
        SiliconFlowEndpoint::China => Ok(SILICONFLOW_API_BASE_CN),
    }
}

fn workers_ai_base_url(
    account_id: &str,
    purpose: ApiConfigPurpose,
) -> Result<String, ApplicationError> {
    let account_id = account_id.trim();
    if account_id.is_empty() {
        return Err(ApplicationError::ValidationError(
            "workers_ai_account_id is required".to_string(),
        ));
    }

    let account_id = utf8_percent_encode(account_id, NON_ALPHANUMERIC).to_string();
    let suffix = match purpose {
        ApiConfigPurpose::Status => "ai",
        ApiConfigPurpose::Generate => "ai/v1",
    };
    Ok(format!("{WORKERS_AI_API_BASE}/{account_id}/{suffix}"))
}

fn minimax_base_url(endpoint: &str) -> Result<&'static str, ApplicationError> {
    match endpoint.trim().to_ascii_lowercase().as_str() {
        "" | "global" => Ok(MINIMAX_API_BASE),
        MINIMAX_ENDPOINT_CN => Ok(MINIMAX_API_BASE_CN),
        other => Err(ApplicationError::ValidationError(format!(
            "Unsupported MiniMax endpoint: {other}"
        ))),
    }
}

fn aws_bedrock_base_url(region: &str) -> String {
    let region = region.trim();
    let region = if region.is_empty() {
        AWS_BEDROCK_DEFAULT_REGION
    } else {
        region
    };
    format!("https://bedrock-runtime.{region}.amazonaws.com")
}

async fn resolve_vertexai_generate_api_config(
    payload: &Map<String, Value>,
    reverse_proxy: &str,
    proxy_password: &str,
    additional_headers: HashMap<String, String>,
    secret_id: Option<&str>,
    secret_repository: &Arc<dyn SecretRepository>,
) -> Result<ChatCompletionApiConfig, ApplicationError> {
    let extra_headers = HashMap::new();

    if !reverse_proxy.is_empty() {
        return Ok(ChatCompletionApiConfig {
            base_url: format!("{}/v1", reverse_proxy.trim_end_matches('/')),
            api_key: String::new(),
            authorization_header: Some(format!("Bearer {}", proxy_password)),
            extra_headers,
            additional_headers,
            anthropic_beta_header_mode: AnthropicBetaHeaderMode::None,
            aws_bedrock_custom_response_path: None,
            aws_bedrock_custom_stream_path: None,
        });
    }

    let mode = payload
        .get("vertexai_auth_mode")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("express")
        .to_ascii_lowercase();

    let region = payload
        .get("vertexai_region")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("us-central1");

    let project_override = payload
        .get("vertexai_express_project_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());

    match mode.as_str() {
        "express" => {
            let api_key = read_required_secret(
                secret_repository,
                SecretKeys::VERTEXAI,
                secret_id,
                "Google Vertex AI",
            )
            .await?;

            let base_url = if let Some(project_id) = project_override {
                format!("{VERTEXAI_GLOBAL_BASE}/v1/projects/{project_id}/locations/{region}",)
            } else {
                format!("{}/v1", vertexai_host(region))
            };

            Ok(ChatCompletionApiConfig {
                base_url,
                api_key,
                authorization_header: None,
                extra_headers,
                additional_headers,
                anthropic_beta_header_mode: AnthropicBetaHeaderMode::None,
                aws_bedrock_custom_response_path: None,
                aws_bedrock_custom_stream_path: None,
            })
        }
        "full" => {
            let service_account_json = read_required_secret(
                secret_repository,
                SecretKeys::VERTEXAI_SERVICE_ACCOUNT,
                secret_id,
                "Google Vertex AI",
            )
            .await?;
            let (project_id, access_token) =
                vertexai_auth::get_service_account_access_token(&service_account_json).await?;

            let base_url = format!(
                "{}/v1/projects/{project_id}/locations/{region}",
                vertexai_host(region)
            );

            Ok(ChatCompletionApiConfig {
                base_url,
                api_key: String::new(),
                authorization_header: Some(format!("Bearer {}", access_token)),
                extra_headers,
                additional_headers,
                anthropic_beta_header_mode: AnthropicBetaHeaderMode::None,
                aws_bedrock_custom_response_path: None,
                aws_bedrock_custom_stream_path: None,
            })
        }
        other => Err(ApplicationError::ValidationError(format!(
            "Unsupported Vertex AI authentication mode: {other}",
        ))),
    }
}

fn vertexai_host(region: &str) -> String {
    if region.trim().eq_ignore_ascii_case("global") {
        VERTEXAI_GLOBAL_BASE.to_string()
    } else {
        format!("https://{}-aiplatform.googleapis.com", region.trim())
    }
}

fn source_extra_headers(source: ChatCompletionSource) -> HashMap<String, String> {
    let mut headers = HashMap::new();

    if source == ChatCompletionSource::Zai {
        headers.insert("Accept-Language".to_string(), "en-US,en".to_string());
    }
    if source == ChatCompletionSource::OpenRouter {
        headers.insert("HTTP-Referer".to_string(), OPENROUTER_REFERER.to_string());
        headers.insert(
            "X-OpenRouter-Title".to_string(),
            OPENROUTER_TITLE.to_string(),
        );
        headers.insert("X-Title".to_string(), OPENROUTER_TITLE.to_string());
        headers.insert(
            "X-OpenRouter-Categories".to_string(),
            OPENROUTER_CATEGORIES.to_string(),
        );
    }

    headers
}

fn apply_dynamic_headers(
    source: ChatCompletionSource,
    hints: &ApiConfigHints<'_>,
    headers: &mut HashMap<String, String>,
) {
    if source != ChatCompletionSource::NanoGpt {
        return;
    }

    let provider = hints.nanogpt_provider.trim();
    if !provider.is_empty() {
        headers.insert("X-Provider".to_string(), provider.to_string());
    }

    if hints.nanogpt_payg_override {
        headers.insert("X-Billing-Mode".to_string(), "paygo".to_string());
    }
}

fn is_zai_coding_endpoint(value: &str) -> bool {
    value.trim().eq_ignore_ascii_case(ZAI_ENDPOINT_CODING)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use async_trait::async_trait;
    use serde_json::json;

    use crate::application::dto::chat_completion_dto::{
        ChatCompletionGenerateRequestDto, ChatCompletionStatusRequestDto,
    };
    use crate::application::errors::ApplicationError;
    use crate::domain::errors::DomainError;
    use crate::domain::models::secret::{SecretKeys, Secrets};
    use crate::domain::repositories::chat_completion_repository::{
        ChatCompletionApiConfig, ChatCompletionSource,
    };
    use crate::domain::repositories::secret_repository::SecretRepository;

    use super::super::additional_parameters::AdditionalParameters;
    use super::{
        ApiConfigHints, ApiConfigPurpose, DEEPSEEK_STATUS_API_BASE, MINIMAX_API_BASE,
        MINIMAX_API_BASE_CN, OPENROUTER_API_BASE, OPENROUTER_CATEGORIES, OPENROUTER_REFERER,
        OPENROUTER_TITLE, ZAI_API_BASE_CODING, default_base_url, resolve_generate_api_config,
        resolve_status_api_config, source_extra_headers, supports_reverse_proxy,
    };

    struct TestSecretRepository {
        secrets: HashMap<(String, Option<String>), String>,
    }

    impl TestSecretRepository {
        fn active(key: &str, value: &str) -> Self {
            Self::with_entries(&[(key, None, value)])
        }

        fn with_entries(entries: &[(&str, Option<&str>, &str)]) -> Self {
            Self {
                secrets: entries
                    .iter()
                    .map(|(key, id, value)| {
                        (
                            ((*key).to_string(), (*id).map(str::to_string)),
                            (*value).to_string(),
                        )
                    })
                    .collect(),
            }
        }
    }

    async fn resolve_generate_for_test(
        source: ChatCompletionSource,
        dto: &ChatCompletionGenerateRequestDto,
        secret_repository: &Arc<dyn SecretRepository>,
    ) -> Result<ChatCompletionApiConfig, ApplicationError> {
        let additional_parameters = AdditionalParameters::from_payload(&dto.payload)?;
        resolve_generate_api_config(source, dto, &additional_parameters, secret_repository).await
    }

    #[async_trait]
    impl SecretRepository for TestSecretRepository {
        async fn save(&self, _secrets: &Secrets) -> Result<(), DomainError> {
            unimplemented!()
        }

        async fn load(&self) -> Result<Secrets, DomainError> {
            unimplemented!()
        }

        async fn clear_cache(&self) -> Result<(), DomainError> {
            Ok(())
        }

        async fn write_secret(
            &self,
            _key: &str,
            _value: &str,
            _label: &str,
        ) -> Result<String, DomainError> {
            unimplemented!()
        }

        async fn read_secret(
            &self,
            key: &str,
            id: Option<&str>,
        ) -> Result<Option<String>, DomainError> {
            Ok(self
                .secrets
                .get(&(key.to_string(), id.map(str::to_string)))
                .cloned())
        }

        async fn delete_secret(&self, _key: &str, _id: Option<&str>) -> Result<(), DomainError> {
            unimplemented!()
        }

        async fn rotate_secret(&self, _key: &str, _id: &str) -> Result<(), DomainError> {
            unimplemented!()
        }

        async fn rename_secret(
            &self,
            _key: &str,
            _id: &str,
            _label: &str,
        ) -> Result<(), DomainError> {
            unimplemented!()
        }
    }

    #[test]
    fn deepseek_status_uses_non_beta_base() {
        let hints = ApiConfigHints::default();
        let actual = default_base_url(
            ChatCompletionSource::DeepSeek,
            ApiConfigPurpose::Status,
            &hints,
        )
        .unwrap();

        assert_eq!(actual, DEEPSEEK_STATUS_API_BASE);
    }

    #[test]
    fn zai_coding_endpoint_resolves_coding_base() {
        let hints = ApiConfigHints {
            zai_endpoint: "coding",
            ..Default::default()
        };
        let actual = default_base_url(
            ChatCompletionSource::Zai,
            ApiConfigPurpose::Generate,
            &hints,
        )
        .unwrap();

        assert_eq!(actual, ZAI_API_BASE_CODING);
    }

    #[test]
    fn aws_bedrock_uses_region_specific_bedrock_runtime_host() {
        let default_region = default_base_url(
            ChatCompletionSource::AwsBedrock,
            ApiConfigPurpose::Generate,
            &ApiConfigHints::default(),
        )
        .unwrap();
        assert_eq!(
            default_region,
            "https://bedrock-runtime.us-east-1.amazonaws.com"
        );

        let custom_region = default_base_url(
            ChatCompletionSource::AwsBedrock,
            ApiConfigPurpose::Generate,
            &ApiConfigHints {
                aws_bedrock_region: "us-west-2",
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            custom_region,
            "https://bedrock-runtime.us-west-2.amazonaws.com"
        );
    }

    #[test]
    fn minimax_endpoint_resolves_region_base() {
        let global = default_base_url(
            ChatCompletionSource::MiniMax,
            ApiConfigPurpose::Generate,
            &ApiConfigHints::default(),
        )
        .unwrap();
        assert_eq!(global, MINIMAX_API_BASE);

        let cn = default_base_url(
            ChatCompletionSource::MiniMax,
            ApiConfigPurpose::Generate,
            &ApiConfigHints {
                minimax_endpoint: "cn",
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(cn, MINIMAX_API_BASE_CN);
    }

    #[test]
    fn openrouter_uses_default_base_url() {
        let hints = ApiConfigHints::default();
        let actual = default_base_url(
            ChatCompletionSource::OpenRouter,
            ApiConfigPurpose::Generate,
            &hints,
        )
        .unwrap();
        assert_eq!(actual, OPENROUTER_API_BASE);
    }

    #[test]
    fn openrouter_uses_app_attribution_headers() {
        let headers = source_extra_headers(ChatCompletionSource::OpenRouter);
        assert_eq!(
            headers.get("HTTP-Referer").map(String::as_str),
            Some(OPENROUTER_REFERER)
        );
        assert_eq!(
            headers.get("X-OpenRouter-Title").map(String::as_str),
            Some(OPENROUTER_TITLE)
        );
        assert_eq!(
            headers.get("X-Title").map(String::as_str),
            Some(OPENROUTER_TITLE)
        );
        assert_eq!(
            headers.get("X-OpenRouter-Categories").map(String::as_str),
            Some(OPENROUTER_CATEGORIES)
        );
    }

    #[test]
    fn moonshot_and_zai_support_reverse_proxy() {
        assert!(supports_reverse_proxy(ChatCompletionSource::Moonshot));
        assert!(supports_reverse_proxy(ChatCompletionSource::Zai));
        assert!(!supports_reverse_proxy(ChatCompletionSource::MiniMax));
    }

    #[tokio::test]
    async fn custom_status_additional_headers_are_final_overrides() {
        let secret_repository: Arc<dyn SecretRepository> = Arc::new(TestSecretRepository::active(
            SecretKeys::CUSTOM,
            "saved-secret",
        ));
        let dto = ChatCompletionStatusRequestDto {
            chat_completion_source: "custom".to_string(),
            custom_url: "https://example.com/v1".to_string(),
            custom_include_headers: json!("Authorization: \"Bearer override\"\nX-Trace: abc"),
            ..Default::default()
        };

        let config =
            resolve_status_api_config(ChatCompletionSource::Custom, &dto, &secret_repository)
                .await
                .expect("status config should resolve");

        assert_eq!(config.base_url, "https://example.com/v1");
        assert_eq!(config.api_key, "saved-secret");
        assert_eq!(config.authorization_header, None);
        assert_eq!(
            config.additional_headers.get("X-Trace").map(String::as_str),
            Some("abc")
        );
        assert_eq!(
            config.additional_headers.iter().find_map(|(key, value)| key
                .eq_ignore_ascii_case("authorization")
                .then_some(value.as_str())),
            Some("Bearer override")
        );
    }

    #[tokio::test]
    async fn custom_status_accepts_object_form_additional_headers() {
        let secret_repository: Arc<dyn SecretRepository> = Arc::new(TestSecretRepository::active(
            SecretKeys::CUSTOM,
            "saved-secret",
        ));
        let dto = ChatCompletionStatusRequestDto {
            chat_completion_source: "custom".to_string(),
            custom_url: "https://example.com/v1".to_string(),
            custom_include_headers: json!({
                "Content-Type": "application/json",
                "Authorization": "Bearer override"
            }),
            ..Default::default()
        };

        let config =
            resolve_status_api_config(ChatCompletionSource::Custom, &dto, &secret_repository)
                .await
                .expect("status config should resolve");

        assert_eq!(
            config
                .additional_headers
                .get("Content-Type")
                .map(String::as_str),
            Some("application/json")
        );
        assert_eq!(
            config.additional_headers.iter().find_map(|(key, value)| key
                .eq_ignore_ascii_case("authorization")
                .then_some(value.as_str())),
            Some("Bearer override")
        );
    }

    #[tokio::test]
    async fn custom_status_scalar_additional_headers_fail_fast() {
        let secret_repository: Arc<dyn SecretRepository> = Arc::new(TestSecretRepository::active(
            SecretKeys::CUSTOM,
            "saved-secret",
        ));
        let dto = ChatCompletionStatusRequestDto {
            chat_completion_source: "custom".to_string(),
            custom_url: "https://example.com/v1".to_string(),
            custom_include_headers: json!(42),
            ..Default::default()
        };

        let error =
            resolve_status_api_config(ChatCompletionSource::Custom, &dto, &secret_repository)
                .await
                .expect_err("numeric custom headers should fail");

        assert!(error.to_string().contains("custom_include_headers"));
    }

    #[tokio::test]
    async fn custom_generate_falls_back_to_saved_secret_without_authorization_header() {
        let secret_repository: Arc<dyn SecretRepository> = Arc::new(TestSecretRepository::active(
            SecretKeys::CUSTOM,
            "saved-secret",
        ));
        let dto = ChatCompletionGenerateRequestDto {
            payload: json!({
                "chat_completion_source": "custom",
                "custom_url": "https://example.com/v1",
                "custom_include_headers": "X-Trace: abc"
            })
            .as_object()
            .cloned()
            .expect("payload should be an object"),
        };

        let additional_parameters =
            AdditionalParameters::from_payload(&dto.payload).expect("additional parameters parse");
        let config = resolve_generate_api_config(
            ChatCompletionSource::Custom,
            &dto,
            &additional_parameters,
            &secret_repository,
        )
        .await
        .expect("generate config should resolve");

        assert_eq!(config.api_key, "saved-secret");
        assert_eq!(config.authorization_header, None);
        assert_eq!(
            config.additional_headers.get("X-Trace").map(String::as_str),
            Some("abc")
        );
    }

    #[tokio::test]
    async fn generate_uses_requested_secret_id_for_provider_key() {
        let secret_repository: Arc<dyn SecretRepository> =
            Arc::new(TestSecretRepository::with_entries(&[
                (SecretKeys::OPENROUTER, None, "active-secret"),
                (
                    SecretKeys::OPENROUTER,
                    Some("profile-secret"),
                    "selected-secret",
                ),
            ]));
        let dto = ChatCompletionGenerateRequestDto {
            payload: json!({
                "chat_completion_source": "openrouter",
                "secret_id": "profile-secret",
            })
            .as_object()
            .cloned()
            .expect("payload should be an object"),
        };

        let config =
            resolve_generate_for_test(ChatCompletionSource::OpenRouter, &dto, &secret_repository)
                .await
                .expect("generate config should resolve");

        assert_eq!(config.api_key, "selected-secret");
    }

    #[tokio::test]
    async fn generate_secret_id_does_not_fallback_to_active_secret() {
        let secret_repository: Arc<dyn SecretRepository> = Arc::new(
            TestSecretRepository::with_entries(&[(SecretKeys::OPENROUTER, None, "active-secret")]),
        );
        let dto = ChatCompletionGenerateRequestDto {
            payload: json!({
                "chat_completion_source": "openrouter",
                "secret_id": "missing-secret",
            })
            .as_object()
            .cloned()
            .expect("payload should be an object"),
        };

        let error =
            resolve_generate_for_test(ChatCompletionSource::OpenRouter, &dto, &secret_repository)
                .await
                .expect_err("missing explicit secret id should fail");

        assert!(
            error
                .to_string()
                .contains("Secret id not found for api_key_openrouter: missing-secret")
        );
    }

    #[tokio::test]
    async fn blank_secret_id_uses_active_secret() {
        let secret_repository: Arc<dyn SecretRepository> = Arc::new(
            TestSecretRepository::with_entries(&[(SecretKeys::OPENROUTER, None, "active-secret")]),
        );
        let dto = ChatCompletionGenerateRequestDto {
            payload: json!({
                "chat_completion_source": "openrouter",
                "secret_id": "  ",
            })
            .as_object()
            .cloned()
            .expect("payload should be an object"),
        };

        let config =
            resolve_generate_for_test(ChatCompletionSource::OpenRouter, &dto, &secret_repository)
                .await
                .expect("blank secret id should keep active-secret semantics");

        assert_eq!(config.api_key, "active-secret");
    }

    #[tokio::test]
    async fn generate_rejects_non_string_secret_id() {
        let secret_repository: Arc<dyn SecretRepository> = Arc::new(TestSecretRepository::active(
            SecretKeys::OPENROUTER,
            "active-secret",
        ));
        let dto = ChatCompletionGenerateRequestDto {
            payload: json!({
                "chat_completion_source": "openrouter",
                "secret_id": 42,
            })
            .as_object()
            .cloned()
            .expect("payload should be an object"),
        };

        let error =
            resolve_generate_for_test(ChatCompletionSource::OpenRouter, &dto, &secret_repository)
                .await
                .expect_err("non-string secret id should fail");

        assert!(
            error
                .to_string()
                .contains("Chat completion request field must be a string: secret_id")
        );
    }

    #[tokio::test]
    async fn generate_rejects_non_string_provider_hints() {
        let secret_repository: Arc<dyn SecretRepository> =
            Arc::new(TestSecretRepository::active(SecretKeys::MINIMAX, "secret"));
        let dto = ChatCompletionGenerateRequestDto {
            payload: json!({
                "chat_completion_source": "minimax",
                "minimax_endpoint": 42,
            })
            .as_object()
            .cloned()
            .expect("payload should be an object"),
        };

        let error =
            resolve_generate_for_test(ChatCompletionSource::MiniMax, &dto, &secret_repository)
                .await
                .expect_err("non-string provider hint should fail");

        assert!(
            error
                .to_string()
                .contains("Chat completion request field must be a string: minimax_endpoint")
        );
    }

    #[tokio::test]
    async fn custom_generate_secret_id_selects_saved_secret() {
        let secret_repository: Arc<dyn SecretRepository> =
            Arc::new(TestSecretRepository::with_entries(&[
                (SecretKeys::CUSTOM, None, "active-secret"),
                (
                    SecretKeys::CUSTOM,
                    Some("profile-secret"),
                    "selected-secret",
                ),
            ]));
        let dto = ChatCompletionGenerateRequestDto {
            payload: json!({
                "chat_completion_source": "custom",
                "custom_url": "https://example.com/v1",
                "secret_id": "profile-secret",
            })
            .as_object()
            .cloned()
            .expect("payload should be an object"),
        };

        let config =
            resolve_generate_for_test(ChatCompletionSource::Custom, &dto, &secret_repository)
                .await
                .expect("custom config should resolve");

        assert_eq!(config.api_key, "selected-secret");
    }

    #[tokio::test]
    async fn custom_additional_authorization_does_not_hide_missing_secret_id() {
        let secret_repository: Arc<dyn SecretRepository> =
            Arc::new(TestSecretRepository::with_entries(&[]));
        let dto = ChatCompletionGenerateRequestDto {
            payload: json!({
                "chat_completion_source": "custom",
                "custom_url": "https://example.com/v1",
                "custom_include_headers": "Authorization: \"Bearer override\"",
                "secret_id": "missing-secret",
            })
            .as_object()
            .cloned()
            .expect("payload should be an object"),
        };

        let error =
            resolve_generate_for_test(ChatCompletionSource::Custom, &dto, &secret_repository)
                .await
                .expect_err("missing explicit secret id should fail");

        assert!(
            error
                .to_string()
                .contains("Secret id not found for api_key_custom: missing-secret")
        );
    }

    #[tokio::test]
    async fn status_uses_requested_secret_id_for_provider_key() {
        let secret_repository: Arc<dyn SecretRepository> =
            Arc::new(TestSecretRepository::with_entries(&[
                (SecretKeys::OPENROUTER, None, "active-secret"),
                (
                    SecretKeys::OPENROUTER,
                    Some("profile-secret"),
                    "selected-secret",
                ),
            ]));
        let dto = ChatCompletionStatusRequestDto {
            chat_completion_source: "openrouter".to_string(),
            secret_id: Some("profile-secret".to_string()),
            ..Default::default()
        };

        let config =
            resolve_status_api_config(ChatCompletionSource::OpenRouter, &dto, &secret_repository)
                .await
                .expect("status config should resolve");

        assert_eq!(config.api_key, "selected-secret");
    }

    #[tokio::test]
    async fn vertexai_generate_uses_secret_id_for_express_key() {
        let secret_repository: Arc<dyn SecretRepository> =
            Arc::new(TestSecretRepository::with_entries(&[
                (SecretKeys::VERTEXAI, None, "active-secret"),
                (
                    SecretKeys::VERTEXAI,
                    Some("vertex-profile"),
                    "selected-secret",
                ),
            ]));
        let dto = ChatCompletionGenerateRequestDto {
            payload: json!({
                "chat_completion_source": "vertexai",
                "vertexai_auth_mode": "express",
                "secret_id": "vertex-profile",
            })
            .as_object()
            .cloned()
            .expect("payload should be an object"),
        };

        let config =
            resolve_generate_for_test(ChatCompletionSource::VertexAi, &dto, &secret_repository)
                .await
                .expect("vertex express config should resolve");

        assert_eq!(config.api_key, "selected-secret");
    }

    #[tokio::test]
    async fn custom_status_prefers_custom_url_secret_over_reverse_proxy_secret() {
        let secret_repository: Arc<dyn SecretRepository> = Arc::new(TestSecretRepository::active(
            SecretKeys::CUSTOM,
            "saved-secret",
        ));
        let dto = ChatCompletionStatusRequestDto {
            chat_completion_source: "custom".to_string(),
            reverse_proxy: "https://proxy.example.com/v1".to_string(),
            proxy_password: "proxy-secret".to_string(),
            custom_url: "https://example.com/v1".to_string(),
            custom_include_headers: json!("X-Trace: abc"),
            ..Default::default()
        };

        let config =
            resolve_status_api_config(ChatCompletionSource::Custom, &dto, &secret_repository)
                .await
                .expect("status config should resolve");

        assert_eq!(config.base_url, "https://example.com/v1");
        assert_eq!(config.api_key, "saved-secret");
        assert_eq!(config.authorization_header, None);
        assert_eq!(
            config.additional_headers.get("X-Trace").map(String::as_str),
            Some("abc")
        );
    }

    #[tokio::test]
    async fn custom_status_uses_proxy_password_when_custom_url_missing_and_reverse_proxy_present() {
        let secret_repository: Arc<dyn SecretRepository> = Arc::new(TestSecretRepository::active(
            SecretKeys::CUSTOM,
            "saved-secret",
        ));
        let dto = ChatCompletionStatusRequestDto {
            chat_completion_source: "custom".to_string(),
            reverse_proxy: "https://proxy.example.com/v1".to_string(),
            proxy_password: "proxy-secret".to_string(),
            custom_url: "".to_string(),
            custom_include_headers: json!("X-Trace: abc"),
            ..Default::default()
        };

        let config =
            resolve_status_api_config(ChatCompletionSource::Custom, &dto, &secret_repository)
                .await
                .expect("status config should resolve");

        assert_eq!(config.base_url, "https://proxy.example.com/v1");
        assert_eq!(config.api_key, "proxy-secret");
        assert_eq!(config.authorization_header, None);
        assert_eq!(
            config.additional_headers.get("X-Trace").map(String::as_str),
            Some("abc")
        );
    }

    #[tokio::test]
    async fn native_generate_stores_additional_headers_as_final_overrides() {
        let secret_repository: Arc<dyn SecretRepository> =
            Arc::new(TestSecretRepository::active(SecretKeys::CLAUDE, "secret"));
        let dto = ChatCompletionGenerateRequestDto {
            payload: json!({
                "chat_completion_source": "claude",
                "custom_include_headers": "X-Trace: abc\nX-Debug: true"
            })
            .as_object()
            .cloned()
            .expect("payload should be object"),
        };

        let config =
            resolve_generate_for_test(ChatCompletionSource::Claude, &dto, &secret_repository)
                .await
                .expect("generate config should resolve");

        assert_eq!(
            config.additional_headers.get("X-Trace").map(String::as_str),
            Some("abc")
        );
        assert_eq!(
            config.additional_headers.get("X-Debug").map(String::as_str),
            Some("true")
        );
    }

    #[tokio::test]
    async fn native_generate_accepts_reserved_additional_headers_as_user_overrides() {
        let secret_repository: Arc<dyn SecretRepository> =
            Arc::new(TestSecretRepository::active(SecretKeys::CLAUDE, "secret"));
        let dto = ChatCompletionGenerateRequestDto {
            payload: json!({
                "chat_completion_source": "claude",
                "custom_include_headers": "Authorization: Bearer hacked"
            })
            .as_object()
            .cloned()
            .expect("payload should be object"),
        };

        let config =
            resolve_generate_for_test(ChatCompletionSource::Claude, &dto, &secret_repository)
                .await
                .expect("generate config should resolve");

        assert_eq!(
            config
                .additional_headers
                .get("Authorization")
                .map(String::as_str),
            Some("Bearer hacked")
        );
    }
}
