use std::fmt::Display;

use reqwest::Response;
use reqwest::StatusCode;
use reqwest::header::CONTENT_TYPE;
use serde_json::Value;

use tt_domain::errors::DomainError;

/// Maximum number of bytes from a non-JSON upstream body to capture in the
/// diagnostic log. Anything larger is truncated; the full `body_len` is still
/// emitted so log readers know how much was elided.
pub(super) const BODY_PREVIEW_BYTES: usize = 512;

/// Emit a structured `error!` event describing why a chat-completion upstream
/// body could not be parsed as JSON. Always truncates the body to
/// [`BODY_PREVIEW_BYTES`] so we don't blow up the log pipeline on huge HTML
/// challenges, plain-text errors, or runaway bodies.
pub(super) fn log_upstream_body_parse_failure(
    provider_name: &str,
    operation: &str,
    status: StatusCode,
    content_type: &str,
    body: &[u8],
    error: &impl Display,
) {
    let body_len = body.len();
    let body_preview = body_preview_string(body);

    tracing::error!(
        provider = provider_name,
        operation = operation,
        status = status.as_u16(),
        content_type = content_type,
        body_len,
        body_preview = %body_preview,
        error = %error,
        "upstream returned non-JSON body for chat completion",
    );
}

/// Lossy UTF-8 preview of the first [`BODY_PREVIEW_BYTES`] of a body.
pub(super) fn body_preview_string(body: &[u8]) -> String {
    let preview_bytes = &body[..body.len().min(BODY_PREVIEW_BYTES)];
    String::from_utf8_lossy(preview_bytes).into_owned()
}

/// Read an upstream HTTP response body and parse it as JSON, logging a
/// detailed diagnostic event on failure. Callers should classify explicit HTTP
/// error statuses before invoking this helper; this path is for responses whose
/// body was expected to carry the provider JSON contract.
///
/// Both body-read failures and JSON-decode failures are reported as
/// [`DomainError::Transient`] tagged with `model.upstream_invalid_response`,
/// because in practice they are caused by upstream-side hiccups (CDN
/// challenges, proxy timeouts mid-body, plain-text 200 errors) that are worth
/// retrying rather than tearing the run down as a fatal `agent.internal_error`.
pub(super) async fn read_upstream_json_body(
    provider_name: &str,
    operation: &str,
    response: Response,
) -> Result<Value, DomainError> {
    let status = response.status();
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();
    let endpoint = response.url().clone();

    let body = response.bytes().await.map_err(|error| {
        let failure = crate::http_error::reqwest_body_failure(&error, Some(&endpoint));
        tracing::warn!(
            provider = provider_name,
            operation = operation,
            status = status.as_u16(),
            code = %failure.code,
            category = %failure.category,
            endpoint = failure.endpoint.as_deref().unwrap_or(""),
            timeout = error.is_timeout(),
            connect = error.is_connect(),
            body = error.is_body(),
            request = error.is_request(),
            "upstream response body read failed",
        );
        DomainError::upstream_failure(failure)
    })?;

    parse_upstream_json_body(
        provider_name,
        operation,
        status,
        &content_type,
        body.as_ref(),
    )
}

pub(super) fn parse_upstream_json_body(
    provider_name: &str,
    operation: &str,
    status: StatusCode,
    content_type: &str,
    body: &[u8],
) -> Result<Value, DomainError> {
    match serde_json::from_slice::<Value>(body) {
        Ok(value) => Ok(value),
        Err(error) => {
            log_upstream_body_parse_failure(
                provider_name,
                operation,
                status,
                content_type,
                body,
                &error,
            );
            Err(DomainError::transient(format!(
                "model.upstream_invalid_response: {provider_name} returned status {} non-JSON body ({operation}): {error}",
                status.as_u16()
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_preview_string_truncates_to_first_512_bytes() {
        let body = vec![b'a'; BODY_PREVIEW_BYTES + 64];
        let preview = body_preview_string(&body);
        assert_eq!(preview.len(), BODY_PREVIEW_BYTES);
        assert!(preview.chars().all(|character| character == 'a'));
    }

    #[test]
    fn body_preview_string_returns_short_body_verbatim() {
        let body = b"<html>error</html>";
        assert_eq!(body_preview_string(body), "<html>error</html>");
    }

    #[test]
    fn body_preview_string_replaces_invalid_utf8_lossily() {
        let body = vec![0xff, 0xfe, b'O', b'K'];
        let preview = body_preview_string(&body);
        assert!(preview.contains("OK"));
    }

    #[test]
    fn parse_upstream_json_body_returns_transient_invalid_response_for_non_json_body() {
        let error = parse_upstream_json_body(
            "OpenAI",
            "generate",
            StatusCode::OK,
            "text/html",
            b"<html>gateway challenge</html>",
        )
        .expect_err("non-json body should be transient");

        assert!(matches!(error, DomainError::Transient(_)));
        assert_eq!(
            error.to_string(),
            "model.upstream_invalid_response: OpenAI returned status 200 non-JSON body (generate): expected value at line 1 column 1"
        );
    }

    #[test]
    fn parse_upstream_json_body_accepts_valid_json_even_with_imprecise_content_type() {
        let value = parse_upstream_json_body(
            "OpenAI",
            "generate",
            StatusCode::OK,
            "text/plain",
            br#"{"ok":true}"#,
        )
        .expect("valid JSON should parse");

        assert_eq!(value["ok"], true);
    }
}
