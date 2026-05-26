use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

pub const LLM_CONNECTION_SCHEMA_VERSION: u32 = 1;
pub const LLM_CONNECTION_KIND: &str = "tauritavern.llmConnection";

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LlmConnectionId(String);

impl LlmConnectionId {
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, String> {
        let raw = raw.as_ref().trim();
        if raw.is_empty() {
            return Err("llm_connection.id_empty: connection id cannot be empty".to_string());
        }
        if raw.len() > 128 {
            return Err(
                "llm_connection.id_too_long: connection id must be <= 128 chars".to_string(),
            );
        }
        if !raw.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_'
        }) {
            return Err(
                "llm_connection.id_invalid: connection id must use lowercase ASCII, digits, '-' or '_'"
                    .to_string(),
            );
        }
        Ok(Self(raw.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Serialize for LlmConnectionId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for LlmConnectionId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LlmConnectionSummary {
    pub id: LlmConnectionId,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub chat_completion_source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_api_format: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LlmConnectionDefinition {
    pub schema_version: u32,
    pub kind: String,
    pub id: LlmConnectionId,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub provider: LlmConnectionProvider,
    #[serde(default)]
    pub endpoint: LlmConnectionEndpoint,
    pub auth: LlmConnectionAuth,
    #[serde(default)]
    pub routing: LlmConnectionRouting,
    #[serde(default)]
    pub adapter_hints: LlmConnectionAdapterHints,
    #[serde(default)]
    pub capabilities: LlmConnectionCapabilities,
}

impl LlmConnectionDefinition {
    pub fn summary(&self) -> LlmConnectionSummary {
        LlmConnectionSummary {
            id: self.id.clone(),
            display_name: self.display_name.clone(),
            description: self.description.clone(),
            chat_completion_source: self.provider.chat_completion_source.clone(),
            custom_api_format: self.provider.custom_api_format.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LlmConnectionProvider {
    pub chat_completion_source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_api_format: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LlmConnectionEndpoint {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub source_specific: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LlmConnectionAuth {
    pub secret_ref: LlmConnectionSecretRef,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LlmConnectionSecretRef {
    pub key: String,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_snapshot: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LlmConnectionRouting {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reverse_proxy: Option<LlmConnectionReverseProxy>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LlmConnectionReverseProxy {
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LlmConnectionAdapterHints {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_post_processing: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_include_headers: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_include_body: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_exclude_body: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LlmConnectionCapabilities {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub streaming: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calling: Option<String>,
}
