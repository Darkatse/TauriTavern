use serde::{Deserialize, Serialize};
use serde_json::Value;

/// World Info (Lorebook) document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldInfo {
    /// Logical lorebook name (filename stem).
    pub name: String,
    /// Raw lorebook payload.
    pub data: Value,
}

impl WorldInfo {
    pub fn new(name: String, data: Value) -> Self {
        Self { name, data }
    }

    pub fn file_stem(&self) -> String {
        sanitize_world_info_name(&self.name)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.file_stem().is_empty() {
            return Err("World file must have a name".to_string());
        }

        validate_world_info_data(&self.data)
    }
}

/// Sanitize world info name for filesystem compatibility.
pub fn sanitize_world_info_name(name: &str) -> String {
    let sanitized = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0' => '_',
            c if c.is_control() => '_',
            _ => c,
        })
        .collect::<String>();

    sanitized
        .trim_matches(|c: char| c.is_whitespace() || c == '.')
        .to_string()
}

/// Validate lorebook payload.
pub fn validate_world_info_data(data: &Value) -> Result<(), String> {
    if !data.is_object() {
        return Err("Is not a valid world info file".to_string());
    }

    if data.get("entries").is_none() {
        return Err("World info must contain an entries list".to_string());
    }

    Ok(())
}
