use crate::application::errors::ApplicationError;

const GEMINI_FLASH_MAX_THINKING_BUDGET: i64 = 24_576;
const GEMINI_PRO_MAX_THINKING_BUDGET: i64 = 32_768;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GeminiThinkingControl {
    BudgetTokens(i64),
    Level(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GeminiThinkingModel {
    Gemini25FlashLite,
    Gemini25Flash,
    Gemini25Pro,
    Gemini3Flash,
    Gemini3ProLowHigh,
    Gemini3ProMedium,
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

pub(super) fn is_zai_reasoning_effort_model(model: &str) -> bool {
    model.trim().eq_ignore_ascii_case("glm-5.2")
}

pub(super) fn map_zai_reasoning_effort(
    model: &str,
    value: &str,
) -> Result<Option<&'static str>, ApplicationError> {
    let effort = parse_known_reasoning_effort(value, "Z.AI")?;
    match effort {
        RequestedReasoningEffort::Auto => Ok(None),
        _ if !is_zai_reasoning_effort_model(model) => Err(ApplicationError::ValidationError(
            "Z.AI reasoning_effort is only supported by glm-5.2".to_string(),
        )),
        RequestedReasoningEffort::None => Ok(Some("none")),
        RequestedReasoningEffort::Minimal => Ok(Some("minimal")),
        RequestedReasoningEffort::Low => Ok(Some("low")),
        RequestedReasoningEffort::Medium => Ok(Some("medium")),
        RequestedReasoningEffort::High => Ok(Some("high")),
        RequestedReasoningEffort::Max => Ok(Some("max")),
        RequestedReasoningEffort::XHigh => Ok(Some("xhigh")),
    }
}

pub(super) fn is_gemini_thinking_config_model(model: &str) -> bool {
    classify_gemini_thinking_model(model).is_some()
}

pub(super) fn map_gemini_thinking_control(
    model: &str,
    max_tokens: i64,
    effort: RequestedReasoningEffort,
) -> Result<Option<GeminiThinkingControl>, ApplicationError> {
    if effort == RequestedReasoningEffort::None {
        return Err(unsupported_reasoning_effort("Gemini", "none"));
    }

    let Some(model) = classify_gemini_thinking_model(model) else {
        return Ok(None);
    };

    let max_tokens = max_tokens.max(0);
    let control = match model {
        GeminiThinkingModel::Gemini25FlashLite => {
            gemini_flash_lite_budget(max_tokens, effort).map(GeminiThinkingControl::BudgetTokens)
        }
        GeminiThinkingModel::Gemini25Flash => Some(GeminiThinkingControl::BudgetTokens(
            gemini_flash_budget(max_tokens, effort),
        )),
        GeminiThinkingModel::Gemini25Pro => Some(GeminiThinkingControl::BudgetTokens(
            gemini_pro_budget(max_tokens, effort),
        )),
        GeminiThinkingModel::Gemini3Flash => {
            gemini_3_flash_level(effort).map(GeminiThinkingControl::Level)
        }
        GeminiThinkingModel::Gemini3ProLowHigh => {
            gemini_3_pro_low_high_level(effort).map(GeminiThinkingControl::Level)
        }
        GeminiThinkingModel::Gemini3ProMedium => {
            gemini_3_pro_medium_level(effort).map(GeminiThinkingControl::Level)
        }
    };

    Ok(control)
}

fn classify_gemini_thinking_model(model: &str) -> Option<GeminiThinkingModel> {
    let model = model.trim().to_ascii_lowercase();
    if model.starts_with("gemini-2.5-") && is_gemini_image_model(&model) {
        return None;
    }

    if model.starts_with("gemini-2.5-flash-lite") {
        return Some(GeminiThinkingModel::Gemini25FlashLite);
    }
    if model.starts_with("gemini-2.5-flash") {
        return Some(GeminiThinkingModel::Gemini25Flash);
    }
    if model.starts_with("gemini-2.5-pro") {
        return Some(GeminiThinkingModel::Gemini25Pro);
    }
    if is_gemini_3_variant(&model, "flash") {
        return Some(GeminiThinkingModel::Gemini3Flash);
    }
    if is_gemini_3_variant(&model, "pro") {
        return Some(if model.starts_with("gemini-3.") {
            GeminiThinkingModel::Gemini3ProMedium
        } else {
            GeminiThinkingModel::Gemini3ProLowHigh
        });
    }

    None
}

fn is_gemini_image_model(model: &str) -> bool {
    model.ends_with("-image") || model.ends_with("-image-preview")
}

fn is_gemini_3_variant(model: &str, variant: &str) -> bool {
    let Some(rest) = model.strip_prefix("gemini-3") else {
        return false;
    };

    let version_end = rest
        .find(|character: char| character != '.' && !character.is_ascii_digit())
        .unwrap_or(rest.len());
    let Some(name) = rest[version_end..].strip_prefix('-') else {
        return false;
    };

    name == variant
        || name
            .strip_prefix(variant)
            .is_some_and(|tail| tail.starts_with('-'))
}

fn gemini_flash_lite_budget(max_tokens: i64, effort: RequestedReasoningEffort) -> Option<i64> {
    match effort {
        RequestedReasoningEffort::Auto => None,
        effort => Some(gemini_budget_tokens(
            max_tokens,
            effort,
            0,
            512,
            GEMINI_FLASH_MAX_THINKING_BUDGET,
        )),
    }
}

fn gemini_flash_budget(max_tokens: i64, effort: RequestedReasoningEffort) -> i64 {
    gemini_budget_tokens(max_tokens, effort, 0, 0, GEMINI_FLASH_MAX_THINKING_BUDGET)
}

fn gemini_pro_budget(max_tokens: i64, effort: RequestedReasoningEffort) -> i64 {
    gemini_budget_tokens(max_tokens, effort, 128, 128, GEMINI_PRO_MAX_THINKING_BUDGET)
}

fn gemini_budget_tokens(
    max_tokens: i64,
    effort: RequestedReasoningEffort,
    minimal_tokens: i64,
    min_budget: i64,
    max_budget: i64,
) -> i64 {
    let tokens = match effort {
        RequestedReasoningEffort::Auto => -1,
        RequestedReasoningEffort::Minimal => minimal_tokens,
        RequestedReasoningEffort::Low => max_tokens.saturating_mul(10) / 100,
        RequestedReasoningEffort::Medium => max_tokens.saturating_mul(25) / 100,
        RequestedReasoningEffort::High => max_tokens.saturating_mul(50) / 100,
        RequestedReasoningEffort::Max | RequestedReasoningEffort::XHigh => max_tokens,
        RequestedReasoningEffort::None => {
            unreachable!("Gemini reasoning mapper rejects none")
        }
    };

    if tokens < 0 {
        tokens
    } else {
        tokens.clamp(min_budget, max_budget)
    }
}

fn gemini_3_flash_level(effort: RequestedReasoningEffort) -> Option<&'static str> {
    match effort {
        RequestedReasoningEffort::Auto => None,
        RequestedReasoningEffort::Minimal => Some("minimal"),
        RequestedReasoningEffort::Low => Some("low"),
        RequestedReasoningEffort::Medium => Some("medium"),
        RequestedReasoningEffort::High
        | RequestedReasoningEffort::Max
        | RequestedReasoningEffort::XHigh => Some("high"),
        RequestedReasoningEffort::None => unreachable!("Gemini reasoning mapper rejects none"),
    }
}

fn gemini_3_pro_low_high_level(effort: RequestedReasoningEffort) -> Option<&'static str> {
    match effort {
        RequestedReasoningEffort::Auto => None,
        RequestedReasoningEffort::Minimal
        | RequestedReasoningEffort::Low
        | RequestedReasoningEffort::Medium => Some("low"),
        RequestedReasoningEffort::High
        | RequestedReasoningEffort::Max
        | RequestedReasoningEffort::XHigh => Some("high"),
        RequestedReasoningEffort::None => unreachable!("Gemini reasoning mapper rejects none"),
    }
}

fn gemini_3_pro_medium_level(effort: RequestedReasoningEffort) -> Option<&'static str> {
    match effort {
        RequestedReasoningEffort::Auto => None,
        RequestedReasoningEffort::Minimal | RequestedReasoningEffort::Low => Some("low"),
        RequestedReasoningEffort::Medium => Some("medium"),
        RequestedReasoningEffort::High
        | RequestedReasoningEffort::Max
        | RequestedReasoningEffort::XHigh => Some("high"),
        RequestedReasoningEffort::None => unreachable!("Gemini reasoning mapper rejects none"),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        GeminiThinkingControl, RequestedReasoningEffort, is_gemini_thinking_config_model,
        is_openrouter_claude_model_name, is_zai_reasoning_effort_model,
        map_gemini_thinking_control, map_openrouter_reasoning_effort, map_zai_reasoning_effort,
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

    #[test]
    fn zai_reasoning_effort_classifier_matches_glm52() {
        assert!(is_zai_reasoning_effort_model(" glm-5.2 "));
        assert!(!is_zai_reasoning_effort_model("glm-5.1"));
        assert!(!is_zai_reasoning_effort_model("glm-5"));
    }

    #[test]
    fn zai_reasoning_effort_maps_project_aliases_to_zai_enum() {
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
            ("max", Some("max")),
            ("maximum", Some("max")),
            ("xhigh", Some("xhigh")),
        ] {
            assert_eq!(
                map_zai_reasoning_effort("glm-5.2", input).expect("known effort must map"),
                expected
            );
        }
    }

    #[test]
    fn zai_reasoning_effort_rejects_unknown_values() {
        let error = map_zai_reasoning_effort("glm-5.2", "turbo")
            .expect_err("unknown effort should fail locally");
        assert!(
            error
                .to_string()
                .contains("Unsupported Z.AI reasoning_effort")
        );
    }

    #[test]
    fn zai_reasoning_effort_rejects_unsupported_models() {
        let error =
            map_zai_reasoning_effort("glm-5.1", "high").expect_err("unsupported model should fail");
        assert!(
            error
                .to_string()
                .contains("Z.AI reasoning_effort is only supported by glm-5.2")
        );
    }

    #[test]
    fn gemini_thinking_classifier_matches_supported_families() {
        for model in [
            "gemini-2.5-flash",
            "gemini-2.5-flash-lite-preview-09-2025",
            "gemini-2.5-pro-preview-06-05",
            "gemini-3-pro-preview",
            "gemini-3.1-pro-preview",
            "gemini-3-flash-preview",
            "gemini-3.1-flash-lite-preview",
            "gemini-3-pro-image-preview",
            "gemini-3.1-flash-image-preview",
            "gemini-3.5-flash",
        ] {
            assert!(
                is_gemini_thinking_config_model(model),
                "{model} should support Gemini thinkingConfig"
            );
        }

        for model in [
            "gemini-2.5-flash-image",
            "gemini-2.5-flash-image-preview",
            "gemma-3",
        ] {
            assert!(
                !is_gemini_thinking_config_model(model),
                "{model} should not receive Gemini thinkingConfig"
            );
        }
    }

    #[test]
    fn gemini_25_models_map_effort_to_budget_tokens() {
        for (model, max_tokens, effort, expected) in [
            (
                "gemini-2.5-flash",
                4000,
                RequestedReasoningEffort::Medium,
                1000,
            ),
            (
                "gemini-2.5-flash",
                4000,
                RequestedReasoningEffort::Minimal,
                0,
            ),
            (
                "gemini-2.5-flash-lite",
                4000,
                RequestedReasoningEffort::Minimal,
                512,
            ),
            (
                "gemini-2.5-pro",
                8000,
                RequestedReasoningEffort::Minimal,
                128,
            ),
            (
                "gemini-2.5-pro",
                8000,
                RequestedReasoningEffort::XHigh,
                8000,
            ),
            ("gemini-2.5-pro", 8000, RequestedReasoningEffort::Auto, -1),
        ] {
            assert_eq!(
                map_gemini_thinking_control(model, max_tokens, effort)
                    .expect("known Gemini effort should map"),
                Some(GeminiThinkingControl::BudgetTokens(expected))
            );
        }
    }

    #[test]
    fn gemini_25_flash_lite_auto_preserves_model_default() {
        assert_eq!(
            map_gemini_thinking_control(
                "gemini-2.5-flash-lite",
                4000,
                RequestedReasoningEffort::Auto,
            )
            .expect("auto should be accepted"),
            None
        );
    }

    #[test]
    fn gemini_3_models_map_effort_to_thinking_level() {
        for (model, effort, expected) in [
            (
                "gemini-3-pro-preview",
                RequestedReasoningEffort::Medium,
                "low",
            ),
            (
                "gemini-3.1-pro-preview",
                RequestedReasoningEffort::Medium,
                "medium",
            ),
            (
                "gemini-3-flash-preview",
                RequestedReasoningEffort::Minimal,
                "minimal",
            ),
            (
                "gemini-3.1-flash-lite-preview",
                RequestedReasoningEffort::Medium,
                "medium",
            ),
            ("gemini-3.5-flash", RequestedReasoningEffort::Max, "high"),
            ("gemini-3.5-flash", RequestedReasoningEffort::XHigh, "high"),
        ] {
            assert_eq!(
                map_gemini_thinking_control(model, 8000, effort)
                    .expect("known Gemini effort should map"),
                Some(GeminiThinkingControl::Level(expected))
            );
        }
    }

    #[test]
    fn gemini_auto_omits_level_for_level_based_models() {
        assert_eq!(
            map_gemini_thinking_control(
                "gemini-3.1-pro-preview",
                8000,
                RequestedReasoningEffort::Auto,
            )
            .expect("auto should be accepted"),
            None
        );
    }

    #[test]
    fn gemini_rejects_none_reasoning_effort() {
        let error = map_gemini_thinking_control(
            "gemini-3-flash-preview",
            8000,
            RequestedReasoningEffort::None,
        )
        .expect_err("none should fail locally");
        assert!(
            error
                .to_string()
                .contains("Unsupported Gemini reasoning_effort")
        );
    }
}
