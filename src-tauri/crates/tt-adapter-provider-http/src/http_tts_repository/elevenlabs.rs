use reqwest::header::{ACCEPT, CONTENT_TYPE, HeaderMap, HeaderValue};
use reqwest::multipart::{Form, Part};

use super::{bytes_response, send_with_retry};
use crate::endpoint_url::append_endpoint_segments;
use tt_domain::errors::DomainError;
use tt_ports::repositories::tts_repository::{
    ElevenLabsTtsRequest, ElevenLabsVoiceFile, TtsRouteResponse,
};

const ELEVENLABS_API: &str = "https://api.elevenlabs.io/v1";

pub(super) async fn handle(
    client: reqwest::Client,
    request: ElevenLabsTtsRequest,
) -> Result<TtsRouteResponse, DomainError> {
    match request {
        ElevenLabsTtsRequest::Voices { api_key } => {
            get_json(client, api_key, "voices", "ElevenLabs voice list request").await
        }
        ElevenLabsTtsRequest::VoiceSettings { api_key } => {
            get_json(
                client,
                api_key,
                "voices/settings/default",
                "ElevenLabs voice settings request",
            )
            .await
        }
        ElevenLabsTtsRequest::Synthesize {
            api_key,
            voice_id,
            request,
        } => {
            let url = append_endpoint_segments(ELEVENLABS_API, &["text-to-speech", &voice_id])?;
            let response = send_with_retry("ElevenLabs synthesis request", || {
                client
                    .post(url.clone())
                    .header("xi-api-key", &api_key)
                    .header(ACCEPT, "*/*")
                    .header(CONTENT_TYPE, "application/json")
                    .json(&request)
            })
            .await?;
            bytes_response(response, "ElevenLabs synthesis request", "audio/mpeg", true).await
        }
        ElevenLabsTtsRequest::History { api_key } => {
            get_json(client, api_key, "history", "ElevenLabs history request").await
        }
        ElevenLabsTtsRequest::HistoryAudio {
            api_key,
            history_item_id,
        } => {
            let url =
                append_endpoint_segments(ELEVENLABS_API, &["history", &history_item_id, "audio"])?;
            let response = send_with_retry("ElevenLabs history audio request", || {
                client
                    .get(url.clone())
                    .header("xi-api-key", &api_key)
                    .header(ACCEPT, "*/*")
            })
            .await?;
            bytes_response(
                response,
                "ElevenLabs history audio request",
                "audio/mpeg",
                true,
            )
            .await
        }
        ElevenLabsTtsRequest::AddVoice {
            api_key,
            name,
            description,
            labels,
            files,
        } => add_voice(client, api_key, name, description, labels, files).await,
    }
}

async fn get_json(
    client: reqwest::Client,
    api_key: String,
    path: &str,
    label: &str,
) -> Result<TtsRouteResponse, DomainError> {
    let url = format!("{ELEVENLABS_API}/{path}");
    let response = send_with_retry(label, || {
        client
            .get(&url)
            .header("xi-api-key", &api_key)
            .header(ACCEPT, "application/json")
    })
    .await?;
    bytes_response(response, label, "application/json; charset=utf-8", false).await
}

async fn add_voice(
    client: reqwest::Client,
    api_key: String,
    name: String,
    description: String,
    labels: String,
    files: Vec<ElevenLabsVoiceFile>,
) -> Result<TtsRouteResponse, DomainError> {
    let files = prepare_files(files)?;
    let response = send_with_retry("ElevenLabs voice upload request", || {
        let form = files.iter().enumerate().fold(
            Form::new()
                .text("name", name.clone())
                .text("description", description.clone())
                .text("labels", labels.clone()),
            |form, (index, file)| {
                let mut headers = HeaderMap::new();
                headers.insert(CONTENT_TYPE, file.content_type.clone());
                form.part(
                    "files",
                    Part::bytes(file.bytes.clone())
                        .file_name(format!("audio-{index}.{}", file.extension))
                        .headers(headers),
                )
            },
        );
        client
            .post(format!("{ELEVENLABS_API}/voices/add"))
            .header("xi-api-key", &api_key)
            .header(ACCEPT, "application/json")
            .multipart(form)
    })
    .await?;

    bytes_response(
        response,
        "ElevenLabs voice upload request",
        "application/json; charset=utf-8",
        false,
    )
    .await
}

struct PreparedFile {
    content_type: HeaderValue,
    extension: String,
    bytes: Vec<u8>,
}

fn prepare_files(files: Vec<ElevenLabsVoiceFile>) -> Result<Vec<PreparedFile>, DomainError> {
    files
        .into_iter()
        .map(|file| {
            let content_type = HeaderValue::from_str(&file.mime_type).map_err(|error| {
                DomainError::InvalidData(format!(
                    "ElevenLabs voice file MIME type is invalid: {error}"
                ))
            })?;
            let extension = match file.mime_type.as_str() {
                "audio/mpeg" => "mp3".to_string(),
                "audio/x-m4a" | "audio/mp4" => "m4a".to_string(),
                mime_type => mime_type
                    .split_once('/')
                    .map(|(_, subtype)| subtype.split('+').next().unwrap_or("wav"))
                    .unwrap_or("wav")
                    .to_string(),
            };
            Ok(PreparedFile {
                content_type,
                extension,
                bytes: file.bytes,
            })
        })
        .collect()
}
