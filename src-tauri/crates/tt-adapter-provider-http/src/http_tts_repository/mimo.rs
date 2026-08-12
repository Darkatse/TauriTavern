use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use reqwest::header::{ACCEPT, CONTENT_TYPE};
use serde_json::{Value, json};

use super::{send_with_retry, upstream_error_response};
use tt_domain::errors::DomainError;
use tt_ports::repositories::tts_repository::TtsRouteResponse;

const CHAT_COMPLETIONS_URL: &str = "https://api.xiaomimimo.com/v1/chat/completions";

pub(super) async fn generate(
    client: reqwest::Client,
    api_key: String,
    text: String,
    voice_id: String,
    model: String,
    format: String,
    instructions: Option<String>,
) -> Result<TtsRouteResponse, DomainError> {
    let mut messages = Vec::new();
    if let Some(instructions) = instructions {
        messages.push(json!({
            "role": "user",
            "content": instructions,
        }));
    }
    messages.push(json!({
        "role": "assistant",
        "content": text,
    }));

    let payload = json!({
        "model": model,
        "messages": messages,
        "audio": {
            "format": format,
            "voice": voice_id,
        },
    });
    let response = send_with_retry("MiMo TTS request", || {
        client
            .post(CHAT_COMPLETIONS_URL)
            .header("api-key", api_key.as_str())
            .header(ACCEPT, "application/json")
            .header(CONTENT_TYPE, "application/json")
            .json(&payload)
    })
    .await?;

    if !response.status().is_success() {
        return upstream_error_response(response, "MiMo TTS request failed").await;
    }

    let bytes = response.bytes().await.map_err(|error| {
        DomainError::InternalError(format!("MiMo TTS response read failed: {error}"))
    })?;
    let payload: Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(error) => {
            return Ok(TtsRouteResponse::text(
                502,
                format!("MiMo TTS response is not valid JSON: {error}"),
            ));
        }
    };
    let Some(audio_base64) = payload
        .get("choices")
        .and_then(|value| value.get(0))
        .and_then(|value| value.get("message"))
        .and_then(|value| value.get("audio"))
        .and_then(|value| value.get("data"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    else {
        return Ok(TtsRouteResponse::text(
            502,
            "MiMo TTS response did not include audio data",
        ));
    };
    let audio = match BASE64_STANDARD.decode(audio_base64) {
        Ok(audio) => audio,
        Err(error) => {
            return Ok(TtsRouteResponse::text(
                502,
                format!("MiMo TTS audio data is not valid base64: {error}"),
            ));
        }
    };
    Ok(TtsRouteResponse::bytes(200, content_type(&format), audio))
}

fn content_type(format: &str) -> String {
    match format {
        "mp3" => "audio/mpeg".to_string(),
        format => format!("audio/{format}"),
    }
}

#[cfg(test)]
mod tests {
    use super::content_type;

    #[test]
    fn preserves_user_selected_audio_format() {
        assert_eq!(content_type("mp3"), "audio/mpeg");
        assert_eq!(content_type("flac"), "audio/flac");
    }
}
