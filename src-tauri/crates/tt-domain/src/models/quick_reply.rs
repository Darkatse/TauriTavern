use crate::errors::DomainError;
use crate::json_document::JsonDocument;

/// A Quick Reply set, stored under the name its document carries.
#[derive(Debug, Clone)]
pub struct QuickReplySet {
    pub name: String,
    pub data: JsonDocument,
}

impl QuickReplySet {
    pub fn parse(json: &str) -> Result<Self, DomainError> {
        let data = JsonDocument::parse(json.as_bytes()).map_err(invalid_set)?;
        match data.get::<String>(&["name"]).map_err(invalid_set)? {
            Some(name) if !name.trim().is_empty() => Ok(Self { name, data }),
            _ => Err(DomainError::InvalidData(
                "Quick Reply set name is required".to_string(),
            )),
        }
    }
}

fn invalid_set(error: serde_json::Error) -> DomainError {
    DomainError::InvalidData(format!("Invalid Quick Reply set: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_rejects_empty_name() {
        assert!(QuickReplySet::parse(r#"{"name":" "}"#).is_err());
    }

    #[test]
    fn parse_rejects_non_object_payload() {
        assert!(QuickReplySet::parse(r#""not-an-object""#).is_err());
    }
}
