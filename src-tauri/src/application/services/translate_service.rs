use std::sync::Arc;

use serde_json::Value;
use url::Url;

use crate::application::errors::ApplicationError;
use crate::domain::models::secret::SecretKeys;
use crate::domain::repositories::secret_repository::SecretRepository;
use crate::domain::repositories::translate_repository::{
    DeeplApiEndpoint, TranslateProvider, TranslateRepository, TranslateRequest,
};

const LINGVA_DEFAULT_BASE_URL: &str = "https://lingva.ml/api/v1";
const ONERING_DEFAULT_URL: &str = "http://127.0.0.1:4990/translate";
const DEEPLX_DEFAULT_URL: &str = "http://127.0.0.1:1188/translate";

pub struct TranslateService {
    translate_repository: Arc<dyn TranslateRepository>,
    secret_repository: Arc<dyn SecretRepository>,
}

impl TranslateService {
    pub fn new(
        translate_repository: Arc<dyn TranslateRepository>,
        secret_repository: Arc<dyn SecretRepository>,
    ) -> Self {
        Self {
            translate_repository,
            secret_repository,
        }
    }

    pub async fn translate(&self, provider: &str, body: Value) -> Result<String, ApplicationError> {
        let provider = TranslateProvider::parse(provider).ok_or_else(|| {
            ApplicationError::NotFound(format!("Unsupported translate provider: {provider}"))
        })?;

        let request = match provider {
            TranslateProvider::Google => {
                let text = require_string(&body, "text")?;
                let lang = normalize_google_lang(require_string(&body, "lang")?);
                TranslateRequest::Google { text, lang }
            }
            TranslateProvider::Libre => {
                let text = require_string(&body, "text")?;
                let lang = normalize_libre_lang(require_string(&body, "lang")?);
                let url = self
                    .read_required_secret(
                        SecretKeys::LIBRE_URL,
                        "LibreTranslate URL is not configured.",
                    )
                    .await?;
                let url = parse_url_with_optional_trailing_slash(&url, "LibreTranslate")?;
                let api_key = self.read_optional_secret(SecretKeys::LIBRE).await?;
                TranslateRequest::Libre {
                    url,
                    api_key,
                    text,
                    lang,
                }
            }
            TranslateProvider::Lingva => {
                let text = require_string(&body, "text")?;
                let lang = normalize_lingva_lang(require_string(&body, "lang")?);
                let raw = self
                    .read_optional_secret(SecretKeys::LINGVA_URL)
                    .await?
                    .unwrap_or_else(|| LINGVA_DEFAULT_BASE_URL.to_string());
                let base_url = parse_url_with_required_trailing_slash(&raw, "Lingva")?;
                TranslateRequest::Lingva {
                    base_url,
                    text,
                    lang,
                }
            }
            TranslateProvider::Deepl => {
                let text = require_string(&body, "text")?;
                let lang = normalize_deepl_lang(require_string(&body, "lang")?);
                let endpoint = parse_deepl_endpoint(&body)?;
                let key = self
                    .read_required_secret(SecretKeys::DEEPL, "No DeepL API key")
                    .await?;
                TranslateRequest::Deepl {
                    endpoint,
                    auth_key: key,
                    text,
                    lang,
                }
            }
            TranslateProvider::Deeplx => {
                let text = require_string(&body, "text")?;
                let lang = normalize_deeplx_lang(require_string(&body, "lang")?);
                let raw = self
                    .read_optional_secret(SecretKeys::DEEPLX_URL)
                    .await?
                    .unwrap_or_else(|| DEEPLX_DEFAULT_URL.to_string());
                let url = parse_url_with_optional_trailing_slash(&raw, "DeepLX")?;
                TranslateRequest::Deeplx { url, text, lang }
            }
            TranslateProvider::OneRing => {
                let text = require_string(&body, "text")?;
                let from_lang = require_string(&body, "from_lang")?;
                let to_lang = require_string(&body, "to_lang")?;
                let raw = self
                    .read_optional_secret(SecretKeys::ONERING_URL)
                    .await?
                    .unwrap_or_else(|| ONERING_DEFAULT_URL.to_string());
                let url = parse_url_with_optional_trailing_slash(&raw, "OneRing")?;
                TranslateRequest::OneRing {
                    url,
                    text,
                    from_lang,
                    to_lang,
                }
            }
        };

        Ok(self.translate_repository.translate(request).await?)
    }

    async fn read_optional_secret(&self, key: &str) -> Result<Option<String>, ApplicationError> {
        Ok(self.secret_repository.read_secret(key, None).await?)
    }

    async fn read_required_secret(
        &self,
        key: &str,
        message: &str,
    ) -> Result<String, ApplicationError> {
        let secret = self
            .secret_repository
            .read_secret(key, None)
            .await?
            .unwrap_or_default();
        if secret.trim().is_empty() {
            return Err(ApplicationError::ValidationError(message.to_string()));
        }

        Ok(secret)
    }
}

fn as_object(value: &Value) -> Result<&serde_json::Map<String, Value>, ApplicationError> {
    value.as_object().ok_or_else(|| {
        ApplicationError::ValidationError("Invalid request body: expected JSON object".to_string())
    })
}

fn require_string(body: &Value, key: &str) -> Result<String, ApplicationError> {
    let object = as_object(body)?;
    let Some(value) = object.get(key) else {
        return Err(ApplicationError::ValidationError(format!(
            "Missing required field: {key}"
        )));
    };
    let Some(text) = value.as_str() else {
        return Err(ApplicationError::ValidationError(format!(
            "Invalid field type: {key} must be a string"
        )));
    };
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(ApplicationError::ValidationError(format!(
            "Invalid field value: {key} cannot be empty"
        )));
    }
    Ok(trimmed.to_string())
}

fn optional_string(body: &Value, key: &str) -> Option<String> {
    body.as_object()
        .and_then(|obj| obj.get(key))
        .and_then(|value| value.as_str())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn parse_url_with_optional_trailing_slash(raw: &str, label: &str) -> Result<Url, ApplicationError> {
    let value = raw.trim();
    Url::parse(value)
        .map_err(|error| ApplicationError::ValidationError(format!("Invalid {label} URL: {error}")))
}

fn parse_url_with_required_trailing_slash(raw: &str, label: &str) -> Result<Url, ApplicationError> {
    let value = raw.trim();
    let normalized = if value.ends_with('/') {
        value.to_string()
    } else {
        format!("{value}/")
    };

    Url::parse(&normalized)
        .map_err(|error| ApplicationError::ValidationError(format!("Invalid {label} URL: {error}")))
}

fn normalize_google_lang(lang: String) -> String {
    if lang == "pt-BR" {
        return "pt".to_string();
    }

    lang
}

fn normalize_libre_lang(lang: String) -> String {
    match lang.as_str() {
        "zh-CN" => "zh".to_string(),
        "zh-TW" => "zt".to_string(),
        "pt-BR" | "pt-PT" => "pt".to_string(),
        _ => lang,
    }
}

fn normalize_lingva_lang(lang: String) -> String {
    match lang.as_str() {
        "zh-CN" | "zh-TW" => "zh".to_string(),
        "pt-BR" | "pt-PT" => "pt".to_string(),
        _ => lang,
    }
}

fn normalize_deepl_lang(lang: String) -> String {
    match lang.as_str() {
        "zh-CN" | "zh-TW" => "ZH".to_string(),
        _ => lang,
    }
}

fn normalize_deeplx_lang(lang: String) -> String {
    match lang.as_str() {
        "zh-CN" | "zh-TW" => "ZH".to_string(),
        _ => lang,
    }
}

fn parse_deepl_endpoint(body: &Value) -> Result<DeeplApiEndpoint, ApplicationError> {
    let Some(raw) = optional_string(body, "endpoint") else {
        return Ok(DeeplApiEndpoint::Free);
    };

    DeeplApiEndpoint::parse(&raw)
        .ok_or_else(|| ApplicationError::ValidationError(format!("Invalid DeepL endpoint: {raw}")))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn normalize_libre_lang_maps_zh_and_pt() {
        assert_eq!(normalize_libre_lang("zh-CN".to_string()), "zh");
        assert_eq!(normalize_libre_lang("zh-TW".to_string()), "zt");
        assert_eq!(normalize_libre_lang("pt-BR".to_string()), "pt");
        assert_eq!(normalize_libre_lang("pt-PT".to_string()), "pt");
    }

    #[test]
    fn normalize_deepl_lang_maps_zh_to_uppercase() {
        assert_eq!(normalize_deepl_lang("zh-CN".to_string()), "ZH");
        assert_eq!(normalize_deepl_lang("zh-TW".to_string()), "ZH");
        assert_eq!(normalize_deepl_lang("en".to_string()), "en");
    }

    #[test]
    fn deepl_endpoint_defaults_to_free_when_missing() {
        let body = json!({ "text": "Hello", "lang": "en" });
        assert_eq!(parse_deepl_endpoint(&body).unwrap(), DeeplApiEndpoint::Free);
    }

    #[test]
    fn deepl_endpoint_rejects_invalid_values() {
        let body = json!({ "text": "Hello", "lang": "en", "endpoint": "bogus" });
        let result = parse_deepl_endpoint(&body);
        assert!(matches!(
            result,
            Err(ApplicationError::ValidationError(message)) if message.contains("Invalid DeepL endpoint:")
        ));
    }
}
