use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use reqwest::header::{ACCEPT, CONTENT_TYPE};
use serde_json::{Value, json};
use uuid::Uuid;

use super::{send_with_retry, upstream_error_response};
use tt_domain::errors::DomainError;
use tt_ports::repositories::tts_repository::{PollinationsTtsRequest, TtsRouteResponse};

const POLLINATIONS_MODELS_URL: &str = "https://gen.pollinations.ai/text/models";
const POLLINATIONS_GENERATE_URL: &str = "https://gen.pollinations.ai/v1/chat/completions";

pub(super) async fn handle(
    client: reqwest::Client,
    request: PollinationsTtsRequest,
) -> Result<TtsRouteResponse, DomainError> {
    match request {
        PollinationsTtsRequest::Voices { model } => voices(client, model).await,
        PollinationsTtsRequest::Generate {
            api_key,
            text,
            model,
            voice,
        } => generate(client, api_key, text, model, voice).await,
    }
}

async fn voices(client: reqwest::Client, model: String) -> Result<TtsRouteResponse, DomainError> {
    let response = send_with_retry("Pollinations model list request", || {
        client
            .get(POLLINATIONS_MODELS_URL)
            .header(ACCEPT, "application/json")
    })
    .await?;
    if !response.status().is_success() {
        return upstream_error_response(response, "Pollinations model list request failed").await;
    }
    let payload: Value = response.json().await.map_err(|error| {
        DomainError::InternalError(format!(
            "Pollinations model list response read failed: {error}"
        ))
    })?;
    let Some(voices) = payload
        .as_array()
        .and_then(|models| {
            models.iter().find(|candidate| {
                candidate.get("name").and_then(Value::as_str) == Some(model.as_str())
            })
        })
        .and_then(|model| model.get("voices"))
        .and_then(Value::as_array)
    else {
        return Ok(TtsRouteResponse::text(
            400,
            format!("Pollinations model `{model}` did not include a voice list"),
        ));
    };
    Ok(TtsRouteResponse::bytes(
        200,
        "application/json; charset=utf-8",
        serde_json::to_vec(voices).map_err(|error| {
            DomainError::InternalError(format!(
                "Pollinations voice list response encode failed: {error}"
            ))
        })?,
    ))
}

async fn generate(
    client: reqwest::Client,
    api_key: String,
    text: String,
    model: String,
    voice: String,
) -> Result<TtsRouteResponse, DomainError> {
    let payload = json!({
        "model": model,
        "stream": false,
        "modalities": ["text", "audio"],
        "seed": Uuid::new_v4().as_u128() as u32,
        "audio": {
            "format": "mp3",
            "voice": voice,
        },
        "messages": [{
            "role": "user",
            "content": text,
        }],
    });
    let response = send_with_retry("Pollinations TTS request", || {
        client
            .post(POLLINATIONS_GENERATE_URL)
            .bearer_auth(&api_key)
            .header(ACCEPT, "application/json")
            .header(CONTENT_TYPE, "application/json")
            .json(&payload)
    })
    .await?;
    if !response.status().is_success() {
        return upstream_error_response(response, "Pollinations TTS request failed").await;
    }
    let payload: Value = response.json().await.map_err(|error| {
        DomainError::InternalError(format!("Pollinations TTS response read failed: {error}"))
    })?;
    let Some(encoded) = payload
        .get("choices")
        .and_then(|value| value.get(0))
        .and_then(|value| value.get("message"))
        .and_then(|value| value.get("audio"))
        .and_then(|value| value.get("data"))
        .and_then(Value::as_str)
    else {
        return Ok(TtsRouteResponse::text(
            502,
            "Pollinations TTS response did not include audio data",
        ));
    };
    let audio = match BASE64_STANDARD.decode(encoded) {
        Ok(audio) => audio,
        Err(error) => {
            return Ok(TtsRouteResponse::text(
                502,
                format!("Pollinations TTS audio is not valid base64: {error}"),
            ));
        }
    };
    Ok(TtsRouteResponse::bytes(200, "audio/mpeg", audio))
}

#[cfg(test)]
mod tests {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
    use serde_json::json;

    #[test]
    fn pollinations_audio_shape_matches_frontend_contract() {
        let payload = json!({
            "choices": [{"message": {"audio": {"data": BASE64_STANDARD.encode([1, 2, 3])}}}]
        });
        let encoded = payload["choices"][0]["message"]["audio"]["data"]
            .as_str()
            .unwrap();
        assert_eq!(BASE64_STANDARD.decode(encoded).unwrap(), [1, 2, 3]);
    }
}
