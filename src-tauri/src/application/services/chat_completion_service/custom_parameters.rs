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

    let value = serde_yaml::from_str::<Value>(raw).map_err(|error| {
        ApplicationError::ValidationError(format!(
            "Failed to parse custom parameter map as YAML/JSON: {error}"
        ))
    })?;

    match value {
        Value::Object(object) => Ok(object),
        Value::Array(entries) => {
            let mut merged = Map::new();
            for entry in entries {
                match entry {
                    Value::Object(object) => {
                        for (key, value) in object {
                            merged.insert(key, value);
                        }
                    }
                    Value::Null => continue,
                    _ => {
                        return Err(ApplicationError::ValidationError(
                            "Custom parameter map must be an object or a list of objects."
                                .to_string(),
                        ))
                    }
                }
            }

            Ok(merged)
        }
        Value::Null => Ok(Map::new()),
        _ => Err(ApplicationError::ValidationError(
            "Custom parameter map must be an object or a list of objects.".to_string(),
        )),
    }
}

pub(super) fn parse_key_list(raw: &str) -> Result<Vec<String>, ApplicationError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(Vec::new());
    }

    let value = serde_yaml::from_str::<Value>(raw).map_err(|error| {
        ApplicationError::ValidationError(format!(
            "Failed to parse custom exclude list as YAML/JSON: {error}"
        ))
    })?;

    match value {
        Value::Array(entries) => {
            let mut keys = HashSet::new();
            for entry in entries {
                match entry {
                    Value::String(key) => {
                        let key = key.trim();
                        if !key.is_empty() {
                            keys.insert(key.to_string());
                        }
                    }
                    Value::Null => continue,
                    _ => {
                        return Err(ApplicationError::ValidationError(
                            "Custom exclude list must be an array of strings, an object, or a string."
                                .to_string(),
                        ))
                    }
                }
            }
            Ok(keys.into_iter().collect())
        }
        Value::Object(object) => Ok(object
            .keys()
            .map(|key| key.trim().to_string())
            .filter(|key| !key.is_empty())
            .collect()),
        Value::String(key) => {
            let key = key.trim();
            if key.is_empty() {
                Ok(Vec::new())
            } else {
                Ok(vec![key.to_string()])
            }
        }
        Value::Null => Ok(Vec::new()),
        _ => Err(ApplicationError::ValidationError(
            "Custom exclude list must be an array of strings, an object, or a string.".to_string(),
        )),
    }
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
    fn parse_object_supports_yaml_format() {
        let result = parse_object("x-api-key: abc\nx-enabled: true").expect("should parse YAML");

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
    fn parse_key_list_supports_json_array() {
        let from_json = parse_key_list(r#"["a","b"]"#).expect("should parse JSON array");
        assert!(from_json.contains(&"a".to_string()));
        assert!(from_json.contains(&"b".to_string()));
    }

    #[test]
    fn parse_object_returns_error_for_invalid_non_empty_input() {
        let result = parse_object("not-a-map-format");
        assert!(result.is_err());
    }
}
