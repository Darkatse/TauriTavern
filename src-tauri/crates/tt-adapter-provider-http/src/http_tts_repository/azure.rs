use reqwest::header::{ACCEPT, CONTENT_TYPE};

use super::{bytes_response, send_with_retry};
use tt_domain::errors::DomainError;
use tt_ports::repositories::tts_repository::{AzureTtsRequest, TtsRouteResponse};

pub(super) async fn handle(
    client: reqwest::Client,
    request: AzureTtsRequest,
) -> Result<TtsRouteResponse, DomainError> {
    match request {
        AzureTtsRequest::List { api_key, region } => list(client, api_key, region).await,
        AzureTtsRequest::Generate {
            api_key,
            region,
            text,
            voice,
        } => generate(client, api_key, region, text, voice).await,
    }
}

async fn list(
    client: reqwest::Client,
    api_key: String,
    region: String,
) -> Result<TtsRouteResponse, DomainError> {
    let url = format!("https://{region}.tts.speech.microsoft.com/cognitiveservices/voices/list");
    let response = send_with_retry("Azure TTS voice list request", || {
        client
            .get(&url)
            .header("Ocp-Apim-Subscription-Key", &api_key)
            .header(ACCEPT, "application/json")
    })
    .await?;

    bytes_response(
        response,
        "Azure TTS voice list request",
        "application/json; charset=utf-8",
        false,
    )
    .await
}

async fn generate(
    client: reqwest::Client,
    api_key: String,
    region: String,
    text: String,
    voice: String,
) -> Result<TtsRouteResponse, DomainError> {
    let url = format!("https://{region}.tts.speech.microsoft.com/cognitiveservices/v1");
    let language = voice.split('-').take(2).collect::<Vec<_>>().join("-");
    let ssml = format!(
        "<speak version='1.0' xmlns='http://www.w3.org/2001/10/synthesis' xml:lang='{}'><voice xml:lang='{}' name='{}'>{}</voice></speak>",
        escape_xml(&language),
        escape_xml(&language),
        escape_xml(&voice),
        escape_xml(&text),
    );
    let response = send_with_retry("Azure TTS request", || {
        client
            .post(&url)
            .header("Ocp-Apim-Subscription-Key", &api_key)
            .header(CONTENT_TYPE, "application/ssml+xml")
            .header("X-Microsoft-OutputFormat", "webm-24khz-16bit-mono-opus")
            .body(ssml.clone())
    })
    .await?;

    bytes_response(response, "Azure TTS request", "audio/ogg", false).await
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\'', "&apos;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::escape_xml;

    #[test]
    fn escapes_ssml_text_and_attributes() {
        assert_eq!(
            escape_xml("A&B <voice name='x'>"),
            "A&amp;B &lt;voice name=&apos;x&apos;&gt;"
        );
    }
}
