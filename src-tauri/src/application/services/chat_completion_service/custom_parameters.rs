use std::collections::{HashMap, HashSet};

use serde_json::{Map, Value};

use crate::application::errors::ApplicationError;

pub(super) fn parse_string_map(raw: &str) -> Result<HashMap<String, String>, ApplicationError> {
    let object = parse_object(raw)?;
    let mut result = HashMap::new();

    for (key, value) in object {
        let key = key.trim();
        if key.is_empty() || value.is_null() {
            continue;
        }

        let mapped = value
            .as_str()
            .map(str::trim)
            .map(str::to_string)
            .unwrap_or_else(|| value.to_string());

        if !mapped.is_empty() {
            result.insert(key.to_string(), mapped);
        }
    }

    Ok(result)
}

pub(super) fn parse_object(raw: &str) -> Result<Map<String, Value>, ApplicationError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(Map::new());
    }

    if let Ok(value) = serde_json::from_str::<Value>(raw) {
        if let Some(object) = value.as_object() {
            return Ok(object.clone());
        }
    }

    let mut object = Map::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let line = line.trim_start_matches('-').trim();
        let Some((raw_key, raw_value)) = line.split_once(':') else {
            continue;
        };

        let key = raw_key.trim();
        if key.is_empty() {
            continue;
        }

        object.insert(key.to_string(), parse_scalar(raw_value.trim()));
    }

    if object.is_empty() {
        return Err(ApplicationError::ValidationError(
            "Failed to parse custom parameter map. Use JSON object or line-based key:value format."
                .to_string(),
        ));
    }

    Ok(object)
}

pub(super) fn parse_key_list(raw: &str) -> Vec<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Vec::new();
    }

    if let Ok(value) = serde_json::from_str::<Value>(raw) {
        match value {
            Value::Array(entries) => {
                return entries
                    .into_iter()
                    .filter_map(|entry| entry.as_str().map(str::trim).map(str::to_string))
                    .filter(|entry| !entry.is_empty())
                    .collect();
            }
            Value::String(entry) => {
                let entry = entry.trim();
                if !entry.is_empty() {
                    return vec![entry.to_string()];
                }
                return Vec::new();
            }
            _ => {}
        }
    }

    let mut keys = HashSet::new();
    for token in raw.split([',', '\n']) {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }

        let token = token.trim_start_matches('-').trim();
        let key = token.split_once(':').map(|(left, _)| left).unwrap_or(token);
        let key = key.trim();
        if !key.is_empty() {
            keys.insert(key.to_string());
        }
    }

    keys.into_iter().collect()
}

fn parse_scalar(raw: &str) -> Value {
    let trimmed = raw.trim();
    if trimmed.eq_ignore_ascii_case("true") {
        return Value::Bool(true);
    }

    if trimmed.eq_ignore_ascii_case("false") {
        return Value::Bool(false);
    }

    if trimmed.eq_ignore_ascii_case("null") {
        return Value::Null;
    }

    if let Ok(integer) = trimmed.parse::<i64>() {
        return Value::Number(integer.into());
    }

    if let Ok(float_number) = trimmed.parse::<f64>() {
        if let Some(number) = serde_json::Number::from_f64(float_number) {
            return Value::Number(number);
        }
    }

    let unquoted = trimmed
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            trimmed
                .strip_prefix('"')
                .and_then(|value| value.strip_suffix('\''))
        })
        .or_else(|| {
            trimmed
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
        .or_else(|| {
            trimmed
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('"'))
        })
        .unwrap_or(trimmed);

    Value::String(unquoted.to_string())
}

#[cfg(test)]
mod tests {
    use super::{parse_key_list, parse_object, parse_string_map};

    #[test]
    fn parse_string_map_supports_json_object() {
        let result =
            parse_string_map(r#"{"x-api-key":"abc","x-int":123}"#).expect("should parse JSON");

        assert_eq!(result.get("x-api-key"), Some(&"abc".to_string()));
        assert_eq!(result.get("x-int"), Some(&"123".to_string()));
    }

    #[test]
    fn parse_object_supports_line_based_format() {
        let result = parse_object("x-api-key: abc\nx-enabled: true").expect("should parse lines");

        assert_eq!(
            result
                .get("x-api-key")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default(),
            "abc"
        );
        assert_eq!(
            result
                .get("x-enabled")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            true
        );
    }

    #[test]
    fn parse_key_list_supports_json_array_and_csv() {
        let from_json = parse_key_list(r#"["a","b"]"#);
        assert!(from_json.contains(&"a".to_string()));
        assert!(from_json.contains(&"b".to_string()));

        let from_csv = parse_key_list("foo, bar\nbaz");
        assert!(from_csv.contains(&"foo".to_string()));
        assert!(from_csv.contains(&"bar".to_string()));
        assert!(from_csv.contains(&"baz".to_string()));
    }

    #[test]
    fn parse_object_returns_error_for_invalid_non_empty_input() {
        let result = parse_object("not-a-map-format");
        assert!(result.is_err());
    }
}
