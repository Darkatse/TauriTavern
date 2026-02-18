use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Represents a Quick Reply set payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuickReplySet {
    /// Quick Reply set name (also used as filename stem).
    pub name: String,
    /// Raw Quick Reply set JSON payload.
    pub data: Value,
}

impl QuickReplySet {
    pub fn new(name: String, data: Value) -> Self {
        Self { name, data }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("Quick Reply set name cannot be empty".to_string());
        }

        if !self.data.is_object() {
            return Err("Quick Reply payload must be a JSON object".to_string());
        }

        Ok(())
    }
}

/// Sanitize a filename to ensure it's safe to write to disk.
pub fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|character| match character {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            control if control.is_control() => '_',
            other => other,
        })
        .collect::<String>()
        .trim()
        .trim_end_matches(['.', ' '])
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn validate_rejects_empty_name() {
        let set = QuickReplySet::new("".to_string(), json!({}));
        assert!(set.validate().is_err());
    }

    #[test]
    fn validate_rejects_non_object_payload() {
        let set = QuickReplySet::new("Default".to_string(), json!("not-an-object"));
        assert!(set.validate().is_err());
    }

    #[test]
    fn sanitize_filename_replaces_invalid_chars() {
        assert_eq!(
            sanitize_filename("name/with\\unsafe:chars*?"),
            "name_with_unsafe_chars__"
        );
    }
}
