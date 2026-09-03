use crate::errors::ApplicationError;

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
    XHigh,
    Max,
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
            "xhigh" => Some(Self::XHigh),
            "max" | "maximum" => Some(Self::Max),
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
    Gemini37Flash,
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
        RequestedReasoningEffort::XHigh => Ok(Some("xhigh")),
        RequestedReasoningEffort::Max => Ok(Some("max")),
    }
}

pub(super) fn map_zai_reasoning_effort(
    model: &str,
    value: &str,
) -> Result<Option<&'static str>, ApplicationError> {
    let effort = parse_known_reasoning_effort(value, "Z.AI")?;
    let model = model.trim().to_ascii_lowercase();

    match effort {
        RequestedReasoningEffort::Auto => Ok(None),
        _ if !matches!(model.as_str(), "glm-5.2" | "glm-5.3" | "glm-5.3-flash") => {
            Err(ApplicationError::ValidationError(
                "Z.AI reasoning_effort is only supported by glm-5.2, glm-5.3, and glm-5.3-flash"
                    .to_string(),
            ))
        }
        RequestedReasoningEffort::None => Ok(Some("none")),
        RequestedReasoningEffort::Minimal => Ok(Some("minimal")),
        RequestedReasoningEffort::Low => Ok(Some("low")),
        RequestedReasoningEffort::Medium => Ok(Some("medium")),
        RequestedReasoningEffort::High => Ok(Some("high")),
        RequestedReasoningEffort::XHigh | RequestedReasoningEffort::Max => Ok(Some("max")),
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
        GeminiThinkingModel::Gemini37Flash => {
            gemini_3_7_flash_level(effort).map(GeminiThinkingControl::Level)
        }
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
    if model == "gemini-3.7-flash" {
        return Some(GeminiThinkingModel::Gemini37Flash);
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

fn gemini_3_7_flash_level(effort: RequestedReasoningEffort) -> Option<&'static str> {
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
        GeminiThinkingControl, RequestedReasoningEffort, map_gemini_thinking_control,
        map_openrouter_reasoning_effort,
    };

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
            ("gemini-3.7-flash", RequestedReasoningEffort::Minimal, "low"),
        ] {
            assert_eq!(
                map_gemini_thinking_control(model, 8000, effort)
                    .expect("known Gemini effort should map"),
                Some(GeminiThinkingControl::Level(expected))
            );
        }
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
