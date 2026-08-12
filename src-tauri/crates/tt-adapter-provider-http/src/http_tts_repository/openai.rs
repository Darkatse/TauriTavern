use reqwest::header::{ACCEPT, CONTENT_TYPE};
use serde_json::{Map, Value, json};

use super::{bytes_response, send_with_retry, upstream_error_response};
use tt_domain::errors::DomainError;
use tt_ports::repositories::tts_repository::{
    ElectronHubTtsRequest, OpenAiTtsRequest, TtsRouteResponse,
};

const OPENAI_TTS_URL: &str = "https://api.openai.com/v1/audio/speech";
const ELECTRONHUB_TTS_URL: &str = "https://api.electronhub.ai/v1/audio/speech";
const ELECTRONHUB_MODELS_URL: &str = "https://api.electronhub.ai/v1/models";
const CHUTES_TTS_URL: &str = "https://chutes-kokoro.chutes.ai/speak";

pub(super) async fn handle_openai(
    client: reqwest::Client,
    request: OpenAiTtsRequest,
) -> Result<TtsRouteResponse, DomainError> {
    match request {
        OpenAiTtsRequest::Generate {
            api_key,
            text,
            voice,
            model,
            speed,
            instructions,
        } => generate_openai(client, api_key, text, voice, model, speed, instructions).await,
        OpenAiTtsRequest::CompatibleGenerate {
            api_key,
            endpoint,
            input,
            voice,
            model,
            response_format,
            speed,
        } => {
            let payload = json!({
                "input": input,
                "response_format": response_format,
                "voice": voice,
                "speed": speed,
                "model": model,
            });
            let response = send_with_retry("OpenAI-compatible TTS request", || {
                client
                    .post(endpoint.clone())
                    .bearer_auth(api_key.as_deref().unwrap_or_default())
                    .header(ACCEPT, "*/*")
                    .header(CONTENT_TYPE, "application/json")
                    .json(&payload)
            })
            .await?;
            bytes_response(
                response,
                "OpenAI-compatible TTS request",
                "audio/mpeg",
                false,
            )
            .await
        }
    }
}

async fn generate_openai(
    client: reqwest::Client,
    api_key: String,
    text: String,
    voice: String,
    model: String,
    speed: f64,
    instructions: Option<String>,
) -> Result<TtsRouteResponse, DomainError> {
    let mut payload = json!({
        "input": text,
        "response_format": "mp3",
        "voice": voice,
        "speed": speed,
        "model": model,
    });
    if let Some(instructions) = instructions {
        payload["instructions"] = Value::String(instructions);
    }

    let response = send_with_retry("OpenAI TTS request", || {
        client
            .post(OPENAI_TTS_URL)
            .bearer_auth(&api_key)
            .header(ACCEPT, "*/*")
            .header(CONTENT_TYPE, "application/json")
            .json(&payload)
    })
    .await?;
    bytes_response(response, "OpenAI TTS request", "audio/mpeg", false).await
}

pub(super) async fn handle_electronhub(
    client: reqwest::Client,
    request: ElectronHubTtsRequest,
) -> Result<TtsRouteResponse, DomainError> {
    match request {
        ElectronHubTtsRequest::Models { api_key } => {
            let response = send_with_retry("ElectronHub model list request", || {
                client
                    .get(ELECTRONHUB_MODELS_URL)
                    .bearer_auth(&api_key)
                    .header(ACCEPT, "application/json")
            })
            .await?;
            if !response.status().is_success() {
                return upstream_error_response(response, "ElectronHub model list request failed")
                    .await;
            }
            let payload: Value = response.json().await.map_err(|error| {
                DomainError::InternalError(format!(
                    "ElectronHub model list response read failed: {error}"
                ))
            })?;
            let models = payload
                .get("data")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            Ok(TtsRouteResponse::bytes(
                200,
                "application/json; charset=utf-8",
                serde_json::to_vec(&models).map_err(|error| {
                    DomainError::InternalError(format!(
                        "ElectronHub model list response encode failed: {error}"
                    ))
                })?,
            ))
        }
        ElectronHubTtsRequest::Generate {
            api_key,
            mut payload,
        } => {
            let Some(payload) = payload.as_object_mut() else {
                return Err(DomainError::InvalidData(
                    "ElectronHub TTS payload must be an object".to_string(),
                ));
            };
            normalize_electronhub_payload(payload);
            let response = send_with_retry("ElectronHub TTS request", || {
                client
                    .post(ELECTRONHUB_TTS_URL)
                    .bearer_auth(&api_key)
                    .header(ACCEPT, "*/*")
                    .header(CONTENT_TYPE, "application/json")
                    .json(&payload)
            })
            .await?;
            bytes_response(response, "ElectronHub TTS request", "audio/mpeg", true).await
        }
    }
}

fn normalize_electronhub_payload(payload: &mut Map<String, Value>) {
    if payload.get("speed").is_none_or(Value::is_null) {
        payload.insert("speed".to_string(), json!(1));
    }
    if !payload
        .get("model")
        .and_then(Value::as_str)
        .is_some_and(|model| !model.is_empty())
    {
        payload.insert("model".to_string(), json!("tts-1"));
    }
    payload.insert("response_format".to_string(), json!("mp3"));
}

pub(super) async fn generate_chutes(
    client: reqwest::Client,
    api_key: String,
    input: String,
    voice: String,
    speed: f64,
) -> Result<TtsRouteResponse, DomainError> {
    let payload = json!({
        "text": input,
        "voice": voice,
        "speed": speed,
    });
    let response = send_with_retry("Chutes TTS request", || {
        client
            .post(CHUTES_TTS_URL)
            .bearer_auth(&api_key)
            .header(ACCEPT, "*/*")
            .header(CONTENT_TYPE, "application/json")
            .json(&payload)
    })
    .await?;
    bytes_response(response, "Chutes TTS request", "audio/mpeg", true).await
}

#[cfg(test)]
mod tests {
    use serde_json::{Map, json};

    use super::normalize_electronhub_payload;

    #[test]
    fn electronhub_preserves_dynamic_parameters_and_sets_contract_fields() {
        let mut payload = Map::from_iter([
            ("model".to_string(), json!("custom-model")),
            ("custom_parameter".to_string(), json!("selected")),
        ]);
        normalize_electronhub_payload(&mut payload);

        assert_eq!(payload["model"], "custom-model");
        assert_eq!(payload["speed"], 1);
        assert_eq!(payload["response_format"], "mp3");
        assert_eq!(payload["custom_parameter"], "selected");
    }
}
