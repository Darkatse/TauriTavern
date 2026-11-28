//! Streamed responses of model provider APIs. Server-sent events yield each `data` payload;
//! newline-delimited JSON yields each line. Reading stops without error once `cancel` turns `true`.

use std::future::Future;

use reqwest::Response;

use tt_adapter_http::upstream;
use tt_domain::errors::DomainError;
use tt_ports::generation_stream::{GenerationCancelReceiver, GenerationStreamSender};

pub(crate) async fn stream_sse_response(
    provider_name: &str,
    response: Response,
    sender: GenerationStreamSender,
    cancel: GenerationCancelReceiver,
) -> Result<(), DomainError> {
    consume_sse_until_cancelled(provider_name, response, cancel, |payload| {
        send_event(&sender, payload)
    })
    .await
}

pub(crate) async fn consume_sse_until_cancelled<F>(
    provider_name: &str,
    response: Response,
    cancel: GenerationCancelReceiver,
    on_event: F,
) -> Result<(), DomainError>
where
    F: FnMut(&[u8]) -> Result<(), DomainError>,
{
    until_cancelled(
        consume_sse_response(provider_name, response, on_event),
        cancel,
    )
    .await
}

pub(crate) async fn consume_ndjson_until_cancelled<F>(
    response: Response,
    cancel: GenerationCancelReceiver,
    mut on_line: F,
) -> Result<(), DomainError>
where
    F: FnMut(&[u8]) -> Result<(), DomainError>,
{
    let consume = async {
        let mut on_json_line = |line: &[u8]| {
            if line.trim_ascii().is_empty() {
                return Ok(());
            }
            on_line(line)
        };
        let rest = consume_lines(response, &mut on_json_line).await?;
        // The last line may end without a line break.
        on_json_line(&rest)
    };
    until_cancelled(consume, cancel).await
}

pub(crate) async fn consume_sse_response<F>(
    provider_name: &str,
    response: Response,
    mut on_event: F,
) -> Result<(), DomainError>
where
    F: FnMut(&[u8]) -> Result<(), DomainError>,
{
    let endpoint = response.url().clone();
    let mut accumulator = SseEventAccumulator::default();
    let rest = consume_lines(response, &mut |line| {
        accumulator.on_line(line, &mut on_event)
    })
    .await?;

    // An event cut off by the end of the stream is never dispatched (SSE).
    let discarded_bytes = rest.len() + accumulator.data.len();
    if discarded_bytes > 0 {
        tracing::warn!(
            provider = provider_name,
            operation = "stream",
            endpoint = upstream::sanitize_endpoint(&endpoint),
            discarded_bytes,
            "upstream stream ended mid-event",
        );
    }
    Ok(())
}

pub(crate) fn send_event(
    sender: &GenerationStreamSender,
    payload: &[u8],
) -> Result<(), DomainError> {
    let payload = std::str::from_utf8(payload).map_err(|error| {
        DomainError::InternalError(format!("Stream payload is not valid UTF-8: {error}"))
    })?;
    let _ = sender.send(payload.to_string());
    Ok(())
}

async fn until_cancelled(
    consume: impl Future<Output = Result<(), DomainError>>,
    mut cancel: GenerationCancelReceiver,
) -> Result<(), DomainError> {
    tokio::select! {
        result = consume => result,
        Ok(_) = cancel.wait_for(|cancelled| *cancelled) => Ok(()),
    }
}

/// Calls `on_line` for every complete line of the body and returns the bytes after the last line
/// break.
async fn consume_lines(
    mut response: Response,
    on_line: &mut impl FnMut(&[u8]) -> Result<(), DomainError>,
) -> Result<Vec<u8>, DomainError> {
    let endpoint = response.url().clone();
    let mut buffer = Vec::<u8>::new();

    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| upstream::body_failure(error, &endpoint))?
    {
        buffer.extend_from_slice(&chunk);
        drain_lines(&mut buffer, on_line)?;
    }

    Ok(buffer)
}

/// Lines end with `\n` or `\r\n`; the line ending is not passed to `on_line`.
fn drain_lines(
    buffer: &mut Vec<u8>,
    on_line: &mut impl FnMut(&[u8]) -> Result<(), DomainError>,
) -> Result<(), DomainError> {
    let mut consumed = 0_usize;

    while let Some(offset) = buffer[consumed..].iter().position(|byte| *byte == b'\n') {
        let end = consumed + offset;
        let line = &buffer[consumed..end];
        on_line(line.strip_suffix(b"\r").unwrap_or(line))?;
        consumed = end + 1;
    }

    buffer.drain(..consumed);
    Ok(())
}

#[derive(Default)]
struct SseEventAccumulator {
    data: Vec<u8>,
}

impl SseEventAccumulator {
    fn on_line<F: FnMut(&[u8]) -> Result<(), DomainError>>(
        &mut self,
        line: &[u8],
        on_event: &mut F,
    ) -> Result<(), DomainError> {
        if line.is_empty() {
            return self.dispatch(on_event);
        }

        if line.first().is_some_and(|byte| *byte == b':') {
            return Ok(());
        }

        let (field, value) = split_sse_field(line);
        if field == b"data" {
            if !self.data.is_empty() {
                self.data.push(b'\n');
            }
            self.data.extend_from_slice(value);
        }

        Ok(())
    }

    fn dispatch<F: FnMut(&[u8]) -> Result<(), DomainError>>(
        &mut self,
        on_event: &mut F,
    ) -> Result<(), DomainError> {
        if self.data.is_empty() {
            return Ok(());
        }

        let payload = std::mem::take(&mut self.data);
        on_event(payload.as_slice())
    }
}

fn split_sse_field(line: &[u8]) -> (&[u8], &[u8]) {
    let Some(colon_index) = line.iter().position(|byte| *byte == b':') else {
        return (line, b"");
    };

    let field = &line[..colon_index];
    let mut value = &line[colon_index + 1..];
    let value_start = value
        .iter()
        .position(|byte| *byte != b' ' && *byte != b'\t')
        .unwrap_or(value.len());
    value = &value[value_start..];

    (field, value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sse_events(buffer: &mut Vec<u8>) -> Vec<Vec<u8>> {
        let mut events = Vec::<Vec<u8>>::new();
        let mut accumulator = SseEventAccumulator::default();
        let mut on_event = |event: &[u8]| {
            events.push(event.to_vec());
            Ok(())
        };
        drain_lines(buffer, &mut |line| accumulator.on_line(line, &mut on_event)).unwrap();
        events
    }

    /// A response whose body chunks are sent after it reaches the reader.
    fn delayed_body() -> (tokio::sync::mpsc::UnboundedSender<&'static [u8]>, Response) {
        let (chunks, received) = tokio::sync::mpsc::unbounded_channel::<&'static [u8]>();
        let body = futures_util::stream::unfold(received, |mut received| async move {
            let chunk = received.recv().await?;
            let chunk = Ok::<_, std::io::Error>(bytes::Bytes::from_static(chunk));
            Some((chunk, received))
        });
        let response = http::Response::new(reqwest::Body::wrap_stream(body));
        (chunks, Response::from(response))
    }

    #[test]
    fn sse_lines_yield_data_payloads() {
        let mut buffer =
            b"event: message\r\ndata: {\"chunk\":1}\n\n: ping\ndata: [DONE]\n\n".to_vec();

        assert_eq!(
            sse_events(&mut buffer),
            [b"{\"chunk\":1}".to_vec(), b"[DONE]".to_vec()]
        );
        assert!(buffer.is_empty());
    }

    #[test]
    fn sse_lines_keep_partial_line_in_buffer() {
        let mut buffer = b"data: {\"chunk\":1}".to_vec();

        assert!(sse_events(&mut buffer).is_empty());
        assert_eq!(buffer, b"data: {\"chunk\":1}".to_vec());
    }

    #[test]
    fn sse_lines_combine_multiline_data_fields() {
        let mut buffer = b"data: first\ndata: second\n\n".to_vec();

        assert_eq!(sse_events(&mut buffer), [b"first\nsecond".to_vec()]);
        assert!(buffer.is_empty());
    }

    #[tokio::test]
    async fn consume_sse_response_discards_an_event_cut_off_by_end_of_stream() {
        for body in ["data: a\n\ndata: b\n", "data: a\n\ndata: b"] {
            let response = Response::from(http::Response::new(body));
            let mut events = Vec::<Vec<u8>>::new();
            consume_sse_response("test", response, |event| {
                events.push(event.to_vec());
                Ok(())
            })
            .await
            .unwrap();

            assert_eq!(events, [b"a".to_vec()], "{body:?}");
        }
    }

    #[tokio::test]
    async fn sse_reading_waits_for_events_that_arrive_after_the_headers() {
        let (body, response) = delayed_body();
        let (_stop, cancel) = tokio::sync::watch::channel(false);
        let mut events = Vec::new();

        let read = consume_sse_until_cancelled("test", response, cancel, |event| {
            events.push(event.to_vec());
            Ok(())
        });
        let write = async move {
            tokio::task::yield_now().await;
            body.send(b"data: a\n\n").unwrap();
            body.send(b"data: b\n\n").unwrap();
        };
        let (result, ()) = tokio::join!(read, write);

        result.unwrap();
        assert_eq!(events, [b"a".to_vec(), b"b".to_vec()]);
    }

    #[tokio::test]
    async fn ndjson_yields_each_line_across_chunk_boundaries() {
        let (body, response) = delayed_body();
        let (_stop, cancel) = tokio::sync::watch::channel(false);
        let mut lines = Vec::new();

        let read = consume_ndjson_until_cancelled(response, cancel, |line| {
            lines.push(String::from_utf8(line.to_vec()).unwrap());
            Ok(())
        });
        let write = async move {
            // Split the 3-byte UTF-8 sequence for "好" (`e5 a5 bd`) across chunks.
            body.send(b"{\"a\":1}\n{\"b\":\"\xe5").unwrap();
            body.send(b"\xa5\xbd\"}\r\n\n{\"c\":3}").unwrap();
        };
        let (result, ()) = tokio::join!(read, write);

        result.unwrap();
        assert_eq!(lines, ["{\"a\":1}", "{\"b\":\"好\"}", "{\"c\":3}"]);
    }
}
