//! Responses of model provider APIs: error statuses, JSON bodies and streamed events.

use reqwest::{RequestBuilder, StatusCode};
use serde_json::Value;

use tt_adapter_http::upstream;
use tt_domain::errors::DomainError;

mod body;
mod context_overflow;
mod stream;

pub(crate) use body::{
    log_upstream_body_parse_failure, parse_upstream_json_body, provider_error,
    provider_error_message, read_upstream_json_body,
};
use context_overflow::context_overflow_error;
pub(crate) use context_overflow::context_overflow_or;
pub(crate) use stream::{
    consume_ndjson_until_cancelled, consume_sse_response, consume_sse_until_cancelled, send_event,
    stream_sse_response,
};

pub(crate) async fn send_checked(
    request: RequestBuilder,
    provider_name: &str,
    error_context: &str,
) -> Result<reqwest::Response, DomainError> {
    let response = request.send().await.map_err(upstream::request_failure)?;

    if !response.status().is_success() {
        return Err(map_error_response(provider_name, response, error_context).await);
    }

    Ok(response)
}

pub(crate) async fn map_error_response(
    provider_name: &str,
    response: reqwest::Response,
    default_message: &str,
) -> DomainError {
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    map_error_status(provider_name, status, &body, default_message)
}

fn map_error_status(
    provider_name: &str,
    status: StatusCode,
    body: &str,
    default_message: &str,
) -> DomainError {
    let body = body.trim();
    let parsed = serde_json::from_str::<Value>(body).ok();
    let error = parsed.as_ref().and_then(provider_error);
    let message = parsed
        .as_ref()
        .and_then(|value| match error {
            Some(error) => Some(provider_error_message(error, value)),
            None => value
                .get("message")
                .and_then(Value::as_str)
                .map(str::to_owned),
        })
        .unwrap_or_else(|| {
            if body.is_empty() {
                default_message
            } else {
                body
            }
            .to_string()
        });

    let status_error = |message: String| match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
            DomainError::AuthenticationError(message)
        }
        StatusCode::BAD_REQUEST => DomainError::InvalidData(message),
        StatusCode::TOO_MANY_REQUESTS => DomainError::rate_limited(format!(
            "{provider_name} endpoint failed with status {}: {message}",
            status.as_u16()
        )),
        status if is_retryable_status(status) => DomainError::transient(format!(
            "{provider_name} endpoint failed with status {}: {message}",
            status.as_u16()
        )),
        _ => DomainError::InternalError(format!(
            "{provider_name} endpoint failed with status {}: {message}",
            status.as_u16()
        )),
    };

    match status {
        StatusCode::PAYLOAD_TOO_LARGE => context_overflow_error(message),
        StatusCode::BAD_REQUEST => {
            let error = error.map_or_else(|| body.to_owned(), Value::to_string);
            context_overflow_or(error, message, status_error)
        }
        _ => status_error(message),
    }
}

fn is_retryable_status(status: StatusCode) -> bool {
    matches!(status.as_u16(), 408 | 425 | 429 | 500 | 502 | 503 | 504)
}

#[cfg(test)]
mod tests {
    use tt_domain::errors::DomainError;

    use super::map_error_status;

    #[test]
    fn error_status_classification_preserves_recovery_semantics() {
        for (status, body, overflow) in [
            (
                400,
                "prompt is too long: 213462 tokens > 200000 maximum",
                true,
            ),
            (
                400,
                "input length and `max_tokens` exceed context limit: 190000 + 32768 > 200000",
                true,
            ),
            (
                400,
                "Your input exceeds the context window of this model",
                true,
            ),
            (
                400,
                "This model's maximum context length is 131072 tokens. However, you requested 140000 tokens",
                true,
            ),
            (
                400,
                "The input token count (1196265) exceeds the maximum number of tokens allowed (1048575)",
                true,
            ),
            (
                400,
                r#"{"error":{"code":"context_length_exceeded","message":"invalid request"}}"#,
                true,
            ),
            (400, r#"{"message":"prompt is too long"}"#, true),
            (413, "<html>Request Entity Too Large</html>", true),
            (
                400,
                "Request payload size exceeds the limit: 20971520 bytes.",
                true,
            ),
            (400, "Invalid value for temperature", false),
            (400, "rate limit: too many tokens", false),
            (
                429,
                "ThrottlingException: Too many tokens, please wait before trying again.",
                false,
            ),
            (502, "upstream unavailable", false),
        ] {
            let error = map_error_status(
                "Provider",
                reqwest::StatusCode::from_u16(status).unwrap(),
                body,
                "Generation request failed",
            );
            assert_eq!(
                matches!(&error, DomainError::InvalidData(message) if message.starts_with("model.context_overflow:")),
                overflow,
                "{status}: {body}: {error}"
            );
            match status {
                429 => assert!(matches!(error, DomainError::RateLimited { .. })),
                502 => assert!(matches!(error, DomainError::Transient(_))),
                _ => assert!(matches!(error, DomainError::InvalidData(_))),
            }
        }
    }
}
