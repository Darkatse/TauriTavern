use reqwest::header::ACCEPT;

use super::{bytes_response, send_with_retry};
use tt_domain::errors::DomainError;
use tt_ports::repositories::tts_repository::TtsRouteResponse;

const NOVELAI_TTS_URL: &str = "https://api.novelai.net/ai/generate-voice";

pub(super) async fn generate(
    client: reqwest::Client,
    api_key: String,
    text: String,
    voice: String,
) -> Result<TtsRouteResponse, DomainError> {
    let response = send_with_retry("NovelAI TTS request", || {
        client
            .get(NOVELAI_TTS_URL)
            .bearer_auth(&api_key)
            .header(ACCEPT, "audio/mpeg")
            .query(&[
                ("text", text.as_str()),
                ("voice", "-1"),
                ("seed", voice.as_str()),
                ("opus", "false"),
                ("version", "v2"),
            ])
    })
    .await?;

    bytes_response(response, "NovelAI TTS request", "audio/mpeg", false).await
}
