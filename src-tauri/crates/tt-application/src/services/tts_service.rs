use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use serde_json::Value;

use crate::dto::tts_dto::TtsRouteResponseDto;
use crate::errors::ApplicationError;
use tt_domain::models::secret::SecretKeys;
use tt_ports::repositories::secret_repository::SecretRepository;
use tt_ports::repositories::tts_repository::{
    GrokOutputFormat, MinimaxGenerateRequest, TtsRepository, TtsRequest, TtsRouteResponse,
};

const MIMO_MODELS: &[&str] = &["mimo-v2-tts", "mimo-v2.5-tts"];
const MIMO_FORMATS: &[&str] = &["wav", "mp3"];
const MINIMAX_TTS_DEFAULT_API_HOST: &str = "https://api.minimax.io";
const MINIMAX_TTS_CN_API_HOST: &str = "https://api.minimaxi.com";

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
            "minimax/generate-voice" => {
                let Some(api_key) = self.read_secret(SecretKeys::MINIMAX).await? else {
                    return Ok(minimax_error_response(400, "MiniMax API key is required").into());
                };

                let text = optional_string(&body, "text").unwrap_or_default();
                if text.is_empty() {
                    return Ok(minimax_error_response(400, "No text provided").into());
                }

                let voice_id = string_or_default(&body, "voiceId", "");
                if voice_id.is_empty() {
                    return Ok(minimax_error_response(400, "No MiniMax voice provided").into());
                }

                let api_host = match minimax_api_host(&body) {
                    Ok(api_host) => api_host,
                    Err(response) => return Ok(response.into()),
                };
                let Some(speed) = finite_f64_or_default(&body, "speed", 1.0)
                    .filter(|speed| (0.5..=2.0).contains(speed))
                else {
                    return Ok(minimax_error_response(
                        400,
                        "MiniMax speed must be a number between 0.5 and 2",
                    )
                    .into());
                };
                let Some(volume) = finite_f64_or_default(&body, "volume", 1.0)
                    .filter(|volume| *volume > 0.0 && *volume <= 10.0)
                else {
                    return Ok(minimax_error_response(
                        400,
                        "MiniMax volume must be greater than 0 and at most 10",
                    )
                    .into());
                };
                let Some(pitch) = minimax_pitch(&body) else {
                    return Ok(minimax_error_response(
                        400,
                        "MiniMax pitch must be an integer between -12 and 12",
                    )
                    .into());
                };

                TtsRequest::MinimaxGenerate {
                    request: MinimaxGenerateRequest {
                        api_key,
                        text,
                        voice_id,
                        api_host,
                        model: string_or_default(&body, "model", "speech-02-hd"),
                        speed,
                        volume,
                        pitch,
                        audio_sample_rate: number_or_default(&body, "audioSampleRate", 32_000),
                        bitrate: number_or_default(&body, "bitrate", 128_000),
                        format: string_or_default(&body, "format", "mp3").to_lowercase(),
                        language_boost: optional_string(&body, "language"),
                    },
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

fn minimax_error_response(status: u16, message: impl Into<String>) -> TtsRouteResponse {
    TtsRouteResponse::json_error(status, message)
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

fn minimax_api_host(body: &Value) -> Result<String, TtsRouteResponse> {
    let api_host = string_or_default(body, "apiHost", MINIMAX_TTS_DEFAULT_API_HOST);
    let normalized = api_host.trim().trim_end_matches('/').to_ascii_lowercase();
    match normalized.as_str() {
        MINIMAX_TTS_DEFAULT_API_HOST | MINIMAX_TTS_CN_API_HOST => Ok(normalized),
        _ => Err(minimax_error_response(
            400,
            format!("Unsupported MiniMax API host: {api_host}"),
        )),
    }
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

fn finite_f64_or_default(body: &Value, key: &str, default: f64) -> Option<f64> {
    let Some(value) = body.as_object().and_then(|object| object.get(key)) else {
        return Some(default);
    };

    let number = value.as_f64().or_else(|| {
        value
            .as_str()
            .and_then(|raw| raw.trim().parse::<f64>().ok())
    })?;

    number.is_finite().then_some(number)
}

fn minimax_pitch(body: &Value) -> Option<i64> {
    let pitch = finite_f64_or_default(body, "pitch", 0.0)?;
    (pitch.fract() == 0.0 && (-12.0..=12.0).contains(&pitch)).then_some(pitch as i64)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        MINIMAX_TTS_CN_API_HOST, MINIMAX_TTS_DEFAULT_API_HOST, minimax_api_host, minimax_pitch,
    };

    #[test]
    fn minimax_api_host_accepts_known_hosts_and_normalizes_trailing_slash() {
        for (api_host, expected) in [
            ("https://api.minimax.io/", MINIMAX_TTS_DEFAULT_API_HOST),
            ("https://api.minimaxi.com/", MINIMAX_TTS_CN_API_HOST),
        ] {
            assert_eq!(
                minimax_api_host(&json!({ "apiHost": api_host })).unwrap(),
                expected
            );
        }
    }

    #[test]
    fn minimax_api_host_defaults_to_official_host() {
        assert_eq!(
            minimax_api_host(&json!({})).unwrap(),
            MINIMAX_TTS_DEFAULT_API_HOST
        );
    }

    #[test]
    fn minimax_api_host_rejects_untrusted_hosts() {
        let response = minimax_api_host(&json!({
            "apiHost": "https://example.test"
        }))
        .expect_err("untrusted host should be rejected");

        assert_eq!(response.status, 400);
        assert_eq!(response.content_type, "application/json; charset=utf-8");
        assert_eq!(response.status_text, None);
        assert_eq!(
            String::from_utf8(response.body).unwrap(),
            r#"{"error":"Unsupported MiniMax API host: https://example.test"}"#
        );
    }

    #[test]
    fn minimax_pitch_accepts_only_contract_integer_range() {
        assert_eq!(minimax_pitch(&json!({ "pitch": 0.0 })), Some(0));
        assert_eq!(minimax_pitch(&json!({ "pitch": "-12" })), Some(-12));
        assert_eq!(minimax_pitch(&json!({ "pitch": 0.5 })), None);
        assert_eq!(minimax_pitch(&json!({ "pitch": 13 })), None);
    }
}
