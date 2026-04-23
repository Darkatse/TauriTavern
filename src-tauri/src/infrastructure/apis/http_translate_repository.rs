use std::sync::Arc;

use async_trait::async_trait;
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use serde_json::Value;
use url::Url;

use crate::domain::errors::DomainError;
use crate::domain::repositories::translate_repository::{TranslateRepository, TranslateRequest};
use crate::infrastructure::http_client_pool::{HttpClientPool, HttpClientProfile};

pub struct HttpTranslateRepository {
    http_clients: Arc<HttpClientPool>,
}

impl HttpTranslateRepository {
    pub fn new(http_clients: Arc<HttpClientPool>) -> Self {
        Self { http_clients }
    }

    fn http_client(&self) -> Result<reqwest::Client, DomainError> {
        self.http_clients.client(HttpClientProfile::Translation)
    }
}

#[async_trait]
impl TranslateRepository for HttpTranslateRepository {
    async fn translate(&self, request: TranslateRequest) -> Result<String, DomainError> {
        match request {
            TranslateRequest::Google { text, lang } => {
                translate_google(self.http_client()?, text, lang).await
            }
            TranslateRequest::Libre {
                url,
                api_key,
                text,
                lang,
            } => translate_libre(self.http_client()?, url, api_key, text, lang).await,
            TranslateRequest::Lingva {
                base_url,
                text,
                lang,
            } => translate_lingva(self.http_client()?, base_url, text, lang).await,
            TranslateRequest::Deepl {
                endpoint,
                auth_key,
                text,
                lang,
            } => translate_deepl(self.http_client()?, endpoint.url(), auth_key, text, lang).await,
            TranslateRequest::Deeplx { url, text, lang } => {
                translate_deeplx(self.http_client()?, url, text, lang).await
            }
            TranslateRequest::OneRing {
                url,
                text,
                from_lang,
                to_lang,
            } => translate_onering(self.http_client()?, url, text, from_lang, to_lang).await,
        }
    }
}

async fn translate_google(
    client: reqwest::Client,
    text: String,
    lang: String,
) -> Result<String, DomainError> {
    let response = client
        .get("https://translate.googleapis.com/translate_a/single")
        .query(&[
            ("client", "gtx"),
            ("sl", "auto"),
            ("tl", lang.as_str()),
            ("dt", "t"),
            ("q", text.as_str()),
        ])
        .header(ACCEPT, "application/json")
        .send()
        .await
        .map_err(|error| {
            DomainError::InternalError(format!("Google Translate request failed: {error}"))
        })?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(DomainError::InternalError(format!(
            "Google Translate error: HTTP {status} {body}"
        )));
    }

    let json: Value = response.json().await.map_err(|error| {
        DomainError::InternalError(format!(
            "Google Translate response is not valid JSON: {error}"
        ))
    })?;

    parse_google_translation(json)
}

fn parse_google_translation(json: Value) -> Result<String, DomainError> {
    let Some(root) = json.as_array() else {
        return Err(DomainError::InternalError(
            "Google Translate response is not an array".to_string(),
        ));
    };

    let Some(segments) = root.first().and_then(|value| value.as_array()) else {
        return Err(DomainError::InternalError(
            "Google Translate response missing segments".to_string(),
        ));
    };

    let mut result = String::new();
    for segment in segments {
        let Some(translated) = segment.get(0).and_then(|value| value.as_str()) else {
            continue;
        };
        result.push_str(translated);
    }

    if result.is_empty() {
        return Err(DomainError::InternalError(
            "Google Translate response missing translation text".to_string(),
        ));
    }

    Ok(result)
}

async fn translate_libre(
    client: reqwest::Client,
    url: Url,
    api_key: Option<String>,
    text: String,
    lang: String,
) -> Result<String, DomainError> {
    let mut payload = serde_json::json!({
        "q": text,
        "source": "auto",
        "target": lang,
        "format": "text",
    });

    if let Some(key) = api_key.as_deref().map(str::trim).filter(|value| !value.is_empty()) {
        payload["api_key"] = Value::String(key.to_string());
    }

    let response = client
        .post(url)
        .header(ACCEPT, "application/json")
        .header(CONTENT_TYPE, "application/json")
        .json(&payload)
        .send()
        .await
        .map_err(|error| {
            DomainError::InternalError(format!("LibreTranslate request failed: {error}"))
        })?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(DomainError::InternalError(format!(
            "LibreTranslate error: HTTP {status} {body}"
        )));
    }

    let json: Value = response.json().await.map_err(|error| {
        DomainError::InternalError(format!(
            "LibreTranslate response is not valid JSON: {error}"
        ))
    })?;

    let translated = json
        .get("translatedText")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string();

    if translated.is_empty() {
        return Err(DomainError::InternalError(
            "LibreTranslate response missing translatedText".to_string(),
        ));
    }

    Ok(translated)
}

async fn translate_lingva(
    client: reqwest::Client,
    base_url: Url,
    text: String,
    lang: String,
) -> Result<String, DomainError> {
    let mut url = base_url;
    {
        let mut segments = url.path_segments_mut().map_err(|_| {
            DomainError::InvalidData("Lingva URL cannot be a base".to_string())
        })?;
        segments.push("auto");
        segments.push(lang.as_str());
        segments.push(text.as_str());
    }

    let response = client
        .get(url)
        .header(ACCEPT, "application/json")
        .send()
        .await
        .map_err(|error| {
            DomainError::InternalError(format!("Lingva request failed: {error}"))
        })?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(DomainError::InternalError(format!(
            "Lingva error: HTTP {status} {body}"
        )));
    }

    let json: Value = response.json().await.map_err(|error| {
        DomainError::InternalError(format!("Lingva response is not valid JSON: {error}"))
    })?;

    let translated = json
        .get("translation")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string();

    if translated.is_empty() {
        return Err(DomainError::InternalError(
            "Lingva response missing translation".to_string(),
        ));
    }

    Ok(translated)
}

async fn translate_deepl(
    client: reqwest::Client,
    endpoint_url: &str,
    auth_key: String,
    text: String,
    lang: String,
) -> Result<String, DomainError> {
    let body = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("text", &text)
        .append_pair("target_lang", &lang)
        .finish();

    let response = client
        .post(endpoint_url)
        .header(ACCEPT, "application/json")
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(AUTHORIZATION, format!("DeepL-Auth-Key {auth_key}"))
        .body(body)
        .send()
        .await
        .map_err(|error| DomainError::InternalError(format!("DeepL request failed: {error}")))?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(DomainError::InternalError(format!(
            "DeepL error: HTTP {status} {body}"
        )));
    }

    let json: Value = response.json().await.map_err(|error| {
        DomainError::InternalError(format!("DeepL response is not valid JSON: {error}"))
    })?;

    let translated = json
        .get("translations")
        .and_then(|value| value.as_array())
        .and_then(|value| value.first())
        .and_then(|value| value.get("text"))
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string();

    if translated.is_empty() {
        return Err(DomainError::InternalError(
            "DeepL response missing translations[0].text".to_string(),
        ));
    }

    Ok(translated)
}

async fn translate_deeplx(
    client: reqwest::Client,
    url: Url,
    text: String,
    lang: String,
) -> Result<String, DomainError> {
    let response = client
        .post(url)
        .header(ACCEPT, "application/json")
        .header(CONTENT_TYPE, "application/json")
        .json(&serde_json::json!({
            "text": text,
            "source_lang": "auto",
            "target_lang": lang,
        }))
        .send()
        .await
        .map_err(|error| DomainError::InternalError(format!("DeepLX request failed: {error}")))?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(DomainError::InternalError(format!(
            "DeepLX error: HTTP {status} {body}"
        )));
    }

    let json: Value = response.json().await.map_err(|error| {
        DomainError::InternalError(format!("DeepLX response is not valid JSON: {error}"))
    })?;

    let translated = json
        .get("data")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string();

    if translated.is_empty() {
        return Err(DomainError::InternalError(
            "DeepLX response missing data".to_string(),
        ));
    }

    Ok(translated)
}

async fn translate_onering(
    client: reqwest::Client,
    url: Url,
    text: String,
    from_lang: String,
    to_lang: String,
) -> Result<String, DomainError> {
    let response = client
        .get(url)
        .query(&[
            ("text", text.as_str()),
            ("from_lang", from_lang.as_str()),
            ("to_lang", to_lang.as_str()),
        ])
        .header(ACCEPT, "application/json")
        .send()
        .await
        .map_err(|error| {
            DomainError::InternalError(format!("OneRing request failed: {error}"))
        })?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(DomainError::InternalError(format!(
            "OneRing error: HTTP {status} {body}"
        )));
    }

    let json: Value = response.json().await.map_err(|error| {
        DomainError::InternalError(format!("OneRing response is not valid JSON: {error}"))
    })?;

    let translated = json
        .get("result")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string();

    if translated.is_empty() {
        return Err(DomainError::InternalError(
            "OneRing response missing result".to_string(),
        ));
    }

    Ok(translated)
}

#[cfg(test)]
mod tests {
    use super::parse_google_translation;

    #[test]
    fn google_translation_parser_collects_segments() {
        let json = serde_json::json!([
            [
                ["Hello ", "你好", null, null, 1],
                ["world", "世界", null, null, 1]
            ],
            null,
            "zh-CN"
        ]);

        let translated = parse_google_translation(json).unwrap();
        assert_eq!(translated, "Hello world");
    }
}
