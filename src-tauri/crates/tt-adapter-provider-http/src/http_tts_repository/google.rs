use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use reqwest::header::{ACCEPT, CONTENT_TYPE};
use serde_json::{Value, json};
use uuid::Uuid;

use super::{parse_upstream_error_message, send_with_retry, upstream_error_response};
use crate::endpoint_url::append_google_api_path;
use tt_domain::errors::DomainError;
use tt_ports::repositories::tts_repository::{
    GoogleGeminiTtsRequest, GoogleTranslateTtsRequest, TtsRouteResponse,
};

const GOOGLE_TRANSLATE_TTS_URL: &str =
    "https://translate.google.com/_/TranslateWebserverUi/data/batchexecute";
// Keep this catalog aligned with the google-translate-api-x version bundled by SillyTavern.
const GOOGLE_TRANSLATE_LANGUAGES: &[u8] = include_bytes!("google_translate_languages.json");
const GOOGLE_NATIVE_VOICES: &[(&str, &str)] = &[
    ("Zephyr", "Bright"),
    ("Puck", "Upbeat"),
    ("Charon", "Informative"),
    ("Kore", "Firm"),
    ("Fenrir", "Excitable"),
    ("Leda", "Youthful"),
    ("Orus", "Firm"),
    ("Aoede", "Breezy"),
    ("Callirhoe", "Easy-going"),
    ("Autonoe", "Bright"),
    ("Enceladus", "Breathy"),
    ("Iapetus", "Clear"),
    ("Umbriel", "Easy-going"),
    ("Algieba", "Smooth"),
    ("Despina", "Smooth"),
    ("Erinome", "Clear"),
    ("Algenib", "Gravelly"),
    ("Rasalgethi", "Informative"),
    ("Laomedeia", "Upbeat"),
    ("Achernar", "Soft"),
    ("Alnilam", "Firm"),
    ("Schedar", "Even"),
    ("Gacrux", "Mature"),
    ("Pulcherrima", "Forward"),
    ("Achird", "Friendly"),
    ("Zubenelgenubi", "Casual"),
    ("Vindemiatrix", "Gentle"),
    ("Sadachbia", "Lively"),
    ("Sadaltager", "Knowledgeable"),
    ("Sulafat", "Warm"),
];

pub(super) async fn handle_translate(
    client: reqwest::Client,
    request: GoogleTranslateTtsRequest,
) -> Result<TtsRouteResponse, DomainError> {
    match request {
        GoogleTranslateTtsRequest::ListVoices => Ok(TtsRouteResponse::bytes(
            200,
            "application/json; charset=utf-8",
            GOOGLE_TRANSLATE_LANGUAGES.to_vec(),
        )),
        GoogleTranslateTtsRequest::Generate { text, voice } => {
            generate_translate(client, text, voice).await
        }
    }
}

async fn generate_translate(
    client: reqwest::Client,
    text: Vec<String>,
    voice: String,
) -> Result<TtsRouteResponse, DomainError> {
    let calls = text
        .iter()
        .enumerate()
        .map(|(index, text)| {
            json!([
                "jQ1olc",
                json!([text, voice, true]).to_string(),
                Value::Null,
                radix36(index),
            ])
        })
        .collect::<Vec<_>>();
    let form_request = json!([calls]).to_string();
    let form_body = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("f.req", &form_request)
        .finish();
    let request_id = 1000 + (Uuid::new_v4().as_u128() % 9000);
    let response = send_with_retry("Google Translate TTS request", || {
        client
            .post(GOOGLE_TRANSLATE_TTS_URL)
            .query(&[
                ("rpcids", "jQ1olc".to_string()),
                ("source-path", "/".to_string()),
                ("f.sid", String::new()),
                ("bl", String::new()),
                ("hl", "en-US".to_string()),
                ("soc-app", "1".to_string()),
                ("soc-platform", "1".to_string()),
                ("soc-device", "1".to_string()),
                ("_reqid", request_id.to_string()),
                ("rt", "c".to_string()),
            ])
            .header(ACCEPT, "*/*")
            .header(
                CONTENT_TYPE,
                "application/x-www-form-urlencoded;charset=UTF-8",
            )
            .body(form_body.clone())
    })
    .await?;
    if !response.status().is_success() {
        return upstream_error_response(response, "Google Translate TTS request failed").await;
    }
    let body = response.text().await.map_err(|error| {
        DomainError::InternalError(format!(
            "Google Translate TTS response read failed: {error}"
        ))
    })?;
    let encoded = parse_translate_audio(&body, text.len())?;
    let mut audio = Vec::new();
    for encoded in encoded {
        audio.extend(BASE64_STANDARD.decode(encoded).map_err(|error| {
            DomainError::InternalError(format!(
                "Google Translate TTS audio is not valid base64: {error}"
            ))
        })?);
    }
    Ok(TtsRouteResponse::bytes(200, "audio/mpeg", audio))
}

fn parse_translate_audio(body: &str, count: usize) -> Result<Vec<String>, DomainError> {
    let mut results = vec![None; count];
    for line in body.lines().filter(|line| line.starts_with('[')) {
        let Ok(chunks) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let Some(chunks) = chunks.as_array() else {
            continue;
        };
        for translation in chunks.iter().filter_map(Value::as_array) {
            if translation.first().and_then(Value::as_str) != Some("wrb.fr") {
                continue;
            }
            let Some(index) = translation
                .last()
                .and_then(Value::as_str)
                .and_then(|value| usize::from_str_radix(value, 36).ok())
            else {
                continue;
            };
            let Some(encoded) = translation
                .get(2)
                .and_then(Value::as_str)
                .and_then(|payload| serde_json::from_str::<Value>(payload).ok())
                .and_then(|payload| payload.get(0).and_then(Value::as_str).map(str::to_string))
            else {
                continue;
            };
            if let Some(result) = results.get_mut(index) {
                *result = Some(encoded);
            }
        }
    }

    results
        .into_iter()
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| {
            DomainError::InternalError(
                "Google Translate TTS response did not include every audio segment".to_string(),
            )
        })
}

fn radix36(mut value: usize) -> String {
    if value == 0 {
        return "0".to_string();
    }
    let mut digits = Vec::new();
    while value > 0 {
        let digit = (value % 36) as u8;
        digits.push(if digit < 10 {
            b'0' + digit
        } else {
            b'a' + digit - 10
        });
        value /= 36;
    }
    digits.reverse();
    String::from_utf8(digits).expect("base36 digits are ASCII")
}

pub(super) async fn handle_gemini(
    client: reqwest::Client,
    request: GoogleGeminiTtsRequest,
) -> Result<TtsRouteResponse, DomainError> {
    match request {
        GoogleGeminiTtsRequest::ListVoices => {
            let voices = GOOGLE_NATIVE_VOICES
                .iter()
                .map(|(name, description)| {
                    json!({
                        "name": name,
                        "voice_id": name,
                        "lang": "en-US",
                        "description": description,
                    })
                })
                .collect::<Vec<_>>();
            Ok(TtsRouteResponse::bytes(
                200,
                "application/json; charset=utf-8",
                serde_json::to_vec(&json!({ "voices": voices })).map_err(|error| {
                    DomainError::InternalError(format!(
                        "Google TTS voice list response encode failed: {error}"
                    ))
                })?,
            ))
        }
        GoogleGeminiTtsRequest::Generate {
            text,
            voice,
            model,
            base_url,
            api_key,
        } => generate_gemini(client, text, voice, model, base_url, api_key).await,
    }
}

async fn generate_gemini(
    client: reqwest::Client,
    text: String,
    voice: String,
    model: String,
    base_url: String,
    api_key: String,
) -> Result<TtsRouteResponse, DomainError> {
    let payload = json!({
        "contents": [{
            "role": "user",
            "parts": [{ "text": text }],
        }],
        "generationConfig": {
            "responseModalities": ["AUDIO"],
            "speechConfig": {
                "voiceConfig": {
                    "prebuiltVoiceConfig": {
                        "voiceName": voice,
                    },
                },
            },
        },
        "safetySettings": safety_settings(),
    });

    let url = append_google_api_path(
        &base_url,
        "v1beta",
        &format!("models/{model}:generateContent"),
    );
    let response = send_with_retry("Google TTS request", || {
        client
            .post(&url)
            .header(ACCEPT, "application/json")
            .header(CONTENT_TYPE, "application/json")
            .header("x-goog-api-key", &api_key)
            .json(&payload)
    })
    .await?;
    if !response.status().is_success() {
        return gemini_error_response(response).await;
    }
    let payload: Value = response.json().await.map_err(|error| {
        DomainError::InternalError(format!("Google TTS response read failed: {error}"))
    })?;
    let inline_data = payload
        .get("candidates")
        .and_then(|value| value.get(0))
        .and_then(|value| value.get("content"))
        .and_then(|value| value.get("parts"))
        .and_then(|value| value.get(0))
        .and_then(|value| value.get("inlineData"))
        .ok_or_else(|| {
            DomainError::InternalError("Google TTS response did not include audio data".to_string())
        })?;
    let encoded = inline_data
        .get("data")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            DomainError::InternalError("Google TTS response did not include audio data".to_string())
        })?;
    let mime_type = inline_data
        .get("mimeType")
        .and_then(Value::as_str)
        .unwrap_or("application/octet-stream");
    let audio = BASE64_STANDARD.decode(encoded).map_err(|error| {
        DomainError::InternalError(format!("Google TTS audio is not valid base64: {error}"))
    })?;
    if mime_type.to_ascii_lowercase().contains("audio/l16") {
        let sample_rate = mime_type
            .split(';')
            .find_map(|part| part.trim().strip_prefix("rate="))
            .and_then(|rate| rate.parse::<u32>().ok())
            .unwrap_or(24_000);
        return Ok(TtsRouteResponse::bytes(
            200,
            "audio/wav",
            wav_from_pcm(&audio, sample_rate),
        ));
    }
    Ok(TtsRouteResponse::bytes(200, mime_type, audio))
}

async fn gemini_error_response(
    response: reqwest::Response,
) -> Result<TtsRouteResponse, DomainError> {
    let status = response.status().as_u16();
    let bytes = response.bytes().await.map_err(|error| {
        DomainError::InternalError(format!("Google TTS error response read failed: {error}"))
    })?;
    Ok(TtsRouteResponse::json_error(
        status,
        parse_upstream_error_message(&bytes, "Google TTS request failed"),
    ))
}

fn safety_settings() -> Vec<Value> {
    [
        "HARM_CATEGORY_HARASSMENT",
        "HARM_CATEGORY_HATE_SPEECH",
        "HARM_CATEGORY_SEXUALLY_EXPLICIT",
        "HARM_CATEGORY_DANGEROUS_CONTENT",
        "HARM_CATEGORY_CIVIC_INTEGRITY",
    ]
    .into_iter()
    .map(|category| json!({ "category": category, "threshold": "OFF" }))
    .collect()
}

fn wav_from_pcm(pcm: &[u8], sample_rate: u32) -> Vec<u8> {
    let data_size = pcm.len() as u32;
    let mut wav = Vec::with_capacity(44 + pcm.len());
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_size).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    wav.extend_from_slice(&2_u16.to_le_bytes());
    wav.extend_from_slice(&16_u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_size.to_le_bytes());
    wav.extend_from_slice(pcm);
    wav
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{parse_translate_audio, radix36, wav_from_pcm};

    #[test]
    fn parses_google_translate_batch_audio_in_input_order() {
        let line = json!([
            [
                "wrb.fr",
                "jQ1olc",
                json!(["Ag=="]).to_string(),
                null,
                null,
                null,
                "1"
            ],
            [
                "wrb.fr",
                "jQ1olc",
                json!(["AQ=="]).to_string(),
                null,
                null,
                null,
                "0"
            ],
        ]);
        let body = format!(")]}}'\n\n{line}\n");
        assert_eq!(parse_translate_audio(&body, 2).unwrap(), ["AQ==", "Ag=="]);
        assert_eq!(radix36(35), "z");
        assert_eq!(radix36(36), "10");
    }

    #[test]
    fn wraps_google_l16_pcm_in_a_wav_header() {
        let wav = wav_from_pcm(&[1, 2, 3, 4], 24_000);
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 24_000);
        assert_eq!(&wav[44..], [1, 2, 3, 4]);
    }
}
