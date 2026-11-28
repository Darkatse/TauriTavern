//! Ollama streams its generate response as newline-delimited JSON; the page reads it as a text
//! completion stream.

use reqwest::{Response, StatusCode};
use serde_json::{Value, json};

use tt_domain::errors::DomainError;
use tt_ports::generation_stream::{GenerationCancelReceiver, GenerationStreamSender};
use tt_ports::repositories::text_completion_repository::TextCompletionType;

use crate::provider_response;

const NDJSON: &str = "application/x-ndjson";
const PROVIDER_DONE: &str = "[DONE]";

pub(super) async fn stream(
    response: Response,
    sender: GenerationStreamSender,
    cancel: GenerationCancelReceiver,
) -> Result<(), DomainError> {
    provider_response::consume_ndjson_until_cancelled(response, cancel, |line| {
        let _ = sender.send(stream_event(line)?);
        Ok(())
    })
    .await?;
    let _ = sender.send(PROVIDER_DONE.to_string());
    Ok(())
}

fn stream_event(line: &[u8]) -> Result<String, DomainError> {
    let event = provider_response::parse_upstream_json_body(
        TextCompletionType::Ollama.display_name(),
        "generate_stream",
        StatusCode::OK,
        NDJSON,
        line,
    )?;
    let text = event.get("response").and_then(Value::as_str).unwrap_or("");
    let thinking = event.get("thinking").and_then(Value::as_str).unwrap_or("");
    Ok(json!({ "choices": [{ "text": text, "thinking": thinking }] }).to_string())
}

#[cfg(test)]
mod tests {
    use tokio::sync::{mpsc, watch};

    use super::*;

    async fn read(chunks: &'static [&'static [u8]]) -> (Vec<String>, Result<(), DomainError>) {
        let body = futures_util::stream::iter(
            chunks
                .iter()
                .map(|chunk| Ok::<_, std::io::Error>(bytes::Bytes::from_static(chunk))),
        );
        let response = Response::from(http::Response::new(reqwest::Body::wrap_stream(body)));
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let (_stop, cancel) = watch::channel(false);

        let result = stream(response, sender, cancel).await;
        let mut events = Vec::new();
        while let Ok(event) = receiver.try_recv() {
            events.push(event);
        }
        (events, result)
    }

    #[tokio::test]
    async fn stream_rewrites_lines_ends_with_done_and_fails_on_an_error_line() {
        let (events, result) = read(&[
            b"{\"response\":\"\",\"thinking\":\"hm\",\"done\":false}\n{\"response\":\"Hi\",\"done\":false}\n",
            b"{\"response\":\"\",\"done\":true,\"eval_count\":3}",
        ])
        .await;
        result.unwrap();
        assert_eq!(
            events,
            [
                r#"{"choices":[{"text":"","thinking":"hm"}]}"#,
                r#"{"choices":[{"text":"Hi","thinking":""}]}"#,
                r#"{"choices":[{"text":"","thinking":""}]}"#,
                "[DONE]",
            ]
        );

        let (events, result) = read(&[
            b"{\"response\":\"Hi\",\"done\":false}\n{\"error\":\"model ran out of memory\"}\n",
        ])
        .await;
        assert!(matches!(
            result,
            Err(DomainError::InvalidData(message)) if message == "Ollama: model ran out of memory"
        ));
        assert_eq!(events, [r#"{"choices":[{"text":"Hi","thinking":""}]}"#]);
    }
}
