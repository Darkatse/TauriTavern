use crate::errors::DomainError;
use crate::json_document::JsonDocument;

/// A UI theme, stored under the name its document carries.
#[derive(Debug, Clone)]
pub struct Theme {
    pub name: String,
    pub data: JsonDocument,
}

impl Theme {
    pub fn parse(json: &str) -> Result<Self, DomainError> {
        let data = JsonDocument::parse(json.as_bytes()).map_err(invalid_theme)?;
        match data.get::<String>(&["name"]).map_err(invalid_theme)? {
            Some(name) if !name.is_empty() => Ok(Self { name, data }),
            _ => Err(DomainError::InvalidData(
                "Theme name is required".to_string(),
            )),
        }
    }
}

fn invalid_theme(error: serde_json::Error) -> DomainError {
    DomainError::InvalidData(format!("Invalid theme: {error}"))
}
