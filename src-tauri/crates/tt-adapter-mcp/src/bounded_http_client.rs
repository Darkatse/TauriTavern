use std::{borrow::Cow, collections::HashMap, sync::Arc};

use futures_util::{StreamExt, stream::BoxStream};
use http::{HeaderName, HeaderValue};
use reqwest::header::{ACCEPT, CONTENT_TYPE, WWW_AUTHENTICATE};
use rmcp::{
    model::{ClientJsonRpcMessage, JsonRpcMessage, ServerJsonRpcMessage},
    transport::{
        common::http_header::{
            EVENT_STREAM_MIME_TYPE, HEADER_LAST_EVENT_ID, HEADER_SESSION_ID, JSON_MIME_TYPE,
        },
        streamable_http_client::{
            AuthRequiredError, InsufficientScopeError, StreamableHttpClient, StreamableHttpError,
            StreamableHttpPostResponse,
        },
    },
};
use sse_stream::{Error as SseError, Sse, SseStream};
use thiserror::Error;
use tokio_util::sync::CancellationToken;

pub(crate) const MAX_HTTP_RESPONSE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone)]
pub(crate) struct BoundedReqwestClient {
    inner: reqwest::Client,
    max_json_body_size: usize,
    cancel: CancellationToken,
}

impl BoundedReqwestClient {
    pub(crate) fn new(
        inner: reqwest::Client,
        max_json_body_size: usize,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            inner,
            max_json_body_size,
            cancel,
        }
    }

    async fn post_message_uncancelled(
        &self,
        uri: Arc<str>,
        message: ClientJsonRpcMessage,
        session_id: Option<Arc<str>>,
        auth_header: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
        max_sse_event_size: usize,
    ) -> Result<StreamableHttpPostResponse, StreamableHttpError<reqwest::Error>> {
        let mut request = self
            .inner
            .post(uri.as_ref())
            .header(ACCEPT, [EVENT_STREAM_MIME_TYPE, JSON_MIME_TYPE].join(", "));
        if let Some(auth_header) = auth_header {
            request = request.bearer_auth(auth_header);
        }
        request = apply_custom_headers(request, custom_headers)?;

        let session_was_attached = session_id.is_some();
        if let Some(session_id) = session_id {
            request = request.header(HEADER_SESSION_ID, session_id.as_ref());
        }
        let response = request
            .json(&message)
            .send()
            .await
            .map_err(StreamableHttpError::Client)?;
        let status = response.status();

        if status == reqwest::StatusCode::UNAUTHORIZED
            && let Some(header) = response.headers().get(WWW_AUTHENTICATE)
        {
            let header = header.to_str().map_err(|_| {
                StreamableHttpError::UnexpectedServerResponse(
                    "invalid www-authenticate header value".into(),
                )
            })?;
            return Err(StreamableHttpError::AuthRequired(AuthRequiredError::new(
                header.to_string(),
            )));
        }
        if status == reqwest::StatusCode::FORBIDDEN
            && let Some(header) = response.headers().get(WWW_AUTHENTICATE)
        {
            let header = header.to_str().map_err(|_| {
                StreamableHttpError::UnexpectedServerResponse(
                    "invalid www-authenticate header value".into(),
                )
            })?;
            return Err(StreamableHttpError::InsufficientScope(
                InsufficientScopeError::new(header.to_string(), None),
            ));
        }
        if matches!(
            status,
            reqwest::StatusCode::ACCEPTED | reqwest::StatusCode::NO_CONTENT
        ) {
            return Ok(StreamableHttpPostResponse::Accepted);
        }
        if status == reqwest::StatusCode::NOT_FOUND && session_was_attached {
            return Err(StreamableHttpError::SessionExpired);
        }

        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .map(|value| String::from_utf8_lossy(value.as_bytes()).into_owned());
        let content_length = response.content_length();
        let response_session_id = response
            .headers()
            .get(HEADER_SESSION_ID)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);

        if status.is_success() && content_length == Some(0) && accepts_empty_response(&message) {
            return Ok(StreamableHttpPostResponse::Accepted);
        }

        if !status.is_success() {
            let body = read_bounded_body(response, self.max_json_body_size).await?;
            if is_content_type(&content_type, JSON_MIME_TYPE)
                && let Ok(message @ JsonRpcMessage::Error(_)) =
                    serde_json::from_slice::<ServerJsonRpcMessage>(&body)
            {
                return Ok(StreamableHttpPostResponse::Json(
                    message,
                    response_session_id,
                ));
            }
            let preview = String::from_utf8_lossy(&body)
                .chars()
                .take(4096)
                .collect::<String>();
            return Err(StreamableHttpError::UnexpectedServerResponse(Cow::Owned(
                format!("HTTP {status}: {preview}"),
            )));
        }

        if is_content_type(&content_type, EVENT_STREAM_MIME_TYPE) {
            let stream = bounded_sse_body(response, max_sse_event_size);
            return Ok(StreamableHttpPostResponse::Sse(stream, response_session_id));
        }
        if is_content_type(&content_type, JSON_MIME_TYPE) {
            let body = read_bounded_body(response, self.max_json_body_size).await?;
            if body.is_empty() && accepts_empty_response(&message) {
                return Ok(StreamableHttpPostResponse::Accepted);
            }
            let message =
                serde_json::from_slice::<ServerJsonRpcMessage>(&body).map_err(|error| {
                    StreamableHttpError::UnexpectedServerResponse(Cow::Owned(format!(
                        "invalid JSON-RPC response: {error}"
                    )))
                })?;
            return Ok(StreamableHttpPostResponse::Json(
                message,
                response_session_id,
            ));
        }

        Err(StreamableHttpError::UnexpectedContentType(content_type))
    }
}

#[derive(Debug, Error)]
enum BoundedSseBodyError {
    #[error(transparent)]
    Source(reqwest::Error),
    #[error("MCP SSE response exceeded {max_size} bytes")]
    TooLarge { max_size: usize },
}

impl StreamableHttpClient for BoundedReqwestClient {
    type Error = reqwest::Error;

    async fn get_stream(
        &self,
        uri: Arc<str>,
        session_id: Option<Arc<str>>,
        last_event_id: Option<String>,
        auth_header: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<BoxStream<'static, Result<Sse, SseError>>, StreamableHttpError<Self::Error>> {
        <reqwest::Client as StreamableHttpClient>::get_stream(
            &self.inner,
            uri,
            session_id,
            last_event_id,
            auth_header,
            custom_headers,
        )
        .await
    }

    async fn get_stream_with_max_sse_event_size(
        &self,
        uri: Arc<str>,
        session_id: Option<Arc<str>>,
        last_event_id: Option<String>,
        auth_header: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
        max_sse_event_size: usize,
    ) -> Result<BoxStream<'static, Result<Sse, SseError>>, StreamableHttpError<Self::Error>> {
        <reqwest::Client as StreamableHttpClient>::get_stream_with_max_sse_event_size(
            &self.inner,
            uri,
            session_id,
            last_event_id,
            auth_header,
            custom_headers,
            max_sse_event_size,
        )
        .await
    }

    async fn delete_session(
        &self,
        uri: Arc<str>,
        session_id: Arc<str>,
        auth_header: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<(), StreamableHttpError<Self::Error>> {
        <reqwest::Client as StreamableHttpClient>::delete_session(
            &self.inner,
            uri,
            session_id,
            auth_header,
            custom_headers,
        )
        .await
    }

    async fn post_message(
        &self,
        uri: Arc<str>,
        message: ClientJsonRpcMessage,
        session_id: Option<Arc<str>>,
        auth_header: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<StreamableHttpPostResponse, StreamableHttpError<Self::Error>> {
        self.post_message_with_max_sse_event_size(
            uri,
            message,
            session_id,
            auth_header,
            custom_headers,
            MAX_HTTP_RESPONSE_BYTES,
        )
        .await
    }

    async fn post_message_with_max_sse_event_size(
        &self,
        uri: Arc<str>,
        message: ClientJsonRpcMessage,
        session_id: Option<Arc<str>>,
        auth_header: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
        max_sse_event_size: usize,
    ) -> Result<StreamableHttpPostResponse, StreamableHttpError<Self::Error>> {
        tokio::select! {
            biased;
            response = self.post_message_uncancelled(
                uri,
                message,
                session_id,
                auth_header,
                custom_headers,
                max_sse_event_size,
            ) => response,
            _ = self.cancel.cancelled() => Err(StreamableHttpError::UnexpectedServerResponse(
                "MCP request cancelled".into(),
            )),
        }
    }
}

fn apply_custom_headers(
    mut request: reqwest::RequestBuilder,
    custom_headers: HashMap<HeaderName, HeaderValue>,
) -> Result<reqwest::RequestBuilder, StreamableHttpError<reqwest::Error>> {
    for (name, value) in custom_headers {
        let reserved = name.as_str().eq_ignore_ascii_case(ACCEPT.as_str())
            || name.as_str().eq_ignore_ascii_case(HEADER_SESSION_ID)
            || name.as_str().eq_ignore_ascii_case(HEADER_LAST_EVENT_ID);
        if reserved {
            return Err(StreamableHttpError::ReservedHeaderConflict(
                name.to_string(),
            ));
        }
        request = request.header(name, value);
    }
    Ok(request)
}

fn accepts_empty_response(message: &ClientJsonRpcMessage) -> bool {
    matches!(
        message,
        ClientJsonRpcMessage::Notification(_)
            | ClientJsonRpcMessage::Response(_)
            | ClientJsonRpcMessage::Error(_)
    )
}

fn is_content_type(actual: &Option<String>, expected: &str) -> bool {
    actual
        .as_deref()
        .is_some_and(|value| value.as_bytes().starts_with(expected.as_bytes()))
}

async fn read_bounded_body(
    response: reqwest::Response,
    max_size: usize,
) -> Result<Vec<u8>, StreamableHttpError<reqwest::Error>> {
    if response
        .content_length()
        .is_some_and(|size| size > max_size as u64)
    {
        return Err(response_too_large(max_size));
    }

    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(StreamableHttpError::Client)?;
        if bytes.len().saturating_add(chunk.len()) > max_size {
            return Err(response_too_large(max_size));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn response_too_large(max_size: usize) -> StreamableHttpError<reqwest::Error> {
    StreamableHttpError::UnexpectedServerResponse(Cow::Owned(format!(
        "MCP HTTP response exceeded {max_size} bytes"
    )))
}

fn bounded_sse_body(
    response: reqwest::Response,
    max_size: usize,
) -> BoxStream<'static, Result<Sse, SseError>> {
    let mut received = 0usize;
    let bytes = response.bytes_stream().map(move |chunk| match chunk {
        Ok(chunk) => {
            received = received.saturating_add(chunk.len());
            if received > max_size {
                Err(BoundedSseBodyError::TooLarge { max_size })
            } else {
                Ok(chunk)
            }
        }
        Err(error) => Err(BoundedSseBodyError::Source(error)),
    });
    SseStream::from_bytes_stream(bytes).boxed()
}

#[cfg(test)]
mod tests {
    use futures_util::stream;
    use serde_json::json;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    use super::*;

    #[tokio::test]
    async fn malformed_nonempty_json_is_not_treated_as_accepted() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0; 4096];
            assert!(stream.read(&mut request).await.unwrap() > 0);
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 8\r\nConnection: close\r\n\r\nnot-json",
                )
                .await
                .unwrap();
        });
        let message = serde_json::from_value(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/list"
        }))
        .unwrap();

        let error = BoundedReqwestClient::new(reqwest::Client::new(), 64, CancellationToken::new())
            .post_message(Arc::from(endpoint), message, None, None, HashMap::new())
            .await
            .unwrap_err();
        server.await.unwrap();

        assert!(error.to_string().contains("invalid JSON-RPC response"));
    }

    #[tokio::test]
    async fn chunked_body_limit_is_enforced_without_content_length() {
        let body = reqwest::Body::wrap_stream(stream::iter([
            Ok::<_, std::io::Error>(vec![0u8; 5]),
            Ok(vec![0u8; 5]),
        ]));
        let response =
            reqwest::Response::from(http::Response::builder().status(200).body(body).unwrap());
        let error = read_bounded_body(response, 8).await.unwrap_err();

        assert!(error.to_string().contains("exceeded 8 bytes"));
    }

    #[tokio::test]
    async fn sse_response_limit_is_enforced_before_parsing_an_oversized_event() {
        let response = reqwest::Response::from(
            http::Response::builder()
                .status(200)
                .body(reqwest::Body::from("data: 123456789\n\n"))
                .unwrap(),
        );
        let mut events = bounded_sse_body(response, 8);

        assert!(events.next().await.unwrap().is_err());
    }
}
