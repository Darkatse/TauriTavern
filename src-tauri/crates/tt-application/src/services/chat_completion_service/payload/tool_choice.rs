use serde_json::Value;

use crate::errors::ApplicationError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OpenAiToolChoice<'a> {
    None,
    Auto,
    Required,
    Specific(&'a str),
}

pub(super) fn parse_openai_tool_choice<'a>(
    value: &'a Value,
    provider: &str,
) -> Result<OpenAiToolChoice<'a>, ApplicationError> {
    if let Some(choice) = value.as_str() {
        return match choice {
            "none" => Ok(OpenAiToolChoice::None),
            "auto" => Ok(OpenAiToolChoice::Auto),
            "required" => Ok(OpenAiToolChoice::Required),
            _ => Err(invalid_tool_choice(provider)),
        };
    }

    let object = value
        .as_object()
        .ok_or_else(|| invalid_tool_choice(provider))?;
    let choice_type = object
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_tool_choice(provider))?;

    match choice_type {
        "auto" => Ok(OpenAiToolChoice::Auto),
        "any" => Ok(OpenAiToolChoice::Required),
        "none" => Ok(OpenAiToolChoice::None),
        "function" => specific_name(
            object
                .get("function")
                .and_then(Value::as_object)
                .and_then(|function| function.get("name"))
                .or_else(|| object.get("name")),
            provider,
        ),
        "tool" => specific_name(object.get("name"), provider),
        _ => Err(invalid_tool_choice(provider)),
    }
}

fn specific_name<'a>(
    value: Option<&'a Value>,
    provider: &str,
) -> Result<OpenAiToolChoice<'a>, ApplicationError> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(OpenAiToolChoice::Specific)
        .ok_or_else(|| invalid_tool_choice(provider))
}

fn invalid_tool_choice(provider: &str) -> ApplicationError {
    ApplicationError::ValidationError(format!(
        "provider.tool_choice_invalid: {provider} cannot translate the supplied tool_choice"
    ))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{OpenAiToolChoice, parse_openai_tool_choice};

    #[test]
    fn parses_canonical_and_specific_tool_choices() {
        assert_eq!(
            parse_openai_tool_choice(&json!("required"), "test").unwrap(),
            OpenAiToolChoice::Required
        );
        assert_eq!(
            parse_openai_tool_choice(
                &json!({ "type": "function", "function": { "name": "search" } }),
                "test",
            )
            .unwrap(),
            OpenAiToolChoice::Specific("search")
        );
        assert!(parse_openai_tool_choice(&json!("unexpected"), "test").is_err());
    }
}
