use std::{collections::BTreeMap, fmt};

use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

use crate::{errors::DomainError, models::tool::ToolProviderId};

const MCP_PROVIDER_PREFIX: &str = "mcp/";
const MAX_NATIVE_TOOL_NAME_BYTES: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct McpRegistrationId(String);

impl McpRegistrationId {
    pub fn generate() -> Self {
        Self(Uuid::new_v4().hyphenated().to_string())
    }

    pub fn parse(raw: impl AsRef<str>) -> Result<Self, DomainError> {
        let raw = raw.as_ref();
        let id = Uuid::parse_str(raw).map_err(|_| {
            DomainError::InvalidData(format!(
                "mcp.registration_id_invalid: `{raw}` is not a canonical UUID"
            ))
        })?;
        if raw != id.hyphenated().to_string() {
            return Err(DomainError::InvalidData(format!(
                "mcp.registration_id_noncanonical: `{raw}` must use lowercase hyphenated UUID form"
            )));
        }
        Ok(Self(raw.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn provider_id(&self) -> ToolProviderId {
        ToolProviderId::parse(format!("{MCP_PROVIDER_PREFIX}{self}"))
            .expect("canonical MCP registration IDs form valid provider IDs")
    }

    pub fn from_provider_id(provider_id: &str) -> Result<Self, DomainError> {
        let raw = provider_id
            .strip_prefix(MCP_PROVIDER_PREFIX)
            .ok_or_else(|| {
                DomainError::InvalidData(format!(
                    "mcp.provider_id_invalid: `{provider_id}` is not an MCP provider id"
                ))
            })?;
        Self::parse(raw)
    }
}

impl fmt::Display for McpRegistrationId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpEndpoint(String);

impl McpEndpoint {
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, DomainError> {
        let raw = raw.as_ref().trim();
        let url = Url::parse(raw)
            .map_err(|error| DomainError::InvalidData(format!("mcp.endpoint_invalid: {error}")))?;

        if url.cannot_be_a_base() || url.host_str().is_none() {
            return Err(DomainError::InvalidData(
                "mcp.endpoint_absolute_required: endpoint must be an absolute HTTP(S) URL"
                    .to_string(),
            ));
        }
        if url.fragment().is_some() {
            return Err(DomainError::InvalidData(
                "mcp.endpoint_fragment_forbidden: endpoint cannot contain a fragment".to_string(),
            ));
        }

        match url.scheme() {
            "http" | "https" => {}
            _ => {
                return Err(DomainError::InvalidData(
                    "mcp.endpoint_scheme_invalid: endpoint must use HTTP or HTTPS".to_string(),
                ));
            }
        }

        Ok(Self(url.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Default, PartialEq, Eq)]
pub struct McpRequestHeaders(BTreeMap<String, String>);

impl McpRequestHeaders {
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
    }

    pub fn as_map(&self) -> &BTreeMap<String, String> {
        &self.0
    }
}

impl From<BTreeMap<String, String>> for McpRequestHeaders {
    fn from(headers: BTreeMap<String, String>) -> Self {
        Self(headers)
    }
}

impl fmt::Debug for McpRequestHeaders {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_map()
            .entries(self.0.keys().map(|name| (name, "[redacted]")))
            .finish()
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum McpServerState {
    Active,
    Paused,
}

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum McpProtocolVersionPreference {
    #[default]
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "2026-07-28")]
    V2026_07_28,
    #[serde(rename = "2025-11-25")]
    V2025_11_25,
    #[serde(rename = "2025-06-18")]
    V2025_06_18,
    #[serde(rename = "2025-03-26")]
    V2025_03_26,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum McpToolPermission {
    Off,
    Ask,
    Allow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServerRegistration {
    id: McpRegistrationId,
    display_name: String,
    endpoint: McpEndpoint,
    request_headers: McpRequestHeaders,
    protocol_version: McpProtocolVersionPreference,
    state: McpServerState,
    tool_permissions: BTreeMap<String, McpToolPermission>,
}

impl McpServerRegistration {
    pub fn try_new(
        id: McpRegistrationId,
        display_name: impl Into<String>,
        endpoint: McpEndpoint,
        request_headers: BTreeMap<String, String>,
        protocol_version: McpProtocolVersionPreference,
        state: McpServerState,
        tool_permissions: BTreeMap<String, McpToolPermission>,
    ) -> Result<Self, DomainError> {
        let display_name = validate_display_name(display_name.into())?;
        let request_headers = McpRequestHeaders::from(request_headers);
        for (native_name, permission) in &tool_permissions {
            validate_native_tool_name(native_name)?;
            if *permission == McpToolPermission::Off {
                return Err(DomainError::InvalidData(format!(
                    "mcp.permission_off_not_persisted: `{native_name}` must be omitted instead of storing Off"
                )));
            }
        }
        Ok(Self {
            id,
            display_name,
            endpoint,
            request_headers,
            protocol_version,
            state,
            tool_permissions,
        })
    }

    pub fn new_paused(
        display_name: impl Into<String>,
        endpoint: McpEndpoint,
        request_headers: BTreeMap<String, String>,
        protocol_version: McpProtocolVersionPreference,
    ) -> Result<Self, DomainError> {
        Self::try_new(
            McpRegistrationId::generate(),
            display_name,
            endpoint,
            request_headers,
            protocol_version,
            McpServerState::Paused,
            BTreeMap::new(),
        )
    }

    pub fn id(&self) -> &McpRegistrationId {
        &self.id
    }

    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    pub fn endpoint(&self) -> &McpEndpoint {
        &self.endpoint
    }

    pub fn request_headers(&self) -> &McpRequestHeaders {
        &self.request_headers
    }

    pub fn protocol_version(&self) -> McpProtocolVersionPreference {
        self.protocol_version
    }

    pub fn state(&self) -> McpServerState {
        self.state
    }

    pub fn tool_permissions(&self) -> &BTreeMap<String, McpToolPermission> {
        &self.tool_permissions
    }

    pub fn update(
        &mut self,
        display_name: impl Into<String>,
        endpoint: McpEndpoint,
        request_headers: BTreeMap<String, String>,
        protocol_version: McpProtocolVersionPreference,
    ) -> Result<bool, DomainError> {
        let display_name = validate_display_name(display_name.into())?;
        let request_headers = McpRequestHeaders::from(request_headers);
        let connection_changed = self.endpoint != endpoint
            || self.request_headers != request_headers
            || self.protocol_version != protocol_version;
        self.display_name = display_name;
        self.endpoint = endpoint;
        self.request_headers = request_headers;
        self.protocol_version = protocol_version;
        Ok(connection_changed)
    }

    pub fn set_state(&mut self, state: McpServerState) {
        self.state = state;
    }

    pub fn permission_for(&self, native_name: &str) -> McpToolPermission {
        self.tool_permissions
            .get(native_name)
            .copied()
            .unwrap_or(McpToolPermission::Off)
    }

    pub fn set_tool_permission(
        &mut self,
        native_name: impl Into<String>,
        permission: McpToolPermission,
    ) -> Result<(), DomainError> {
        let native_name = native_name.into();
        validate_native_tool_name(&native_name)?;
        match permission {
            McpToolPermission::Off => {
                self.tool_permissions.remove(&native_name);
            }
            McpToolPermission::Ask | McpToolPermission::Allow => {
                self.tool_permissions.insert(native_name, permission);
            }
        }
        Ok(())
    }
}

fn validate_display_name(raw: String) -> Result<String, DomainError> {
    let value = raw.trim();
    if value.is_empty() {
        return Err(DomainError::InvalidData(
            "mcp.display_name_empty: display name cannot be empty".to_string(),
        ));
    }
    Ok(value.to_string())
}

pub fn validate_native_tool_name(native_name: &str) -> Result<(), DomainError> {
    if native_name.is_empty() {
        return Err(DomainError::InvalidData(
            "mcp.tool_name_empty: native tool name cannot be empty".to_string(),
        ));
    }
    if native_name.len() > MAX_NATIVE_TOOL_NAME_BYTES {
        return Err(DomainError::InvalidData(format!(
            "mcp.tool_name_too_long: native tool name must be at most {MAX_NATIVE_TOOL_NAME_BYTES} bytes"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::tool::ToolId;

    #[test]
    fn registration_id_is_canonical_and_forms_the_existing_tool_identity() {
        let id = McpRegistrationId::parse("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let tool_id = ToolId::new(&id.provider_id(), "search:exact").unwrap();

        assert_eq!(
            tool_id.as_str(),
            "mcp/550e8400-e29b-41d4-a716-446655440000:search:exact"
        );
        assert_eq!(
            McpRegistrationId::from_provider_id(tool_id.provider_id()).unwrap(),
            id
        );
        assert!(McpRegistrationId::parse("550E8400-E29B-41D4-A716-446655440000").is_err());
    }

    #[test]
    fn endpoint_preserves_user_selected_http_configuration() {
        assert_eq!(
            McpEndpoint::parse("https://user:pass@example.com/mcp?tenant=one")
                .unwrap()
                .as_str(),
            "https://user:pass@example.com/mcp?tenant=one"
        );
        for endpoint in [
            "http://127.0.0.1:3000/mcp",
            "http://example.com/mcp",
            "http://8.8.8.8/mcp?token=x",
            "https://example.com/mcp",
        ] {
            assert!(McpEndpoint::parse(endpoint).is_ok(), "{endpoint}");
        }
        assert!(McpEndpoint::parse("file:///tmp/mcp").is_err());
        assert!(McpEndpoint::parse("https://example.com/mcp#fragment").is_err());
    }

    #[test]
    fn off_permissions_are_absent_and_new_registrations_are_paused() {
        let mut registration = McpServerRegistration::new_paused(
            " Local tools ",
            McpEndpoint::parse("http://localhost:3000/mcp").unwrap(),
            BTreeMap::new(),
            McpProtocolVersionPreference::Auto,
        )
        .unwrap();

        assert_eq!(registration.display_name(), "Local tools");
        assert_eq!(registration.state(), McpServerState::Paused);
        registration
            .set_tool_permission("search", McpToolPermission::Allow)
            .unwrap();
        assert_eq!(
            registration.permission_for("search"),
            McpToolPermission::Allow
        );
        registration
            .set_tool_permission("search", McpToolPermission::Off)
            .unwrap();
        assert!(registration.tool_permissions().is_empty());
    }

    #[test]
    fn request_headers_preserve_user_input_and_redact_debug() {
        let headers = McpRequestHeaders::from(BTreeMap::from([
            (" X-API-Key ".to_string(), "secret".to_string()),
            ("Authorization".to_string(), "Bearer token".to_string()),
            ("Mcp-Session-Id".to_string(), "line\nbreak".to_string()),
        ]));

        assert_eq!(
            headers.iter().collect::<Vec<_>>(),
            [
                (" X-API-Key ", "secret"),
                ("Authorization", "Bearer token"),
                ("Mcp-Session-Id", "line\nbreak"),
            ]
        );
        let debug = format!("{headers:?}");
        assert!(!debug.contains("secret"));
        assert!(!debug.contains("Bearer token"));
        let many = (0..64)
            .map(|index| (format!("x-custom-{index}"), index.to_string()))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(McpRequestHeaders::from(many).as_map().len(), 64);
        assert_eq!(
            validate_display_name("n".repeat(512))
                .unwrap()
                .chars()
                .count(),
            512
        );
    }
}
