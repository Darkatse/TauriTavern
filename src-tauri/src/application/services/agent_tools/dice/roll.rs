use rand::Rng;
use serde::Serialize;

use super::{MAX_ABS_MODIFIER, MAX_DICE, MAX_SIDES};
use crate::application::errors::ApplicationError;
use crate::application::services::agent_tools::common::{
    object_args, required_trimmed_string_arg, tool_error,
};
use crate::application::services::agent_tools::dispatcher::AgentToolEffect;
use crate::application::services::agent_tools::structured::structured_value;
use crate::domain::models::agent::{AgentToolCall, AgentToolResult};

#[derive(Debug, Clone, PartialEq, Eq)]
struct DiceFormula {
    normalized: String,
    dice: usize,
    sides: u64,
    modifier: i64,
    min: i64,
    max: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DiceRollStructured<'a> {
    formula: &'a str,
    rolls: &'a [u64],
    modifier: i64,
    total: i64,
    min: i64,
    max: i64,
}

pub(in crate::application::services::agent_tools) async fn roll(
    call: &AgentToolCall,
) -> Result<(AgentToolResult, AgentToolEffect), ApplicationError> {
    let Some(args) = object_args(call) else {
        return Ok((
            tool_error(
                call,
                "tool.invalid_arguments",
                "arguments must be an object",
            ),
            AgentToolEffect::None,
        ));
    };
    let Some(formula) = required_trimmed_string_arg(args, "formula") else {
        return Ok((
            tool_error(call, "tool.invalid_arguments", "formula is required"),
            AgentToolEffect::None,
        ));
    };
    let formula = match parse_formula(formula) {
        Ok(formula) => formula,
        Err(message) => {
            return Ok((
                tool_error(call, "dice.invalid_formula", &message),
                AgentToolEffect::None,
            ));
        }
    };

    let mut rng = rand::rng();
    let rolls = (0..formula.dice)
        .map(|_| rng.random_range(1..=formula.sides))
        .collect::<Vec<_>>();
    let total = rolls.iter().map(|roll| *roll as i64).sum::<i64>() + formula.modifier;
    let content = render_content(&formula, &rolls, total);

    Ok((
        AgentToolResult {
            call_id: call.id.clone(),
            name: call.name.clone(),
            content,
            structured: structured_value(DiceRollStructured {
                formula: formula.normalized.as_str(),
                rolls: &rolls,
                modifier: formula.modifier,
                total,
                min: formula.min,
                max: formula.max,
            }),
            is_error: false,
            error_code: None,
            resource_refs: Vec::new(),
        },
        AgentToolEffect::None,
    ))
}

fn parse_formula(raw: &str) -> Result<DiceFormula, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("formula is required".to_string());
    }
    if raw.chars().any(char::is_whitespace) {
        return Err("formula must not contain spaces".to_string());
    }

    let formula = if raw.bytes().all(|byte| byte.is_ascii_digit()) {
        format!("1d{raw}")
    } else {
        raw.to_ascii_lowercase()
    };
    let Some(d_index) = formula.find('d') else {
        return Err("formula must use dice notation such as 1d20 or 3d6+2".to_string());
    };
    if formula[d_index + 1..].contains('d') {
        return Err("formula must contain only one d separator".to_string());
    }

    let dice_part = &formula[..d_index];
    let after_d = &formula[d_index + 1..];
    let sign_index = after_d
        .bytes()
        .position(|byte| byte == b'+' || byte == b'-');
    let (sides_part, modifier_part) = match sign_index {
        Some(index) => (&after_d[..index], Some(&after_d[index..])),
        None => (after_d, None),
    };

    let dice = if dice_part.is_empty() {
        1
    } else {
        parse_positive_usize(dice_part, "dice count")?
    };
    let sides = parse_positive_u64(sides_part, "side count")?;
    let modifier = modifier_part.map(parse_modifier).transpose()?.unwrap_or(0);

    if dice > MAX_DICE {
        return Err(format!("dice count must be <= {MAX_DICE}"));
    }
    if sides > MAX_SIDES {
        return Err(format!("side count must be <= {MAX_SIDES}"));
    }
    if modifier.abs() > MAX_ABS_MODIFIER {
        return Err(format!(
            "modifier absolute value must be <= {MAX_ABS_MODIFIER}"
        ));
    }

    let min = dice as i64 + modifier;
    let max = dice as i64 * sides as i64 + modifier;
    Ok(DiceFormula {
        normalized: normalize_formula(dice, sides, modifier),
        dice,
        sides,
        modifier,
        min,
        max,
    })
}

fn parse_positive_usize(value: &str, label: &str) -> Result<usize, String> {
    if value.is_empty()
        || !value.bytes().all(|byte| byte.is_ascii_digit())
        || value.as_bytes()[0] == b'0'
    {
        return Err(format!("{label} must be a positive integer"));
    }
    value
        .parse::<usize>()
        .map_err(|_| format!("{label} is too large"))
}

fn parse_positive_u64(value: &str, label: &str) -> Result<u64, String> {
    if value.is_empty()
        || !value.bytes().all(|byte| byte.is_ascii_digit())
        || value.as_bytes()[0] == b'0'
    {
        return Err(format!("{label} must be a positive integer"));
    }
    value
        .parse::<u64>()
        .map_err(|_| format!("{label} is too large"))
}

fn parse_modifier(value: &str) -> Result<i64, String> {
    let Some(sign) = value.as_bytes().first().copied() else {
        return Err("modifier must start with + or -".to_string());
    };
    if sign != b'+' && sign != b'-' {
        return Err("modifier must start with + or -".to_string());
    }
    let digits = &value[1..];
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("modifier must be an integer".to_string());
    }
    let parsed = digits
        .parse::<i64>()
        .map_err(|_| "modifier is too large".to_string())?;
    Ok(if sign == b'-' { -parsed } else { parsed })
}

fn normalize_formula(dice: usize, sides: u64, modifier: i64) -> String {
    if modifier > 0 {
        format!("{dice}d{sides}+{modifier}")
    } else if modifier < 0 {
        format!("{dice}d{sides}{modifier}")
    } else {
        format!("{dice}d{sides}")
    }
}

fn render_content(formula: &DiceFormula, rolls: &[u64], total: i64) -> String {
    let mut expression = rolls
        .iter()
        .map(u64::to_string)
        .collect::<Vec<_>>()
        .join(" + ");
    if formula.modifier > 0 {
        expression.push_str(&format!(" + {}", formula.modifier));
    } else if formula.modifier < 0 {
        expression.push_str(&format!(" - {}", formula.modifier.abs()));
    }
    if expression == total.to_string() {
        format!("Rolled {}: {}.", formula.normalized, total)
    } else {
        format!("Rolled {}: {} = {}.", formula.normalized, expression, total)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{MAX_DICE, MAX_SIDES, parse_formula, render_content};
    use crate::domain::models::agent::AgentToolCall;

    #[test]
    fn parse_plain_number_as_single_die() {
        let formula = parse_formula("20").expect("plain number formula");

        assert_eq!(formula.normalized, "1d20");
        assert_eq!(formula.dice, 1);
        assert_eq!(formula.sides, 20);
        assert_eq!(formula.modifier, 0);
        assert_eq!(formula.min, 1);
        assert_eq!(formula.max, 20);
    }

    #[test]
    fn parse_dice_formula_with_default_count_and_modifier() {
        let formula = parse_formula("d6-2").expect("default count formula");

        assert_eq!(formula.normalized, "1d6-2");
        assert_eq!(formula.dice, 1);
        assert_eq!(formula.sides, 6);
        assert_eq!(formula.modifier, -2);
        assert_eq!(formula.min, -1);
        assert_eq!(formula.max, 4);
    }

    #[test]
    fn parse_dice_formula_with_explicit_count_and_modifier() {
        let formula = parse_formula("3D6+4").expect("explicit count formula");

        assert_eq!(formula.normalized, "3d6+4");
        assert_eq!(formula.dice, 3);
        assert_eq!(formula.sides, 6);
        assert_eq!(formula.modifier, 4);
        assert_eq!(formula.min, 7);
        assert_eq!(formula.max, 22);
    }

    #[test]
    fn reject_invalid_or_unbounded_formulas() {
        let too_many_dice = format!("{}d6", MAX_DICE + 1);
        let too_many_sides = format!("1d{}", MAX_SIDES + 1);
        for value in [
            "",
            "0d6",
            "1d0",
            "1d20 + 4",
            "1dd20",
            "1d20++4",
            "1d20+",
            "01d6",
            too_many_dice.as_str(),
            too_many_sides.as_str(),
        ] {
            assert!(parse_formula(value).is_err(), "{value}");
        }
    }

    #[test]
    fn render_content_matches_single_and_modified_rolls() {
        let simple = parse_formula("1d20").expect("simple formula");
        assert_eq!(render_content(&simple, &[14], 14), "Rolled 1d20: 14.");

        let modified = parse_formula("3d6+4").expect("modified formula");
        assert_eq!(
            render_content(&modified, &[2, 5, 6], 17),
            "Rolled 3d6+4: 2 + 5 + 6 + 4 = 17."
        );
    }

    #[tokio::test]
    async fn roll_tool_returns_structured_result() {
        let call = AgentToolCall {
            id: "call_dice".to_string(),
            name: "dice.roll".to_string(),
            arguments: json!({ "formula": "1d1+2" }),
            provider_metadata: Value::Null,
        };

        let (result, effect) = super::roll(&call).await.expect("dice roll result");

        assert!(matches!(
            effect,
            crate::application::services::agent_tools::AgentToolEffect::None
        ));
        assert!(!result.is_error);
        assert_eq!(result.content, "Rolled 1d1+2: 1 + 2 = 3.");
        assert_eq!(
            result.structured,
            json!({
                "formula": "1d1+2",
                "rolls": [1],
                "modifier": 2,
                "total": 3,
                "min": 3,
                "max": 3
            })
        );
    }
}
