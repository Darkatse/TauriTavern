use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use futures_util::StreamExt;
use reqwest::header::{ACCEPT, CONTENT_TYPE};
use serde_json::{Value, json};

use super::{send_with_retry, upstream_error_response};
use tt_domain::errors::DomainError;
use tt_ports::repositories::tts_repository::{TtsRouteResponse, VolcengineTtsRequest};

pub(super) async fn generate(
    client: reqwest::Client,
    request: VolcengineTtsRequest,
) -> Result<TtsRouteResponse, DomainError> {
    let VolcengineTtsRequest {
        app_id,
        access_key,
        provider_endpoint,
        resource_id,
        text,
        voice_speaker,
        speed,
    } = request;
    let payload = json!({
        "req_params": {
            "text": text,
            "speaker": voice_speaker,
            "audio_params": {
                "format": "mp3",
                "speech_rate": speed,
            },
            "additions": json!({
                "mute_cut_threshold": "400",
                "mute_cut_remain_ms": "1",
                "explicit_language": "crosslingual",
                "enable_language_detector": true,
                "disable_markdown_filter": true,
                "cache_config": {
                    "use_cache": true,
                    "text_type": 1,
                },
            }).to_string(),
        },
    });
    let response = send_with_retry("Volcengine TTS request", || {
        client
            .post(&provider_endpoint)
            .header("X-Api-App-Id", &app_id)
            .header("X-Api-Access-Key", &access_key)
            .header("X-Api-Resource-Id", &resource_id)
            .header(ACCEPT, "*/*")
            .header(CONTENT_TYPE, "application/json")
            .json(&payload)
    })
    .await?;
    if !response.status().is_success() {
        return upstream_error_response(response, "Volcengine TTS request failed").await;
    }

    let mut stream = response.bytes_stream();
    let mut pending = Vec::new();
    let mut audio = Vec::new();
    while let Some(chunk) = stream.next().await {
        pending.extend_from_slice(&chunk.map_err(|error| {
            DomainError::InternalError(format!("Volcengine TTS stream read failed: {error}"))
        })?);
        while let Some(index) = pending.iter().position(|byte| *byte == b'\n') {
            let line = pending.drain(..=index).collect::<Vec<_>>();
            if let Err(message) = decode_line(&line[..line.len() - 1], &mut audio) {
                return Ok(TtsRouteResponse::text(502, message));
            }
        }
    }
    if let Err(message) = decode_line(&pending, &mut audio) {
        return Ok(TtsRouteResponse::text(502, message));
    }
    Ok(TtsRouteResponse::bytes(200, "audio/mpeg", audio))
}

fn decode_line(line: &[u8], audio: &mut Vec<u8>) -> Result<(), String> {
    let line = String::from_utf8_lossy(line);
    let line = line.trim();
    if line.is_empty() {
        return Ok(());
    }
    let payload: Value = serde_json::from_str(line)
        .map_err(|error| format!("Volcengine TTS stream line is invalid: {error}"))?;
    let code = payload
        .get("code")
        .and_then(Value::as_i64)
        .ok_or_else(|| "Volcengine TTS stream line did not include a status code".to_string())?;
    if !matches!(code, 0 | 20_000_000) {
        let message = payload
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("Unknown error");
        return Err(format!(
            "Volcengine TTS stream failed with code {code}: {message}"
        ));
    }
    if let Some(data) = payload.get("data").and_then(Value::as_str) {
        audio.extend(
            BASE64_STANDARD
                .decode(data)
                .map_err(|error| format!("Volcengine TTS audio is not valid base64: {error}"))?,
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::decode_line;

    #[test]
    fn decodes_volcengine_audio_lines() {
        let mut audio = Vec::new();
        decode_line(br#"{"code":20000000,"data":"AQI="}"#, &mut audio).unwrap();
        decode_line(br#"{"code":0,"data":"AwQ="}"#, &mut audio).unwrap();
        assert_eq!(audio, [1, 2, 3, 4]);

        assert_eq!(
            decode_line(
                br#"{"code":30000000,"message":"invalid voice"}"#,
                &mut audio
            )
            .unwrap_err(),
            "Volcengine TTS stream failed with code 30000000: invalid voice",
        );
    }
}
