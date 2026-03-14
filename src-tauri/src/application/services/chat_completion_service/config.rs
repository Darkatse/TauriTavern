use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;

use crate::application::dto::chat_completion_dto::{
    ChatCompletionGenerateRequestDto, ChatCompletionStatusRequestDto,
};
use crate::application::errors::ApplicationError;
use crate::domain::models::secret::SecretKeys;
use crate::domain::repositories::chat_completion_repository::{
    ChatCompletionApiConfig, ChatCompletionSource,
};
use crate::domain::repositories::secret_repository::SecretRepository;

use super::custom_parameters;

const OPENAI_API_BASE: &str = "https://api.openai.com/v1";
const OPENROUTER_API_BASE: &str = "https://openrouter.ai/api/v1";
const CLAUDE_API_BASE: &str = "https://api.anthropic.com/v1";
const GEMINI_API_BASE: &str = "https://generativelanguage.googleapis.com";
const DEEPSEEK_API_BASE: &str = "https://api.deepseek.com/beta";
const DEEPSEEK_STATUS_API_BASE: &str = "https://api.deepseek.com";
const MOONSHOT_API_BASE: &str = "https://api.moonshot.ai/v1";
const SILICONFLOW_API_BASE: &str = "https://api.siliconflow.com/v1";
const ZAI_API_BASE_COMMON: &str = "https://api.z.ai/api/paas/v4";
const ZAI_API_BASE_CODING: &str = "https://api.z.ai/api/coding/paas/v4";
const OPENROUTER_REFERER: &str = "https://tauritavern.client";
const OPENROUTER_TITLE: &str = "TauriTavern";

const ZAI_ENDPOINT_CODING: &str = "coding";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ApiConfigPurpose {
    Status,
    Generate,
}

pub(super) async fn resolve_status_api_config(
    source: ChatCompletionSource,
    dto: &ChatCompletionStatusRequestDto,
    secret_repository: &Arc<dyn SecretRepository>,
) -> Result<ChatCompletionApiConfig, ApplicationError> {
    let reverse_proxy = dto.reverse_proxy.trim();
    let proxy_password = dto.proxy_password.trim();

    let custom_url = dto.custom_url.trim();
    let custom_headers_raw = dto.custom_include_headers.as_str();

    resolve_api_config(
        source,
        reverse_proxy,
        proxy_password,
        custom_url,
        custom_headers_raw,
        "",
        ApiConfigPurpose::Status,
        secret_repository,
    )
    .await
}

pub(super) async fn resolve_generate_api_config(
    source: ChatCompletionSource,
    dto: &ChatCompletionGenerateRequestDto,
    secret_repository: &Arc<dyn SecretRepository>,
) -> Result<ChatCompletionApiConfig, ApplicationError> {
    let reverse_proxy = dto.get_string("reverse_proxy").unwrap_or_default().trim();
    let proxy_password = dto.get_string("proxy_password").unwrap_or_default().trim();
    let custom_url_raw = get_payload_string(&dto.payload, "custom_url");
    let custom_url = custom_url_raw.trim();
    let custom_headers_raw = get_payload_string(&dto.payload, "custom_include_headers");
    let zai_endpoint = get_payload_string(&dto.payload, "zai_endpoint");

    resolve_api_config(
        source,
        reverse_proxy,
        proxy_password,
        custom_url,
        &custom_headers_raw,
        &zai_endpoint,
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
    custom_headers_raw: &str,
    zai_endpoint: &str,
    purpose: ApiConfigPurpose,
    secret_repository: &Arc<dyn SecretRepository>,
) -> Result<ChatCompletionApiConfig, ApplicationError> {
    match source {
        ChatCompletionSource::Custom => {
            let base_url = resolve_custom_base_url(custom_url, reverse_proxy)?;
            let mut extra_headers = custom_parameters::parse_string_map(custom_headers_raw)?;
            let authorization_header = take_header_value(&mut extra_headers, "Authorization");

            let api_key = if authorization_header.is_some() {
                String::new()
            } else if reverse_proxy.is_empty() {
                read_optional_secret(secret_repository, SecretKeys::CUSTOM)
                    .await?
                    .unwrap_or_default()
            } else {
                proxy_password.to_string()
            };

            Ok(ChatCompletionApiConfig {
                base_url,
                api_key,
                authorization_header,
                extra_headers,
            })
        }
        _ => {
            let base_url = if supports_reverse_proxy(source) && !reverse_proxy.is_empty() {
                reverse_proxy.to_string()
            } else {
                default_base_url(source, purpose, zai_endpoint)
            };

            let api_key = if supports_reverse_proxy(source) && !reverse_proxy.is_empty() {
                proxy_password.to_string()
            } else {
                let secret_key = source_secret_key(source).ok_or_else(|| {
                    ApplicationError::InternalError(
                        "Secret key mapping is missing for chat completion source".to_string(),
                    )
                })?;

                read_required_secret(secret_repository, secret_key, source_display_name(source))
                    .await?
            };

            Ok(ChatCompletionApiConfig {
                base_url,
                api_key,
                authorization_header: None,
                extra_headers: source_extra_headers(source),
            })
        }
    }
}

fn take_header_value(headers: &mut HashMap<String, String>, header_name: &str) -> Option<String> {
    let mut matching_keys = headers
        .keys()
        .filter(|key| key.eq_ignore_ascii_case(header_name))
        .cloned()
        .collect::<Vec<_>>();

    if matching_keys.is_empty() {
        return None;
    }

    matching_keys.sort_unstable();

    let preferred_key = matching_keys
        .iter()
        .find(|key| key.as_str() == header_name)
        .cloned()
        .unwrap_or_else(|| matching_keys[0].clone());

    let value = headers.remove(&preferred_key);

    for key in matching_keys {
        if key != preferred_key {
            headers.remove(&key);
        }
    }

    value
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

fn get_payload_string(payload: &serde_json::Map<String, Value>, key: &str) -> String {
    payload
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_default()
}

async fn read_required_secret(
    secret_repository: &Arc<dyn SecretRepository>,
    secret_key: &str,
    source_name: &str,
) -> Result<String, ApplicationError> {
    secret_repository
        .read_secret(secret_key, None)
        .await?
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            ApplicationError::ValidationError(format!(
                "{} API key is missing. Please configure {}.",
                source_name, secret_key
            ))
        })
}

async fn read_optional_secret(
    secret_repository: &Arc<dyn SecretRepository>,
    secret_key: &str,
) -> Result<Option<String>, ApplicationError> {
    Ok(secret_repository
        .read_secret(secret_key, None)
        .await?
        .filter(|value| !value.trim().is_empty()))
}

fn default_base_url(
    source: ChatCompletionSource,
    purpose: ApiConfigPurpose,
    zai_endpoint: &str,
) -> String {
    match source {
        ChatCompletionSource::OpenAi => OPENAI_API_BASE.to_string(),
        ChatCompletionSource::OpenRouter => OPENROUTER_API_BASE.to_string(),
        ChatCompletionSource::Claude => CLAUDE_API_BASE.to_string(),
        ChatCompletionSource::Makersuite => GEMINI_API_BASE.to_string(),
        ChatCompletionSource::DeepSeek => match purpose {
            ApiConfigPurpose::Status => DEEPSEEK_STATUS_API_BASE.to_string(),
            ApiConfigPurpose::Generate => DEEPSEEK_API_BASE.to_string(),
        },
        ChatCompletionSource::Moonshot => MOONSHOT_API_BASE.to_string(),
        ChatCompletionSource::SiliconFlow => SILICONFLOW_API_BASE.to_string(),
        ChatCompletionSource::Zai => {
            if is_zai_coding_endpoint(zai_endpoint) {
                ZAI_API_BASE_CODING.to_string()
            } else {
                ZAI_API_BASE_COMMON.to_string()
            }
        }
        ChatCompletionSource::Custom => OPENAI_API_BASE.to_string(),
    }
}

fn source_secret_key(source: ChatCompletionSource) -> Option<&'static str> {
    match source {
        ChatCompletionSource::OpenAi => Some(SecretKeys::OPENAI),
        ChatCompletionSource::OpenRouter => Some(SecretKeys::OPENROUTER),
        ChatCompletionSource::Claude => Some(SecretKeys::CLAUDE),
        ChatCompletionSource::Makersuite => Some(SecretKeys::MAKERSUITE),
        ChatCompletionSource::DeepSeek => Some(SecretKeys::DEEPSEEK),
        ChatCompletionSource::Moonshot => Some(SecretKeys::MOONSHOT),
        ChatCompletionSource::SiliconFlow => Some(SecretKeys::SILICONFLOW),
        ChatCompletionSource::Zai => Some(SecretKeys::ZAI),
        ChatCompletionSource::Custom => Some(SecretKeys::CUSTOM),
    }
}

fn source_display_name(source: ChatCompletionSource) -> &'static str {
    match source {
        ChatCompletionSource::OpenAi => "OpenAI",
        ChatCompletionSource::OpenRouter => "OpenRouter",
        ChatCompletionSource::Claude => "Claude",
        ChatCompletionSource::Makersuite => "Google Gemini",
        ChatCompletionSource::DeepSeek => "DeepSeek",
        ChatCompletionSource::Moonshot => "Moonshot AI",
        ChatCompletionSource::SiliconFlow => "SiliconFlow",
        ChatCompletionSource::Zai => "Z.AI (GLM)",
        ChatCompletionSource::Custom => "Custom OpenAI",
    }
}

fn supports_reverse_proxy(source: ChatCompletionSource) -> bool {
    matches!(
        source,
        ChatCompletionSource::OpenAi
            | ChatCompletionSource::Claude
            | ChatCompletionSource::Makersuite
            | ChatCompletionSource::DeepSeek
            | ChatCompletionSource::Moonshot
            | ChatCompletionSource::Zai
    )
}

fn source_extra_headers(source: ChatCompletionSource) -> HashMap<String, String> {
    let mut headers = HashMap::new();

    if source == ChatCompletionSource::Zai {
        headers.insert("Accept-Language".to_string(), "en-US,en".to_string());
    }
    if source == ChatCompletionSource::OpenRouter {
        headers.insert("HTTP-Referer".to_string(), OPENROUTER_REFERER.to_string());
        headers.insert("X-Title".to_string(), OPENROUTER_TITLE.to_string());
    }

    headers
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
    use crate::domain::errors::DomainError;
    use crate::domain::models::secret::Secrets;
    use crate::domain::repositories::chat_completion_repository::ChatCompletionSource;
    use crate::domain::repositories::secret_repository::SecretRepository;

    use super::{
        ApiConfigPurpose, DEEPSEEK_STATUS_API_BASE, OPENROUTER_API_BASE, ZAI_API_BASE_CODING,
        default_base_url, resolve_generate_api_config, resolve_status_api_config,
        source_extra_headers, supports_reverse_proxy, take_header_value,
    };

    struct TestSecretRepository {
        secrets: HashMap<String, String>,
    }

    #[async_trait]
    impl SecretRepository for TestSecretRepository {
        async fn save(&self, _secrets: &Secrets) -> Result<(), DomainError> {
            unimplemented!()
        }

        async fn load(&self) -> Result<Secrets, DomainError> {
            unimplemented!()
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
            _id: Option<&str>,
        ) -> Result<Option<String>, DomainError> {
            Ok(self.secrets.get(key).cloned())
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
        let actual = default_base_url(ChatCompletionSource::DeepSeek, ApiConfigPurpose::Status, "");

        assert_eq!(actual, DEEPSEEK_STATUS_API_BASE);
    }

    #[test]
    fn zai_coding_endpoint_resolves_coding_base() {
        let actual = default_base_url(
            ChatCompletionSource::Zai,
            ApiConfigPurpose::Generate,
            "coding",
        );

        assert_eq!(actual, ZAI_API_BASE_CODING);
    }

    #[test]
    fn openrouter_uses_default_base_url() {
        let actual = default_base_url(
            ChatCompletionSource::OpenRouter,
            ApiConfigPurpose::Generate,
            "",
        );
        assert_eq!(actual, OPENROUTER_API_BASE);
    }

    #[test]
    fn openrouter_uses_referer_headers() {
        let headers = source_extra_headers(ChatCompletionSource::OpenRouter);
        assert!(headers.contains_key("HTTP-Referer"));
        assert!(headers.contains_key("X-Title"));
    }

    #[test]
    fn moonshot_and_zai_support_reverse_proxy() {
        assert!(supports_reverse_proxy(ChatCompletionSource::Moonshot));
        assert!(supports_reverse_proxy(ChatCompletionSource::Zai));
    }

    #[test]
    fn take_header_value_removes_all_case_variants() {
        let mut headers = HashMap::from([
            ("authorization".to_string(), "Bearer lower".to_string()),
            ("Authorization".to_string(), "Bearer exact".to_string()),
            ("x-extra".to_string(), "ok".to_string()),
        ]);

        let value = take_header_value(&mut headers, "Authorization");

        assert_eq!(value.as_deref(), Some("Bearer exact"));
        assert!(
            headers
                .keys()
                .all(|key| !key.eq_ignore_ascii_case("authorization"))
        );
        assert_eq!(headers.get("x-extra").map(String::as_str), Some("ok"));
    }

    #[tokio::test]
    async fn custom_status_authorization_header_overrides_saved_secret() {
        let secret_repository: Arc<dyn SecretRepository> = Arc::new(TestSecretRepository {
            secrets: HashMap::from([("api_key_custom".to_string(), "saved-secret".to_string())]),
        });
        let dto = ChatCompletionStatusRequestDto {
            chat_completion_source: "custom".to_string(),
            custom_url: "https://example.com/v1".to_string(),
            custom_include_headers: "Authorization: \"Bearer override\"\nX-Trace: abc".to_string(),
            ..Default::default()
        };

        let config =
            resolve_status_api_config(ChatCompletionSource::Custom, &dto, &secret_repository)
                .await
                .expect("status config should resolve");

        assert_eq!(config.base_url, "https://example.com/v1");
        assert!(config.api_key.is_empty());
        assert_eq!(
            config.authorization_header.as_deref(),
            Some("Bearer override")
        );
        assert_eq!(
            config.extra_headers.get("X-Trace").map(String::as_str),
            Some("abc")
        );
        assert!(
            config
                .extra_headers
                .keys()
                .all(|key| !key.eq_ignore_ascii_case("authorization"))
        );
    }

    #[tokio::test]
    async fn custom_generate_falls_back_to_saved_secret_without_authorization_header() {
        let secret_repository: Arc<dyn SecretRepository> = Arc::new(TestSecretRepository {
            secrets: HashMap::from([("api_key_custom".to_string(), "saved-secret".to_string())]),
        });
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

        let config =
            resolve_generate_api_config(ChatCompletionSource::Custom, &dto, &secret_repository)
                .await
                .expect("generate config should resolve");

        assert_eq!(config.api_key, "saved-secret");
        assert_eq!(config.authorization_header, None);
        assert_eq!(
            config.extra_headers.get("X-Trace").map(String::as_str),
            Some("abc")
        );
    }
}
