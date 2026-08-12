use reqwest::header::{ACCEPT, CONTENT_TYPE};
use serde_json::{Value, json};

use super::{bytes_response, response_content_type, send_with_retry, upstream_error_response};
use tt_domain::errors::DomainError;
use tt_ports::repositories::tts_repository::{GrokOutputFormat, TtsRouteResponse};

const VOICES_URL: &str = "https://api.x.ai/v1/tts/voices";
const TTS_URL: &str = "https://api.x.ai/v1/tts";

pub(super) async fn voices(
    client: reqwest::Client,
    api_key: String,
) -> Result<TtsRouteResponse, DomainError> {
    let response = send_with_retry("Grok voice list request", || {
        client
            .get(VOICES_URL)
            .bearer_auth(&api_key)
            .header(ACCEPT, "application/json")
    })
    .await?;

    if !response.status().is_success() {
        return upstream_error_response(response, "Grok voice list request failed").await;
    }

    let content_type = response_content_type(&response, "application/json");
    let bytes = response.bytes().await.map_err(|error| {
        DomainError::InternalError(format!("Grok voice list response read failed: {error}"))
    })?;

    if let Err(error) = serde_json::from_slice::<Value>(&bytes) {
        return Ok(TtsRouteResponse::text(
            502,
            format!("Grok voice list response is not valid JSON: {error}"),
        ));
    }

    Ok(TtsRouteResponse::bytes(200, content_type, bytes.to_vec()))
}

pub(super) async fn generate(
    client: reqwest::Client,
    api_key: String,
    text: String,
    voice_id: String,
    language: String,
    output_format: GrokOutputFormat,
) -> Result<TtsRouteResponse, DomainError> {
    let payload = json!({
        "text": text,
        "voice_id": voice_id,
        "language": language,
        "output_format": {
            "codec": output_format.codec,
            "sample_rate": output_format.sample_rate,
            "bit_rate": output_format.bit_rate,
        },
    });

    let response = send_with_retry("Grok TTS request", || {
        client
            .post(TTS_URL)
            .bearer_auth(&api_key)
            .header(ACCEPT, "*/*")
            .header(CONTENT_TYPE, "application/json")
            .json(&payload)
    })
    .await?;

    bytes_response(response, "Grok TTS request", "audio/mpeg", true).await
}
