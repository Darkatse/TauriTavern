use crate::application::errors::ApplicationError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RequestedReasoningEffort {
    Auto,
    None,
    Minimal,
    Low,
    Medium,
    High,
    Max,
    XHigh,
}

impl RequestedReasoningEffort {
    pub(super) fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "" | "auto" => Some(Self::Auto),
            "none" => Some(Self::None),
            "min" | "minimum" | "minimal" => Some(Self::Minimal),
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            "max" | "maximum" => Some(Self::Max),
            "xhigh" => Some(Self::XHigh),
            _ => None,
        }
    }
}

pub(super) fn parse_known_reasoning_effort(
    value: &str,
    provider: &str,
) -> Result<RequestedReasoningEffort, ApplicationError> {
    RequestedReasoningEffort::parse(value)
        .ok_or_else(|| unsupported_reasoning_effort(provider, value))
}

pub(super) fn unsupported_reasoning_effort(provider: &str, value: &str) -> ApplicationError {
    ApplicationError::ValidationError(format!(
        "Unsupported {provider} reasoning_effort: {}",
        value.trim().to_ascii_lowercase()
    ))
}

pub(super) fn is_openrouter_claude_model_name(model: &str) -> bool {
    model
        .trim()
        .to_ascii_lowercase()
        .starts_with("anthropic/claude")
}

pub(super) fn map_openrouter_reasoning_effort(
    value: &str,
) -> Result<Option<&'static str>, ApplicationError> {
    match parse_known_reasoning_effort(value, "OpenRouter")? {
        RequestedReasoningEffort::Auto => Ok(None),
        RequestedReasoningEffort::None => Ok(Some("none")),
        RequestedReasoningEffort::Minimal => Ok(Some("minimal")),
        RequestedReasoningEffort::Low => Ok(Some("low")),
        RequestedReasoningEffort::Medium => Ok(Some("medium")),
        RequestedReasoningEffort::High => Ok(Some("high")),
        RequestedReasoningEffort::Max => Ok(Some("high")),
        RequestedReasoningEffort::XHigh => Ok(Some("xhigh")),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        RequestedReasoningEffort, is_openrouter_claude_model_name, map_openrouter_reasoning_effort,
    };

    #[test]
    fn requested_reasoning_effort_parser_normalizes_project_aliases() {
        for (input, expected) in [
            ("auto", Some(RequestedReasoningEffort::Auto)),
            ("", Some(RequestedReasoningEffort::Auto)),
            ("none", Some(RequestedReasoningEffort::None)),
            ("min", Some(RequestedReasoningEffort::Minimal)),
            ("minimum", Some(RequestedReasoningEffort::Minimal)),
            ("minimal", Some(RequestedReasoningEffort::Minimal)),
            ("low", Some(RequestedReasoningEffort::Low)),
            ("medium", Some(RequestedReasoningEffort::Medium)),
            ("high", Some(RequestedReasoningEffort::High)),
            ("max", Some(RequestedReasoningEffort::Max)),
            ("maximum", Some(RequestedReasoningEffort::Max)),
            ("xhigh", Some(RequestedReasoningEffort::XHigh)),
            ("turbo", None),
        ] {
            assert_eq!(RequestedReasoningEffort::parse(input), expected);
        }
    }

    #[test]
    fn openrouter_claude_classifier_matches_anthropic_route() {
        assert!(is_openrouter_claude_model_name(
            " anthropic/claude-sonnet-4-5 "
        ));
        assert!(!is_openrouter_claude_model_name("openai/gpt-5.2"));
    }

    #[test]
    fn openrouter_reasoning_effort_maps_project_aliases_to_router_enum() {
        for (input, expected) in [
            ("auto", None),
            ("", None),
            ("none", Some("none")),
            ("min", Some("minimal")),
            ("minimum", Some("minimal")),
            ("minimal", Some("minimal")),
            ("low", Some("low")),
            ("medium", Some("medium")),
            ("high", Some("high")),
            ("max", Some("high")),
            ("maximum", Some("high")),
            ("xhigh", Some("xhigh")),
        ] {
            assert_eq!(
                map_openrouter_reasoning_effort(input).expect("known effort must map"),
                expected
            );
        }
    }

    #[test]
    fn openrouter_reasoning_effort_rejects_unknown_values() {
        let error = map_openrouter_reasoning_effort("turbo")
            .expect_err("unknown effort should fail locally");
        assert!(
            error
                .to_string()
                .contains("Unsupported OpenRouter reasoning_effort")
        );
    }
}
