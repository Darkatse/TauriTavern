use serde_json::{Map, Value};

use crate::errors::ApplicationError;

const ZEN_API_BASE: &str = "https://opencode.ai/inference/v1";
const GO_API_BASE: &str = "https://opencode.ai/inference/go/v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::services) enum OpenCodeApiFormat {
    OpenAiCompat,
    OpenAiResponses,
    ClaudeMessages,
    Gemini,
}

impl OpenCodeApiFormat {
    pub(super) fn parse(raw: &str) -> Result<Self, ApplicationError> {
        match raw.trim() {
            "" | "openai_compat" => Ok(Self::OpenAiCompat),
            "openai_responses" => Ok(Self::OpenAiResponses),
            "claude_messages" => Ok(Self::ClaudeMessages),
            "gemini" => Ok(Self::Gemini),
            other => Err(ApplicationError::ValidationError(format!(
                "Unsupported opencode_api_format: {other}"
            ))),
        }
    }
}

pub(super) fn format_from_payload(
    payload: &Map<String, Value>,
) -> Result<OpenCodeApiFormat, ApplicationError> {
    resolve_format(
        payload
            .get("opencode_endpoint")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        payload
            .get("opencode_api_format")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    )
}

pub(in crate::services) fn resolve_format(
    endpoint: &str,
    api_format: &str,
) -> Result<OpenCodeApiFormat, ApplicationError> {
    base_url(endpoint)?;
    OpenCodeApiFormat::parse(api_format)
}

pub(super) fn base_url(endpoint: &str) -> Result<&'static str, ApplicationError> {
    match endpoint.trim() {
        "" | "zen" => Ok(ZEN_API_BASE),
        "go" => Ok(GO_API_BASE),
        other => Err(ApplicationError::ValidationError(format!(
            "Unsupported OpenCode endpoint: {other}"
        ))),
    }
}
