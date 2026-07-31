use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

use crate::errors::DomainError;

const TOOL_ID_SEPARATOR: char = ':';
const BUILTIN_TOOL_PROVIDER_ID: &str = "builtin";

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ToolProviderId(String);

impl ToolProviderId {
    pub fn parse(raw: impl Into<String>) -> Result<Self, DomainError> {
        let raw = raw.into();
        if raw.is_empty() {
            return Err(DomainError::InvalidData(
                "tool.provider_id_empty: tool provider id cannot be empty".to_string(),
            ));
        }
        if raw.contains(TOOL_ID_SEPARATOR) {
            return Err(DomainError::InvalidData(format!(
                "tool.provider_id_invalid: tool provider id `{raw}` cannot contain `{TOOL_ID_SEPARATOR}`"
            )));
        }

        Ok(Self(raw))
    }

    pub fn builtin() -> Self {
        Self(BUILTIN_TOOL_PROVIDER_ID.to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ToolProviderId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Serialize for ToolProviderId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ToolProviderId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::parse(String::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ToolId(String);

impl ToolId {
    pub fn new(
        provider_id: &ToolProviderId,
        native_name: impl AsRef<str>,
    ) -> Result<Self, DomainError> {
        let native_name = native_name.as_ref();
        if native_name.is_empty() {
            return Err(DomainError::InvalidData(
                "tool.native_name_empty: tool native name cannot be empty".to_string(),
            ));
        }

        Ok(Self(format!(
            "{}{TOOL_ID_SEPARATOR}{native_name}",
            provider_id.as_str()
        )))
    }

    pub fn parse(raw: impl Into<String>) -> Result<Self, DomainError> {
        let raw = raw.into();
        let (provider_id, native_name) = raw.split_once(TOOL_ID_SEPARATOR).ok_or_else(|| {
            DomainError::InvalidData(format!(
                "tool.id_invalid: tool id `{raw}` must contain a provider and native name"
            ))
        })?;
        let provider_id = ToolProviderId::parse(provider_id.to_string())?;
        Self::new(&provider_id, native_name)
    }

    pub fn builtin(native_name: impl AsRef<str>) -> Result<Self, DomainError> {
        Self::new(&ToolProviderId::builtin(), native_name)
    }

    pub fn provider_id(&self) -> &str {
        self.0
            .split_once(TOOL_ID_SEPARATOR)
            .expect("ToolId constructor guarantees a separator")
            .0
    }

    pub fn native_name(&self) -> &str {
        self.0
            .split_once(TOOL_ID_SEPARATOR)
            .expect("ToolId constructor guarantees a separator")
            .1
    }

    pub fn is_builtin(&self) -> bool {
        self.provider_id() == BUILTIN_TOOL_PROVIDER_ID
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ToolId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Serialize for ToolId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ToolId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::parse(String::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolChoice {
    None,
    Auto,
    Required,
    Specific(ToolId),
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{ToolChoice, ToolId, ToolProviderId};

    #[test]
    fn tool_identity_is_stable_and_opaque() {
        let builtin = ToolProviderId::builtin();
        let mcp = ToolProviderId::parse("mcp/registration-1").unwrap();
        let builtin_id = ToolId::new(&builtin, "workspace.read_file").unwrap();
        let mcp_id = ToolId::new(&mcp, "workspace.read_file").unwrap();

        assert_eq!(builtin_id.as_str(), "builtin:workspace.read_file");
        assert_eq!(builtin_id.provider_id(), "builtin");
        assert_eq!(builtin_id.native_name(), "workspace.read_file");
        assert_ne!(builtin_id, mcp_id);
    }

    #[test]
    fn tool_identity_rejects_invalid_serialized_values() {
        for invalid in ["", "builtin", ":tool", "builtin:"] {
            assert!(serde_json::from_value::<ToolId>(json!(invalid)).is_err());
        }
        assert!(ToolProviderId::parse("mcp:registration-1").is_err());
    }

    #[test]
    fn tool_choice_uses_canonical_domain_shape() {
        let choice = ToolChoice::Specific(ToolId::builtin("workspace.finish").unwrap());
        let value = serde_json::to_value(&choice).unwrap();

        assert_eq!(value, json!({ "specific": "builtin:workspace.finish" }));
        assert_eq!(serde_json::from_value::<ToolChoice>(value).unwrap(), choice);
    }
}
