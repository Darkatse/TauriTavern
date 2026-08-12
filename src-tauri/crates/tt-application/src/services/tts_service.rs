use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use serde_json::Value;

use crate::dto::tts_dto::TtsRouteResponseDto;
use crate::errors::ApplicationError;
use tt_domain::models::secret::SecretKeys;
use tt_ports::repositories::secret_repository::SecretRepository;
use tt_ports::repositories::tts_repository::{
    AzureTtsRequest, ElectronHubTtsRequest, ElevenLabsTtsRequest, ElevenLabsVoiceFile,
    GoogleGeminiTtsRequest, GoogleTranslateTtsRequest, GrokOutputFormat, MinimaxGenerateRequest,
    OpenAiTtsRequest, PollinationsTtsRequest, TtsRepository, TtsRequest, TtsRouteResponse,
    VolcengineTtsRequest,
};
use url::Url;

const MINIMAX_TTS_DEFAULT_API_HOST: &str = "https://api.minimax.io";
const GOOGLE_AI_STUDIO_API: &str = "https://generativelanguage.googleapis.com";
const VOLCENGINE_TTS_ENDPOINT: &str = "https://openspeech.bytedance.com/api/v3/tts/unidirectional";

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
            "azure/list" => {
                let Some(api_key) = self.read_secret(SecretKeys::AZURE_TTS).await? else {
                    return Ok(text_response(403, "Azure TTS API key is required").into());
                };
                let Some(region) = optional_string(&body, "region") else {
                    return Ok(text_response(400, "Azure TTS region is required").into());
                };
                TtsRequest::Azure(AzureTtsRequest::List { api_key, region })
            }
            "azure/generate" => {
                let Some(api_key) = self.read_secret(SecretKeys::AZURE_TTS).await? else {
                    return Ok(text_response(403, "Azure TTS API key is required").into());
                };
                let Some(region) = optional_string(&body, "region") else {
                    return Ok(text_response(400, "Azure TTS region is required").into());
                };
                let Some(text) = optional_content(&body, "text") else {
                    return Ok(text_response(400, "No text provided").into());
                };
                let Some(voice) = optional_string(&body, "voice") else {
                    return Ok(text_response(400, "No Azure TTS voice provided").into());
                };
                TtsRequest::Azure(AzureTtsRequest::Generate {
                    api_key,
                    region,
                    text,
                    voice,
                })
            }
            "google/list-voices" => {
                TtsRequest::GoogleTranslate(GoogleTranslateTtsRequest::ListVoices)
            }
            "google/generate-voice" => {
                let Some(text) = string_list(&body, "text") else {
                    return Ok(text_response(400, "No text provided").into());
                };
                TtsRequest::GoogleTranslate(GoogleTranslateTtsRequest::Generate {
                    text,
                    voice: string_or_default(&body, "voice", "en"),
                })
            }
            "google/list-native-voices" => {
                TtsRequest::GoogleGemini(GoogleGeminiTtsRequest::ListVoices)
            }
            "google/generate-native-tts" => {
                let Some(text) = optional_content(&body, "text") else {
                    return Ok(json_error_response(400, "No text provided").into());
                };
                let Some(voice) = optional_string(&body, "voice") else {
                    return Ok(json_error_response(400, "No Google TTS voice provided").into());
                };
                let Some(model) = optional_string(&body, "model") else {
                    return Ok(json_error_response(400, "No Google TTS model provided").into());
                };

                let (base_url, api_key) =
                    if let Some(base_url) = optional_string(&body, "reverse_proxy") {
                        (
                            base_url,
                            optional_content(&body, "proxy_password").unwrap_or_default(),
                        )
                    } else {
                        let Some(api_key) = self.read_secret(SecretKeys::MAKERSUITE).await? else {
                            return Ok(json_error_response(
                                400,
                                "Google AI Studio API key is required",
                            )
                            .into());
                        };
                        (GOOGLE_AI_STUDIO_API.to_string(), api_key)
                    };

                TtsRequest::GoogleGemini(GoogleGeminiTtsRequest::Generate {
                    text,
                    voice,
                    model,
                    base_url,
                    api_key,
                })
            }
            "novelai/generate-voice" => {
                let Some(api_key) = self.read_secret(SecretKeys::NOVEL).await? else {
                    return Ok(text_response(400, "NovelAI access token is required").into());
                };
                let Some(text) = optional_content(&body, "text") else {
                    return Ok(text_response(400, "No text provided").into());
                };
                let Some(voice) = optional_string(&body, "voice") else {
                    return Ok(text_response(400, "No NovelAI voice provided").into());
                };
                TtsRequest::NovelAiGenerate {
                    api_key,
                    text,
                    voice,
                }
            }
            "openai/generate-voice" => {
                let Some(api_key) = self.read_secret(SecretKeys::OPENAI).await? else {
                    return Ok(text_response(400, "OpenAI API key is required").into());
                };
                TtsRequest::OpenAi(OpenAiTtsRequest::Generate {
                    api_key,
                    text: optional_content(&body, "text").unwrap_or_default(),
                    voice: string_or_default(&body, "voice", "alloy"),
                    model: string_or_default(&body, "model", "tts-1"),
                    speed: finite_f64_or_default(&body, "speed", 1.0).unwrap_or(1.0),
                    instructions: optional_content(&body, "instructions"),
                })
            }
            "openai/custom/generate-voice" => {
                let Some(endpoint) = optional_string(&body, "provider_endpoint") else {
                    return Ok(text_response(400, "Provider endpoint is required").into());
                };
                let endpoint = match Url::parse(&endpoint) {
                    Ok(endpoint) => endpoint,
                    Err(error) => {
                        return Ok(text_response(
                            400,
                            format!("Invalid provider endpoint: {error}"),
                        )
                        .into());
                    }
                };
                TtsRequest::OpenAi(OpenAiTtsRequest::CompatibleGenerate {
                    api_key: self.read_secret(SecretKeys::CUSTOM_OPENAI_TTS).await?,
                    endpoint,
                    input: optional_content(&body, "input").unwrap_or_default(),
                    voice: string_or_default(&body, "voice", "alloy"),
                    model: string_or_default(&body, "model", "tts-1"),
                    response_format: string_or_default(&body, "response_format", "mp3"),
                    speed: finite_f64_or_default(&body, "speed", 1.0).unwrap_or(1.0),
                })
            }
            "openai/electronhub/models" => {
                let Some(api_key) = self.read_secret(SecretKeys::ELECTRONHUB).await? else {
                    return Ok(text_response(400, "ElectronHub API key is required").into());
                };
                TtsRequest::ElectronHub(ElectronHubTtsRequest::Models { api_key })
            }
            "openai/electronhub/generate-voice" => {
                let Some(api_key) = self.read_secret(SecretKeys::ELECTRONHUB).await? else {
                    return Ok(text_response(400, "ElectronHub API key is required").into());
                };
                if !body.is_object() {
                    return Ok(
                        text_response(400, "ElectronHub request body must be an object").into(),
                    );
                }
                TtsRequest::ElectronHub(ElectronHubTtsRequest::Generate {
                    api_key,
                    payload: body,
                })
            }
            "openai/chutes/generate-voice" => {
                let Some(api_key) = self.read_secret(SecretKeys::CHUTES).await? else {
                    return Ok(text_response(400, "Chutes API key is required").into());
                };
                TtsRequest::ChutesGenerate {
                    api_key,
                    input: optional_content(&body, "input").unwrap_or_default(),
                    voice: string_or_default(&body, "voice", "af_heart"),
                    speed: finite_f64_or_default(&body, "speed", 1.0).unwrap_or(1.0),
                }
            }
            "speech/elevenlabs/voices" => {
                let Some(api_key) = self.read_secret(SecretKeys::ELEVENLABS).await? else {
                    return Ok(text_response(400, "ElevenLabs API key is required").into());
                };
                TtsRequest::ElevenLabs(ElevenLabsTtsRequest::Voices { api_key })
            }
            "speech/elevenlabs/voice-settings" => {
                let Some(api_key) = self.read_secret(SecretKeys::ELEVENLABS).await? else {
                    return Ok(text_response(400, "ElevenLabs API key is required").into());
                };
                TtsRequest::ElevenLabs(ElevenLabsTtsRequest::VoiceSettings { api_key })
            }
            "speech/elevenlabs/synthesize" => {
                let Some(api_key) = self.read_secret(SecretKeys::ELEVENLABS).await? else {
                    return Ok(text_response(400, "ElevenLabs API key is required").into());
                };
                let Some(voice_id) = optional_string(&body, "voiceId") else {
                    return Ok(text_response(400, "ElevenLabs voice ID is required").into());
                };
                let Some(request) = body
                    .get("request")
                    .filter(|value| !value.is_null())
                    .cloned()
                else {
                    return Ok(text_response(400, "ElevenLabs request body is required").into());
                };
                TtsRequest::ElevenLabs(ElevenLabsTtsRequest::Synthesize {
                    api_key,
                    voice_id,
                    request,
                })
            }
            "speech/elevenlabs/history" => {
                let Some(api_key) = self.read_secret(SecretKeys::ELEVENLABS).await? else {
                    return Ok(text_response(400, "ElevenLabs API key is required").into());
                };
                TtsRequest::ElevenLabs(ElevenLabsTtsRequest::History { api_key })
            }
            "speech/elevenlabs/history-audio" => {
                let Some(api_key) = self.read_secret(SecretKeys::ELEVENLABS).await? else {
                    return Ok(text_response(400, "ElevenLabs API key is required").into());
                };
                let Some(history_item_id) = optional_string(&body, "historyItemId") else {
                    return Ok(text_response(400, "ElevenLabs history item ID is required").into());
                };
                TtsRequest::ElevenLabs(ElevenLabsTtsRequest::HistoryAudio {
                    api_key,
                    history_item_id,
                })
            }
            "speech/elevenlabs/voices/add" => {
                let Some(api_key) = self.read_secret(SecretKeys::ELEVENLABS).await? else {
                    return Ok(text_response(400, "ElevenLabs API key is required").into());
                };
                let files = match elevenlabs_voice_files(&body) {
                    Ok(files) => files,
                    Err(response) => return Ok(response.into()),
                };
                TtsRequest::ElevenLabs(ElevenLabsTtsRequest::AddVoice {
                    api_key,
                    name: string_or_default(&body, "name", "Custom Voice"),
                    description: string_or_default(
                        &body,
                        "description",
                        "Uploaded via SillyTavern",
                    ),
                    labels: optional_content(&body, "labels").unwrap_or_default(),
                    files,
                })
            }
            "speech/pollinations/voices" => {
                TtsRequest::Pollinations(PollinationsTtsRequest::Voices {
                    model: string_or_default(&body, "model", "openai-audio"),
                })
            }
            "speech/pollinations/generate" => {
                let Some(api_key) = self.read_secret(SecretKeys::POLLINATIONS).await? else {
                    return Ok(text_response(400, "Pollinations API key is required").into());
                };
                TtsRequest::Pollinations(PollinationsTtsRequest::Generate {
                    api_key,
                    text: optional_content(&body, "text").unwrap_or_default(),
                    model: string_or_default(&body, "model", "openai-audio"),
                    voice: string_or_default(&body, "voice", "alloy"),
                })
            }
            "volcengine/generate-voice" => {
                let Some(app_id) = self.read_secret(SecretKeys::VOLCENGINE_APP_ID).await? else {
                    return Ok(text_response(403, "Volcengine App ID is required").into());
                };
                let Some(access_key) = self.read_secret(SecretKeys::VOLCENGINE_ACCESS_KEY).await?
                else {
                    return Ok(text_response(403, "Volcengine access key is required").into());
                };
                let Some(resource_id) = optional_string(&body, "resource_id") else {
                    return Ok(text_response(400, "Volcengine resource ID is required").into());
                };
                let Some(text) = optional_content(&body, "text") else {
                    return Ok(text_response(400, "No text provided").into());
                };
                let Some(voice_speaker) = optional_string(&body, "voice_speaker") else {
                    return Ok(text_response(400, "Volcengine voice speaker is required").into());
                };
                TtsRequest::Volcengine(VolcengineTtsRequest {
                    app_id,
                    access_key,
                    provider_endpoint: string_or_default(
                        &body,
                        "provider_endpoint",
                        VOLCENGINE_TTS_ENDPOINT,
                    ),
                    resource_id,
                    text,
                    voice_speaker,
                    speed: integer_or_default(&body, "speed", 0),
                })
            }
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
                let format = string_or_default(&body, "format", "wav").to_lowercase();

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
                    return Ok(json_error_response(400, "MiniMax API key is required").into());
                };

                let text = optional_string(&body, "text").unwrap_or_default();
                if text.is_empty() {
                    return Ok(json_error_response(400, "No text provided").into());
                }

                let voice_id = string_or_default(&body, "voiceId", "");
                if voice_id.is_empty() {
                    return Ok(json_error_response(400, "No MiniMax voice provided").into());
                }

                let api_host = string_or_default(&body, "apiHost", MINIMAX_TTS_DEFAULT_API_HOST)
                    .trim()
                    .trim_end_matches('/')
                    .to_string();
                let Some(speed) = finite_f64_or_default(&body, "speed", 1.0) else {
                    return Ok(json_error_response(400, "MiniMax speed must be a number").into());
                };
                let Some(volume) = finite_f64_or_default(&body, "volume", 1.0) else {
                    return Ok(json_error_response(400, "MiniMax volume must be a number").into());
                };
                let Some(pitch) = minimax_pitch(&body) else {
                    return Ok(json_error_response(400, "MiniMax pitch must be an integer").into());
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

fn json_error_response(status: u16, message: impl Into<String>) -> TtsRouteResponse {
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

fn optional_content(body: &Value, key: &str) -> Option<String> {
    body.as_object()
        .and_then(|object| object.get(key))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
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

fn integer_or_default(body: &Value, key: &str, default: i64) -> i64 {
    let Some(value) = body.as_object().and_then(|object| object.get(key)) else {
        return default;
    };

    value
        .as_i64()
        .or_else(|| value.as_f64().map(|number| number.trunc() as i64))
        .or_else(|| {
            value
                .as_str()
                .and_then(|raw| raw.trim().parse::<f64>().ok())
                .map(|number| number.trunc() as i64)
        })
        .unwrap_or(default)
}

fn string_list(body: &Value, key: &str) -> Option<Vec<String>> {
    let value = body.as_object()?.get(key)?;
    let values = match value {
        Value::String(value) if !value.is_empty() => vec![value.clone()],
        Value::Array(values) => values
            .iter()
            .map(Value::as_str)
            .collect::<Option<Vec<_>>>()?
            .into_iter()
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect(),
        _ => return None,
    };

    (!values.is_empty()).then_some(values)
}

fn elevenlabs_voice_files(body: &Value) -> Result<Vec<ElevenLabsVoiceFile>, TtsRouteResponse> {
    let Some(values) = body.get("files") else {
        return Ok(Vec::new());
    };
    let Some(values) = values.as_array() else {
        return Err(text_response(
            400,
            "ElevenLabs voice files must be an array",
        ));
    };

    values
        .iter()
        .map(|value| {
            let Some(value) = value.as_str() else {
                return Err(text_response(
                    400,
                    "ElevenLabs voice file must be a base64 data URL",
                ));
            };
            let Some((mime_type, encoded)) = value
                .strip_prefix("data:")
                .and_then(|value| value.split_once(";base64,"))
            else {
                return Err(text_response(
                    400,
                    "ElevenLabs voice file must be a base64 data URL",
                ));
            };
            let bytes = BASE64_STANDARD.decode(encoded).map_err(|error| {
                text_response(
                    400,
                    format!("ElevenLabs voice file is not valid base64: {error}"),
                )
            })?;
            Ok(ElevenLabsVoiceFile {
                mime_type: mime_type.to_string(),
                bytes,
            })
        })
        .collect()
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
    (pitch.fract() == 0.0).then_some(pitch as i64)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use async_trait::async_trait;
    use serde_json::json;

    use super::{TtsService, minimax_pitch};
    use tt_domain::errors::DomainError;
    use tt_domain::models::secret::Secrets;
    use tt_ports::repositories::secret_repository::SecretRepository;
    use tt_ports::repositories::tts_repository::{TtsRepository, TtsRequest, TtsRouteResponse};

    struct TestSecretRepository;

    #[async_trait]
    impl SecretRepository for TestSecretRepository {
        async fn save(&self, _secrets: &Secrets) -> Result<(), DomainError> {
            Ok(())
        }

        async fn load(&self) -> Result<Secrets, DomainError> {
            Ok(Secrets::default())
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
            Ok("id".to_string())
        }

        async fn read_secret(
            &self,
            _key: &str,
            _id: Option<&str>,
        ) -> Result<Option<String>, DomainError> {
            Ok(Some("secret".to_string()))
        }

        async fn delete_secret(&self, _key: &str, _id: Option<&str>) -> Result<(), DomainError> {
            Ok(())
        }

        async fn rotate_secret(&self, _key: &str, _id: &str) -> Result<(), DomainError> {
            Ok(())
        }

        async fn rename_secret(
            &self,
            _key: &str,
            _id: &str,
            _label: &str,
        ) -> Result<(), DomainError> {
            Ok(())
        }
    }

    struct RecordingTtsRepository(AtomicUsize);

    #[async_trait]
    impl TtsRepository for RecordingTtsRepository {
        async fn handle(&self, _request: TtsRequest) -> Result<TtsRouteResponse, DomainError> {
            self.0.fetch_add(1, Ordering::Relaxed);
            Ok(TtsRouteResponse::bytes(
                200,
                "application/octet-stream",
                Vec::new(),
            ))
        }
    }

    #[test]
    fn minimax_pitch_accepts_integers_without_policy_range() {
        assert_eq!(minimax_pitch(&json!({ "pitch": 0.0 })), Some(0));
        assert_eq!(minimax_pitch(&json!({ "pitch": "-12" })), Some(-12));
        assert_eq!(minimax_pitch(&json!({ "pitch": 13 })), Some(13));
        assert_eq!(minimax_pitch(&json!({ "pitch": 0.5 })), None);
    }

    #[tokio::test]
    async fn resolves_every_network_provider_route() {
        let repository = Arc::new(RecordingTtsRepository(AtomicUsize::new(0)));
        let service = TtsService::new(repository.clone(), Arc::new(TestSecretRepository));
        let routes = [
            ("azure/list", json!({ "region": "eastus" })),
            (
                "azure/generate",
                json!({ "region": "eastus", "text": "hello", "voice": "en-US-Ava" }),
            ),
            ("google/list-voices", json!({})),
            (
                "google/generate-voice",
                json!({ "text": ["hello"], "voice": "en" }),
            ),
            ("google/list-native-voices", json!({})),
            (
                "google/generate-native-tts",
                json!({ "text": "hello", "voice": "Puck", "model": "gemini-2.5-flash-preview-tts" }),
            ),
            (
                "novelai/generate-voice",
                json!({ "text": "hello", "voice": "seed" }),
            ),
            ("openai/generate-voice", json!({ "text": "hello" })),
            (
                "openai/custom/generate-voice",
                json!({ "provider_endpoint": "http://localhost/audio", "input": "hello" }),
            ),
            ("openai/electronhub/models", json!({})),
            (
                "openai/electronhub/generate-voice",
                json!({ "input": "hello", "voice": "alloy" }),
            ),
            (
                "openai/chutes/generate-voice",
                json!({ "input": "hello", "voice": "af_heart" }),
            ),
            ("speech/elevenlabs/voices", json!({})),
            ("speech/elevenlabs/voice-settings", json!({})),
            (
                "speech/elevenlabs/synthesize",
                json!({ "voiceId": "voice", "request": { "text": "hello" } }),
            ),
            ("speech/elevenlabs/history", json!({})),
            (
                "speech/elevenlabs/history-audio",
                json!({ "historyItemId": "history" }),
            ),
            (
                "speech/elevenlabs/voices/add",
                json!({ "name": "voice", "files": [] }),
            ),
            (
                "speech/pollinations/voices",
                json!({ "model": "openai-audio" }),
            ),
            (
                "speech/pollinations/generate",
                json!({ "text": "hello", "voice": "alloy" }),
            ),
            (
                "volcengine/generate-voice",
                json!({ "resource_id": "resource", "text": "hello", "voice_speaker": "speaker" }),
            ),
            (
                "mimo/generate",
                json!({ "text": "hello", "model": "future-model", "format": "flac" }),
            ),
        ];
        let route_count = routes.len();

        for (path, body) in routes {
            assert_eq!(
                service
                    .handle_request(path.to_string(), body)
                    .await
                    .unwrap()
                    .status,
                200,
                "{path} should resolve to the repository",
            );
        }

        assert_eq!(repository.0.load(Ordering::Relaxed), route_count);
    }
}
