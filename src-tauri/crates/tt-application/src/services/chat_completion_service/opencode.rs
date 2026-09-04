use serde_json::{Map, Value};

use crate::errors::ApplicationError;

const ZEN_API_BASE: &str = "https://opencode.ai/zen/v1";
const GO_API_BASE: &str = "https://opencode.ai/zen/go/v1";

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
    let format = OpenCodeApiFormat::parse(api_format)?;
    base_url(endpoint, format)?;
    Ok(format)
}

pub(super) fn base_url(
    endpoint: &str,
    format: OpenCodeApiFormat,
) -> Result<&'static str, ApplicationError> {
    match (endpoint.trim(), format) {
        ("" | "zen", _) => Ok(ZEN_API_BASE),
        ("go", OpenCodeApiFormat::Gemini) => Err(ApplicationError::ValidationError(
            "OpenCode Go does not support the Gemini API format".to_string(),
        )),
        ("go", _) => Ok(GO_API_BASE),
        (other, _) => Err(ApplicationError::ValidationError(format!(
            "Unsupported OpenCode endpoint: {other}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::{OpenCodeApiFormat, base_url};

    #[test]
    fn service_and_format_are_validated_together() {
        assert_eq!(
            base_url("go", OpenCodeApiFormat::OpenAiCompat).unwrap(),
            "https://opencode.ai/zen/go/v1"
        );
        assert!(base_url("go", OpenCodeApiFormat::Gemini).is_err());
        assert!(OpenCodeApiFormat::parse("unknown").is_err());
    }
}
