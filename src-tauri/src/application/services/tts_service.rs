use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use serde_json::Value;

use crate::application::dto::tts_dto::TtsRouteResponseDto;
use crate::application::errors::ApplicationError;
use crate::domain::models::secret::SecretKeys;
use crate::domain::repositories::secret_repository::SecretRepository;
use crate::domain::repositories::tts_repository::{
    GrokOutputFormat, TtsRepository, TtsRequest, TtsRouteResponse,
};

const MIMO_MODELS: &[&str] = &["mimo-v2-tts", "mimo-v2.5-tts"];
const MIMO_FORMATS: &[&str] = &["wav", "mp3"];

pub struct TtsService {
    tts_repository: Arc<dyn TtsRepository>,
    secret_repository: Arc<dyn SecretRepository>,
}

impl TtsService {
    pub fn new(
        tts_repository: Arc<dyn TtsRepository>,
        secret_repository: Arc<dyn SecretRepository>,
    ) -> Self {
        Self {
            tts_repository,
            secret_repository,
        }
    }

    pub async fn handle_request(
        &self,
        path: String,
        body: Value,
    ) -> Result<TtsRouteResponseDto, ApplicationError> {
        let request = match normalize_path(&path).as_str() {
            "grok/voices" => {
                let Some(api_key) = self.read_secret(SecretKeys::XAI).await? else {
                    return Ok(text_response(400, "xAI API key is required").into());
                };

                TtsRequest::GrokVoices { api_key }
            }
            "grok/generate" => {
                let Some(api_key) = self.read_secret(SecretKeys::XAI).await? else {
                    return Ok(text_response(400, "xAI API key is required").into());
                };

                let text = optional_string(&body, "text").unwrap_or_default();
                if text.is_empty() {
                    return Ok(text_response(400, "No text provided").into());
                }

                let voice_id = string_or_default(&body, "voiceId", "eve").to_lowercase();
                if voice_id.is_empty() {
                    return Ok(text_response(400, "No Grok voice provided").into());
                }

                let language = string_or_default(&body, "language", "auto");
                let output_format = body
                    .as_object()
                    .and_then(|object| object.get("outputFormat"))
                    .filter(|value| value.is_object())
                    .unwrap_or(&Value::Null);

                TtsRequest::GrokGenerate {
                    api_key,
                    text,
                    voice_id,
                    language,
                    output_format: GrokOutputFormat {
                        codec: string_or_default(output_format, "codec", "mp3"),
                        sample_rate: number_or_default(output_format, "sampleRate", 24_000),
                        bit_rate: number_or_default(output_format, "bitRate", 128_000),
                    },
                }
            }
            "mimo/generate" => {
                let Some(api_key) = self.read_secret(SecretKeys::MIMO).await? else {
                    return Ok(text_response(400, "MiMo API key is required").into());
                };

                let text = optional_string(&body, "text").unwrap_or_default();
                if text.is_empty() {
                    return Ok(text_response(400, "No text provided").into());
                }

                let voice_id = string_or_default(&body, "voiceId", "mimo_default");
                let model = string_or_default(&body, "model", "mimo-v2-tts");
                if !MIMO_MODELS.contains(&model.as_str()) {
                    return Ok(
                        text_response(400, format!("Unsupported MiMo model: {model}")).into(),
                    );
                }

                let format = string_or_default(&body, "format", "wav").to_lowercase();
                if !MIMO_FORMATS.contains(&format.as_str()) {
                    return Ok(text_response(
                        400,
                        format!("Unsupported MiMo audio format: {format}"),
                    )
                    .into());
                }

                TtsRequest::MimoGenerate {
                    api_key,
                    text,
                    voice_id,
                    model,
                    format,
                    instructions: optional_string(&body, "instructions"),
                }
            }
            _ => {
                return Err(ApplicationError::NotFound(format!(
                    "Unsupported TTS route: {path}"
                )));
            }
        };

        Ok(self.tts_repository.handle(request).await?.into())
    }

    async fn read_secret(&self, key: &str) -> Result<Option<String>, ApplicationError> {
        Ok(self
            .secret_repository
            .read_secret(key, None)
            .await?
            .map(|secret| secret.trim().to_string())
            .filter(|secret| !secret.is_empty()))
    }
}

impl From<TtsRouteResponse> for TtsRouteResponseDto {
    fn from(response: TtsRouteResponse) -> Self {
        Self {
            status: response.status,
            content_type: response.content_type,
            body_base64: BASE64_STANDARD.encode(response.body),
            status_text: response.status_text,
        }
    }
}

fn text_response(status: u16, message: impl Into<String>) -> TtsRouteResponse {
    TtsRouteResponse::text(status, message)
}

fn normalize_path(path: &str) -> String {
    path.trim().trim_matches('/').to_lowercase()
}

fn optional_string(body: &Value, key: &str) -> Option<String> {
    body.as_object()
        .and_then(|object| object.get(key))
        .and_then(|value| value.as_str())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn string_or_default(body: &Value, key: &str, default: &str) -> String {
    optional_string(body, key).unwrap_or_else(|| default.to_string())
}

fn number_or_default(body: &Value, key: &str, default: u32) -> u32 {
    let Some(value) = body.as_object().and_then(|object| object.get(key)) else {
        return default;
    };

    if let Some(number) = value.as_u64().and_then(|number| u32::try_from(number).ok()) {
        return number;
    }

    value
        .as_str()
        .and_then(|raw| raw.trim().parse::<u32>().ok())
        .unwrap_or(default)
}
