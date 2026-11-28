use std::sync::Arc;

use serde_json::Value;
use url::Url;

use crate::errors::ApplicationError;
use tt_domain::models::secret::SecretKeys;
use tt_ports::repositories::secret_repository::SecretRepository;
use tt_ports::repositories::translate_repository::{
    DeeplApiEndpoint, TranslateProvider, TranslateRepository, TranslateRequest,
};

const LINGVA_DEFAULT_BASE_URL: &str = "https://lingva.ml/api/v1/";
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

    pub async fn resolve_user_endpoint(
        &self,
        provider: &str,
    ) -> Result<Option<Url>, ApplicationError> {
        self.configured_url(parse_provider(provider)?).await
    }

    pub async fn translate(&self, provider: &str, body: Value) -> Result<String, ApplicationError> {
        let provider = parse_provider(provider)?;

        let request = match provider {
            TranslateProvider::Google => {
                let text = require_string(&body, "text")?;
                let lang = normalize_google_lang(require_string(&body, "lang")?);
                TranslateRequest::Google { text, lang }
            }
            TranslateProvider::Libre => {
                let text = require_string(&body, "text")?;
                let lang = normalize_libre_lang(require_string(&body, "lang")?);
                let url = self.configured_url(provider).await?.ok_or_else(|| {
                    ApplicationError::ValidationError(
                        "LibreTranslate URL is not configured.".to_string(),
                    )
                })?;
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
                let configured = self.configured_url(provider).await?;
                TranslateRequest::Lingva {
                    user_configured_endpoint: configured.is_some(),
                    base_url: configured.unwrap_or_else(|| builtin_url(LINGVA_DEFAULT_BASE_URL)),
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
                let configured = self.configured_url(provider).await?;
                TranslateRequest::Deeplx {
                    user_configured_endpoint: configured.is_some(),
                    url: configured.unwrap_or_else(|| builtin_url(DEEPLX_DEFAULT_URL)),
                    text,
                    lang,
                }
            }
            TranslateProvider::OneRing => {
                let text = require_string(&body, "text")?;
                let from_lang = require_string(&body, "from_lang")?;
                let to_lang = require_string(&body, "to_lang")?;
                let configured = self.configured_url(provider).await?;
                TranslateRequest::OneRing {
                    user_configured_endpoint: configured.is_some(),
                    url: configured.unwrap_or_else(|| builtin_url(ONERING_DEFAULT_URL)),
                    text,
                    from_lang,
                    to_lang,
                }
            }
        };

        Ok(self.translate_repository.translate(request).await?)
    }

    async fn configured_url(
        &self,
        provider: TranslateProvider,
    ) -> Result<Option<Url>, ApplicationError> {
        let (key, label) = match provider {
            TranslateProvider::Libre => (SecretKeys::LIBRE_URL, "LibreTranslate"),
            TranslateProvider::Lingva => (SecretKeys::LINGVA_URL, "Lingva"),
            TranslateProvider::Deeplx => (SecretKeys::DEEPLX_URL, "DeepLX"),
            TranslateProvider::OneRing => (SecretKeys::ONERING_URL, "OneRing"),
            TranslateProvider::Google | TranslateProvider::Deepl => return Ok(None),
        };
        let Some(raw) = self
            .read_optional_secret(key)
            .await?
            .filter(|raw| !raw.trim().is_empty())
        else {
            return Ok(None);
        };
        let url = if provider == TranslateProvider::Lingva {
            parse_url_with_required_trailing_slash(&raw, label)?
        } else {
            parse_url_with_optional_trailing_slash(&raw, label)?
        };
        Ok(Some(url))
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

fn parse_provider(raw: &str) -> Result<TranslateProvider, ApplicationError> {
    TranslateProvider::parse(raw)
        .ok_or_else(|| ApplicationError::NotFound(format!("Unsupported translate provider: {raw}")))
}

fn builtin_url(url: &str) -> Url {
    Url::parse(url).expect("built-in translation URLs are valid")
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
    fn deepl_endpoint_rejects_invalid_values() {
        let body = json!({ "text": "Hello", "lang": "en", "endpoint": "bogus" });
        let result = parse_deepl_endpoint(&body);
        assert!(matches!(
            result,
            Err(ApplicationError::ValidationError(message)) if message.contains("Invalid DeepL endpoint:")
        ));
    }
}
