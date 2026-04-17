use serde_json::{Map, Value};

use crate::application::errors::ApplicationError;

use super::contract::{ClaudeModelContract, ClaudeSamplingMode, ClaudeThinkingMode};
use super::params::{
    collect_non_default_sampling_params, has_non_default_temperature, has_non_default_top_p,
    value_to_i64,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClaudeRequestThinkingMode {
    Enabled,
    Adaptive,
}

pub(super) fn validate_request(payload: &Value) -> Result<(), ApplicationError> {
    let request = payload.as_object().ok_or_else(|| {
        ApplicationError::ValidationError(
            "Claude request payload must be a JSON object".to_string(),
        )
    })?;
    let model = request
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            ApplicationError::ValidationError("Claude request is missing model".to_string())
        })?;
    let contract = ClaudeModelContract::resolve(model);
    let thinking_mode = validate_claude_thinking_request(request, contract, model)?;
    validate_claude_output_config(request, contract, model)?;
    validate_claude_sampling_request(request, contract.sampling, thinking_mode, model)?;

    Ok(())
}

fn validate_claude_thinking_request(
    request: &Map<String, Value>,
    contract: ClaudeModelContract,
    model: &str,
) -> Result<Option<ClaudeRequestThinkingMode>, ApplicationError> {
    let Some(thinking) = request.get("thinking") else {
        return Ok(None);
    };

    let thinking = thinking.as_object().ok_or_else(|| {
        ApplicationError::ValidationError(format!(
            "Claude model `{model}` expects `thinking` to be an object"
        ))
    })?;
    let thinking_type = thinking
        .get("type")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            ApplicationError::ValidationError(format!(
                "Claude model `{model}` requires `thinking.type`"
            ))
        })?;

    let thinking_mode = match thinking_type {
        "enabled" => {
            if thinking
                .get("budget_tokens")
                .and_then(value_to_i64)
                .is_none()
            {
                return Err(ApplicationError::ValidationError(format!(
                    "Claude model `{model}` requires `thinking.budget_tokens` for legacy thinking"
                )));
            }
            ClaudeRequestThinkingMode::Enabled
        }
        "adaptive" => {
            if thinking.get("budget_tokens").is_some() {
                return Err(ApplicationError::ValidationError(format!(
                    "Claude model `{model}` does not allow `thinking.budget_tokens` with adaptive thinking"
                )));
            }

            if let Some(display) = thinking
                .get("display")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                if !matches!(display, "summarized" | "omitted") {
                    return Err(ApplicationError::ValidationError(format!(
                        "Unsupported Claude adaptive thinking display: {display}"
                    )));
                }
            }

            ClaudeRequestThinkingMode::Adaptive
        }
        other => {
            return Err(ApplicationError::ValidationError(format!(
                "Unsupported Claude thinking.type: {other}"
            )));
        }
    };

    match (contract.thinking, thinking_mode) {
        (ClaudeThinkingMode::Unsupported, _) => Err(ApplicationError::ValidationError(format!(
            "Claude model `{model}` does not support thinking"
        ))),
        (ClaudeThinkingMode::ManualOnly, ClaudeRequestThinkingMode::Adaptive) => {
            Err(ApplicationError::ValidationError(format!(
                "Claude model `{model}` requires legacy thinking with budget_tokens"
            )))
        }
        (ClaudeThinkingMode::AdaptiveOnly, ClaudeRequestThinkingMode::Enabled) => {
            Err(ApplicationError::ValidationError(format!(
                "Claude model `{model}` requires adaptive thinking"
            )))
        }
        _ => Ok(Some(thinking_mode)),
    }
}

fn validate_claude_output_config(
    request: &Map<String, Value>,
    contract: ClaudeModelContract,
    model: &str,
) -> Result<(), ApplicationError> {
    let Some(output_config) = request.get("output_config") else {
        return Ok(());
    };

    let output_config = output_config.as_object().ok_or_else(|| {
        ApplicationError::ValidationError(format!(
            "Claude model `{model}` expects `output_config` to be an object"
        ))
    })?;
    let Some(effort) = output_config.get("effort") else {
        return Ok(());
    };

    if !contract.supports_output_effort {
        return Err(ApplicationError::ValidationError(format!(
            "Claude model `{model}` does not support `output_config.effort`"
        )));
    }

    let effort = effort
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            ApplicationError::ValidationError(format!(
                "Claude model `{model}` expects `output_config.effort` to be a non-empty string"
            ))
        })?;

    if !matches!(effort, "low" | "medium" | "high" | "max" | "xhigh") {
        return Err(ApplicationError::ValidationError(format!(
            "Unsupported Claude adaptive effort: {effort}"
        )));
    }

    Ok(())
}

fn validate_claude_sampling_request(
    request: &Map<String, Value>,
    sampling: ClaudeSamplingMode,
    thinking_mode: Option<ClaudeRequestThinkingMode>,
    model: &str,
) -> Result<(), ApplicationError> {
    let disallowed = collect_non_default_sampling_params(request);
    if thinking_mode == Some(ClaudeRequestThinkingMode::Enabled) && !disallowed.is_empty() {
        return Err(ApplicationError::ValidationError(format!(
            "Claude model `{model}` does not allow non-default sampling parameters with legacy thinking: {}",
            disallowed.join(", ")
        )));
    }

    match sampling {
        ClaudeSamplingMode::Full => Ok(()),
        ClaudeSamplingMode::TemperatureOrTopP => {
            if has_non_default_temperature(request) && has_non_default_top_p(request) {
                return Err(ApplicationError::ValidationError(format!(
                    "Claude model `{model}` accepts either temperature or top_p, not both"
                )));
            }
            Ok(())
        }
        ClaudeSamplingMode::None => {
            if disallowed.is_empty() {
                Ok(())
            } else {
                Err(ApplicationError::ValidationError(format!(
                    "Claude model `{model}` does not support non-default sampling parameters: {}",
                    disallowed.join(", ")
                )))
            }
        }
    }
}
