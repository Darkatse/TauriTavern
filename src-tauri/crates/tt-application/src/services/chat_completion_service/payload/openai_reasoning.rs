use std::borrow::Cow;

use super::super::model_capabilities::RequestedReasoningEffort;

const OPENAI_REASONING_EFFORT_MODELS: &[&str] = &[
    "o1",
    "o3-mini",
    "o3-mini-2025-01-31",
    "o4-mini",
    "o4-mini-2025-04-16",
    "o3",
    "o3-2025-04-16",
    "gpt-5",
    "gpt-5-2025-08-07",
    "gpt-5-mini",
    "gpt-5-mini-2025-08-07",
    "gpt-5-nano",
    "gpt-5-nano-2025-08-07",
    "gpt-5.1",
    "gpt-5.1-2025-11-13",
    "gpt-5.1-chat-latest",
    "gpt-5.1-codex-max",
    "gpt-5.2",
    "gpt-5.2-2025-12-11",
    "gpt-5.2-chat-latest",
    "gpt-5.4",
    "gpt-5.4-2026-03-05",
    "gpt-5.4-chat-latest",
    "gpt-5.4-mini",
    "gpt-5.4-mini-2026-03-17",
    "gpt-5.4-nano",
    "gpt-5.4-nano-2026-03-17",
    "gpt-5.5",
    "gpt-5.5-2026-04-23",
];

// OpenAI documents xhigh as starting at gpt-5.1-codex-max and later GPT models.
const OPENAI_XHIGH_REASONING_THRESHOLD: &str = "gpt-5.1-codex-max";

pub(super) fn should_forward_openai_reasoning_effort(source: &str, model: &str) -> bool {
    let model = model.trim();
    matches!(source, "openai" | "custom")
        && (OPENAI_REASONING_EFFORT_MODELS.contains(&model)
            || supports_openai_xhigh_reasoning_effort(model))
}

pub(super) fn normalize_openai_reasoning_effort<'a>(
    value: &'a str,
    model: &str,
) -> Option<Cow<'a, str>> {
    normalize_reasoning_effort(value, supports_openai_xhigh_reasoning_effort(model))
}

/// Maps project reasoning-effort aliases onto OpenAI's provider enum. `auto` is
/// dropped, `min`/`minimum`/`minimal` become `minimal`, `max`/`maximum` become
/// `high`, and `xhigh` is preserved only when `allow_xhigh` is set.
pub(super) fn normalize_reasoning_effort(value: &str, allow_xhigh: bool) -> Option<Cow<'_, str>> {
    let value = value.trim();
    match RequestedReasoningEffort::parse(value) {
        Some(RequestedReasoningEffort::Auto) => None,
        Some(RequestedReasoningEffort::None) => Some(Cow::Borrowed("none")),
        Some(RequestedReasoningEffort::Minimal) => Some(Cow::Borrowed("minimal")),
        Some(RequestedReasoningEffort::Low) => Some(Cow::Borrowed("low")),
        Some(RequestedReasoningEffort::Medium) => Some(Cow::Borrowed("medium")),
        Some(RequestedReasoningEffort::High) => Some(Cow::Borrowed("high")),
        Some(RequestedReasoningEffort::Max) => Some(Cow::Borrowed("high")),
        Some(RequestedReasoningEffort::XHigh) => {
            Some(Cow::Borrowed(if allow_xhigh { "xhigh" } else { "high" }))
        }
        None => Some(Cow::Borrowed(value)),
    }
}

fn supports_openai_xhigh_reasoning_effort(model: &str) -> bool {
    let model = model.trim().to_ascii_lowercase();
    if model
        .strip_prefix(OPENAI_XHIGH_REASONING_THRESHOLD)
        .is_some_and(|suffix| suffix.is_empty() || suffix.starts_with('-'))
    {
        return true;
    }

    if let Some(minor) = parse_gpt5_minor_version(&model) {
        return minor >= 2;
    }

    parse_gpt_major_version(&model).is_some_and(|major| major > 5)
}

fn parse_gpt5_minor_version(model: &str) -> Option<u16> {
    parse_leading_digits(model.strip_prefix("gpt-5.")?)
}

fn parse_gpt_major_version(model: &str) -> Option<u16> {
    parse_leading_digits(model.strip_prefix("gpt-")?)
}

fn parse_leading_digits(value: &str) -> Option<u16> {
    let end = value
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(value.len());
    if end == 0 {
        return None;
    }
    value[..end].parse().ok()
}

#[cfg(test)]
mod tests {
    use super::{normalize_openai_reasoning_effort, supports_openai_xhigh_reasoning_effort};

    #[test]
    fn openai_xhigh_support_starts_at_codex_max_and_later_gpt_models() {
        for model in [
            "gpt-5.1-codex-max",
            "gpt-5.1-codex-max-2025-11-19",
            "gpt-5.2",
            "gpt-5.3-codex",
            "gpt-5.10",
            "gpt-6",
        ] {
            assert!(
                supports_openai_xhigh_reasoning_effort(model),
                "{model} should support xhigh"
            );
        }

        for model in ["gpt-5", "gpt-5.1", "gpt-5.1-chat-latest", "gpt-5-pro"] {
            assert!(
                !supports_openai_xhigh_reasoning_effort(model),
                "{model} should not support xhigh"
            );
        }
    }

    #[test]
    fn openai_reasoning_normalizes_project_maximum_aliases_to_provider_values() {
        assert_eq!(
            normalize_openai_reasoning_effort("max", "gpt-5.1").as_deref(),
            Some("high")
        );
        assert_eq!(
            normalize_openai_reasoning_effort("minimum", "gpt-5.1").as_deref(),
            Some("minimal")
        );
        assert_eq!(
            normalize_openai_reasoning_effort("xhigh", "gpt-5.1").as_deref(),
            Some("high")
        );
        assert_eq!(
            normalize_openai_reasoning_effort("xhigh", "gpt-5.2").as_deref(),
            Some("xhigh")
        );
    }
}
