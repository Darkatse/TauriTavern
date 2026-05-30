use serde_json::{Value, json};

use super::commit_ledger::RunCommitLedger;
use crate::application::errors::ApplicationError;

/// Drift-class codes that should _never_ be auto-retried (the model
/// disobeyed the tool contract), but the user is allowed to manually retry
/// when no host-confirmed chat commit needs to be preserved. See issue #55.
const USER_RETRYABLE_DRIFT_CODES: &[&str] = &[
    "model.tool_call_required",
    "agent.tool_after_finish",
    "agent.max_tool_rounds_exceeded",
];

pub(super) fn run_failure_payload(error: &ApplicationError) -> Value {
    let (code, message) = agent_error_code_and_message(error);
    let retryable = is_retryable(error);
    let user_retryable = retryable || USER_RETRYABLE_DRIFT_CODES.contains(&code.as_str());

    json!({
        "code": code,
        "message": message,
        "technicalMessage": error.to_string(),
        "retryable": retryable,
        "userRetryable": user_retryable,
        "details": {},
    })
}

pub(super) fn run_partial_success_payload(
    error: &ApplicationError,
    commit_ledger: &RunCommitLedger,
) -> Value {
    let mut payload = run_failure_payload(error);
    let object = payload
        .as_object_mut()
        .expect("run failure payload must be a JSON object");
    object.insert("retryable".to_string(), json!(false));
    object.insert("userRetryable".to_string(), json!(false));
    object.insert(
        "preservedCommitCount".to_string(),
        json!(commit_ledger.len()),
    );
    object.insert(
        "preservedCommits".to_string(),
        Value::Array(commit_ledger.preserved_commits()),
    );
    payload
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
        // Issue #55: drift errors must offer a manual Retry path even
        // though automatic retry is disabled.
        assert_eq!(payload["userRetryable"], true);
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
        // Non-drift, non-retryable errors stay non-user-retryable so the
        // UI does not lure the user into clicking Retry on policy errors.
        assert_eq!(payload["userRetryable"], false);
    }

    #[test]
    fn rate_limited_error_is_retryable() {
        let payload = run_failure_payload(&ApplicationError::RateLimited(
            "model.provider_rate_limited: upstream rate limit".to_string(),
        ));

        assert_eq!(payload["code"], "model.provider_rate_limited");
        assert_eq!(payload["message"], "upstream rate limit");
        assert_eq!(payload["retryable"], true);
        // Auto-retryable errors are user-retryable by definition.
        assert_eq!(payload["userRetryable"], true);
    }

    #[test]
    fn tool_after_finish_drift_is_user_retryable() {
        let payload = run_failure_payload(&ApplicationError::ValidationError(
            "agent.tool_after_finish: model requested additional tools after workspace.finish"
                .to_string(),
        ));

        assert_eq!(payload["code"], "agent.tool_after_finish");
        assert_eq!(payload["retryable"], false);
        assert_eq!(payload["userRetryable"], true);
    }

    #[test]
    fn max_tool_rounds_exceeded_is_user_retryable() {
        let payload = run_failure_payload(&ApplicationError::ValidationError(
            "agent.max_tool_rounds_exceeded: workspace.finish was not called within 12 rounds"
                .to_string(),
        ));

        assert_eq!(payload["code"], "agent.max_tool_rounds_exceeded");
        assert_eq!(payload["retryable"], false);
        assert_eq!(payload["userRetryable"], true);
    }

    #[test]
    fn upstream_invalid_response_is_transient_and_retryable() {
        let payload = run_failure_payload(&ApplicationError::Transient(
            "model.upstream_invalid_response: openai returned status 200 non-JSON body (generate): expected value at line 1 column 1"
                .to_string(),
        ));

        assert_eq!(payload["code"], "model.upstream_invalid_response");
        assert_eq!(
            payload["message"],
            "openai returned status 200 non-JSON body (generate): expected value at line 1 column 1"
        );
        assert_eq!(payload["retryable"], true);
        assert_eq!(payload["userRetryable"], true);
    }

    #[test]
    fn partial_success_payload_preserves_commits_but_disables_retry_flags() {
        let mut ledger = RunCommitLedger::default();
        ledger.record(
            &crate::domain::models::agent::WorkspacePath::parse("output/main.md").unwrap(),
            crate::domain::models::agent::AgentChatCommitMode::Replace,
            Some("42".to_string()),
            3,
        );

        let payload = run_partial_success_payload(
            &ApplicationError::ValidationError(
                "model.tool_call_required: model must use Agent tools and finish through workspace_finish"
                    .to_string(),
            ),
            &ledger,
        );

        assert_eq!(payload["code"], "model.tool_call_required");
        assert_eq!(payload["retryable"], false);
        assert_eq!(payload["userRetryable"], false);
        assert_eq!(payload["preservedCommitCount"], 1);
        assert_eq!(payload["preservedCommits"][0]["path"], "output/main.md");
        assert_eq!(payload["preservedCommits"][0]["messageId"], "42");
    }
}
