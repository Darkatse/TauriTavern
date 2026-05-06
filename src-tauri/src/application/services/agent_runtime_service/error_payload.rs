use serde_json::{Value, json};

use crate::application::errors::ApplicationError;

pub(super) fn run_failure_payload(error: &ApplicationError) -> Value {
    let (code, message) = agent_error_code_and_message(error);

    json!({
        "code": code,
        "message": message,
        "technicalMessage": error.to_string(),
        "retryable": is_retryable(error),
        "details": {},
    })
}

fn agent_error_code_and_message(error: &ApplicationError) -> (String, String) {
    match error {
        ApplicationError::RateLimited(message) => {
            structured_code_and_message(message, "agent.rate_limited")
        }
        ApplicationError::Transient(message) => {
            structured_code_and_message(message, "agent.transient")
        }
        ApplicationError::Cancelled(message) => {
            structured_code_and_message(message, "agent.cancelled")
        }
        ApplicationError::InternalError(message) => {
            structured_code_and_message(message, "agent.internal_error")
        }
        ApplicationError::ValidationError(message) => {
            structured_code_and_message(message, "agent.validation_error")
        }
        ApplicationError::NotFound(message) => {
            structured_code_and_message(message, "agent.not_found")
        }
        ApplicationError::Unauthorized(message) => {
            structured_code_and_message(message, "agent.unauthorized")
        }
        ApplicationError::PermissionDenied(message) => {
            structured_code_and_message(message, "agent.permission_denied")
        }
    }
}

fn structured_code_and_message(message: &str, fallback_code: &str) -> (String, String) {
    let message = message.trim();
    if let Some((code, detail)) = message.split_once(':') {
        let code = code.trim();
        if is_error_code(code) {
            return (code.to_string(), detail.trim().to_string());
        }
    }

    (fallback_code.to_string(), message.to_string())
}

fn is_error_code(value: &str) -> bool {
    !value.is_empty()
        && value.contains('.')
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
}

fn is_retryable(error: &ApplicationError) -> bool {
    matches!(
        error,
        ApplicationError::RateLimited(_) | ApplicationError::Transient(_)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_error_with_code_becomes_structured_run_failure_payload() {
        let payload = run_failure_payload(&ApplicationError::ValidationError(
            "model.tool_call_required: model must use Agent tools and finish through workspace_finish"
                .to_string(),
        ));

        assert_eq!(payload["code"], "model.tool_call_required");
        assert_eq!(
            payload["message"],
            "model must use Agent tools and finish through workspace_finish"
        );
        assert_eq!(
            payload["technicalMessage"],
            "Validation error: model.tool_call_required: model must use Agent tools and finish through workspace_finish"
        );
        assert_eq!(payload["retryable"], false);
        assert_eq!(payload["details"], json!({}));
    }

    #[test]
    fn application_error_without_code_uses_variant_code() {
        let payload = run_failure_payload(&ApplicationError::PermissionDenied(
            "workspace root is hidden".to_string(),
        ));

        assert_eq!(payload["code"], "agent.permission_denied");
        assert_eq!(payload["message"], "workspace root is hidden");
        assert_eq!(
            payload["technicalMessage"],
            "Permission denied: workspace root is hidden"
        );
        assert_eq!(payload["retryable"], false);
    }

    #[test]
    fn rate_limited_error_is_retryable() {
        let payload = run_failure_payload(&ApplicationError::RateLimited(
            "model.provider_rate_limited: upstream rate limit".to_string(),
        ));

        assert_eq!(payload["code"], "model.provider_rate_limited");
        assert_eq!(payload["message"], "upstream rate limit");
        assert_eq!(payload["retryable"], true);
    }
}
