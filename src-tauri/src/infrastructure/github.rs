use reqwest::StatusCode;
use serde::Deserialize;

use crate::domain::errors::DomainError;

pub const GITHUB_RATE_LIMIT_MESSAGE: &str =
    "GitHub has rate-limited your requests. Please try again later, or change your network and try again.";

const GITHUB_RATE_LIMIT_TOKENS: [&str; 2] = ["rate limit", "abuse detection"];

#[derive(Debug, Deserialize)]
struct GithubApiErrorResponse {
    message: Option<String>,
}

fn extract_error_message(body: &str) -> String {
    serde_json::from_str::<GithubApiErrorResponse>(body)
        .ok()
        .and_then(|payload| payload.message)
        .unwrap_or_else(|| body.trim().to_string())
}

pub fn classify_github_rate_limit(status: StatusCode, body: &str) -> Option<DomainError> {
    if !matches!(status, StatusCode::FORBIDDEN | StatusCode::TOO_MANY_REQUESTS) {
        return None;
    }

    let message = extract_error_message(body);
    let normalized = message.to_ascii_lowercase();
    if !GITHUB_RATE_LIMIT_TOKENS
        .iter()
        .any(|token| normalized.contains(token))
    {
        return None;
    }

    Some(DomainError::rate_limited(GITHUB_RATE_LIMIT_MESSAGE))
}

#[cfg(test)]
mod tests {
    use super::{GITHUB_RATE_LIMIT_MESSAGE, classify_github_rate_limit};
    use crate::domain::errors::DomainError;
    use reqwest::StatusCode;

    #[test]
    fn classifies_primary_github_rate_limit_as_domain_rate_limit() {
        let classified = classify_github_rate_limit(
            StatusCode::FORBIDDEN,
            r#"{"message":"API rate limit exceeded for 127.0.0.1."}"#,
        );

        assert!(matches!(
            classified,
            Some(DomainError::RateLimited { message }) if message == GITHUB_RATE_LIMIT_MESSAGE
        ));
    }

    #[test]
    fn ignores_non_rate_limit_github_responses() {
        assert!(classify_github_rate_limit(
            StatusCode::FORBIDDEN,
            r#"{"message":"Repository access blocked"}"#,
        )
        .is_none());
    }

    #[test]
    fn classifies_github_abuse_detection_as_rate_limit() {
        assert!(matches!(
            classify_github_rate_limit(
                StatusCode::FORBIDDEN,
                r#"{"message":"You have triggered an abuse detection mechanism."}"#,
            ),
            Some(DomainError::RateLimited { message }) if message == GITHUB_RATE_LIMIT_MESSAGE
        ));
    }

    #[test]
    fn ignores_rate_limit_tokens_on_unrelated_status_codes() {
        assert!(classify_github_rate_limit(StatusCode::BAD_REQUEST, "rate limit").is_none());
    }
}

