use std::{collections::BTreeMap, fmt};

use serde::{Deserialize, Serialize};
use url::{Host, Url};
use uuid::Uuid;

use crate::{errors::DomainError, models::tool::ToolProviderId};

const MCP_PROVIDER_PREFIX: &str = "mcp/";
const MAX_DISPLAY_NAME_CHARS: usize = 128;
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
        if !url.username().is_empty() || url.password().is_some() {
            return Err(DomainError::InvalidData(
                "mcp.endpoint_userinfo_forbidden: endpoint cannot contain credentials".to_string(),
            ));
        }
        if url.query().is_some() || url.fragment().is_some() {
            return Err(DomainError::InvalidData(
                "mcp.endpoint_suffix_forbidden: endpoint cannot contain a query or fragment"
                    .to_string(),
            ));
        }

        match url.scheme() {
            "https" => {}
            "http" if url.host().is_some_and(is_local_network_host) => {}
            "http" => {
                return Err(DomainError::InvalidData(
                    "mcp.endpoint_insecure_http: HTTP is only allowed for local or private network endpoints"
                        .to_string(),
                ));
            }
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

fn is_local_network_host(host: Host<&str>) -> bool {
    match host {
        Host::Domain(host) => {
            let host = host.trim_end_matches('.').to_ascii_lowercase();
            !host.contains('.')
                || host == "localhost"
                || host.ends_with(".localhost")
                || host.ends_with(".local")
                || host == "home.arpa"
                || host.ends_with(".home.arpa")
        }
        Host::Ipv4(address) => {
            let octets = address.octets();
            address.is_loopback()
                || address.is_private()
                || address.is_link_local()
                || (octets[0] == 100 && (64..=127).contains(&octets[1]))
        }
        Host::Ipv6(address) => {
            address.is_loopback() || address.is_unique_local() || address.is_unicast_link_local()
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum McpServerState {
    Active,
    Paused,
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
    state: McpServerState,
    tool_permissions: BTreeMap<String, McpToolPermission>,
}

impl McpServerRegistration {
    pub fn try_new(
        id: McpRegistrationId,
        display_name: impl Into<String>,
        endpoint: McpEndpoint,
        state: McpServerState,
        tool_permissions: BTreeMap<String, McpToolPermission>,
    ) -> Result<Self, DomainError> {
        let display_name = validate_display_name(display_name.into())?;
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
            state,
            tool_permissions,
        })
    }

    pub fn new_paused(
        display_name: impl Into<String>,
        endpoint: McpEndpoint,
    ) -> Result<Self, DomainError> {
        Self::try_new(
            McpRegistrationId::generate(),
            display_name,
            endpoint,
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

    pub fn state(&self) -> McpServerState {
        self.state
    }

    pub fn tool_permissions(&self) -> &BTreeMap<String, McpToolPermission> {
        &self.tool_permissions
    }

    pub fn rename(&mut self, display_name: impl Into<String>) -> Result<(), DomainError> {
        self.display_name = validate_display_name(display_name.into())?;
        Ok(())
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
    if value.chars().count() > MAX_DISPLAY_NAME_CHARS {
        return Err(DomainError::InvalidData(format!(
            "mcp.display_name_too_long: display name must be at most {MAX_DISPLAY_NAME_CHARS} characters"
        )));
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
        assert!(McpRegistrationId::parse("550E8400-E29B-41D4-A716-446655440000").is_err());
    }

    #[test]
    fn endpoint_accepts_https_and_local_network_http_only() {
        assert_eq!(
            McpEndpoint::parse("https://example.com/mcp")
                .unwrap()
                .as_str(),
            "https://example.com/mcp"
        );
        for endpoint in [
            "http://127.0.0.1:3000/mcp",
            "http://192.168.1.20:3000/mcp",
            "http://10.0.0.20:3000/mcp",
            "http://172.16.0.20:3000/mcp",
            "http://169.254.1.20:3000/mcp",
            "http://100.64.0.20:3000/mcp",
            "http://[::1]:3000/mcp",
            "http://[fd00::20]:3000/mcp",
            "http://nas.local:3000/mcp",
            "http://mcp.home.arpa:3000/mcp",
            "http://mcp-server:3000/mcp",
        ] {
            assert!(McpEndpoint::parse(endpoint).is_ok(), "{endpoint}");
        }
        for endpoint in [
            "http://example.com/mcp",
            "http://8.8.8.8/mcp",
            "http://172.15.255.255/mcp",
            "http://172.32.0.1/mcp",
            "http://100.128.0.1/mcp",
            "http://[2001:4860:4860::8888]/mcp",
        ] {
            assert!(McpEndpoint::parse(endpoint).is_err(), "{endpoint}");
        }
        assert!(McpEndpoint::parse("https://user@example.com/mcp").is_err());
        assert!(McpEndpoint::parse("https://example.com/mcp?token=x").is_err());
    }

    #[test]
    fn off_permissions_are_absent_and_new_registrations_are_paused() {
        let mut registration = McpServerRegistration::new_paused(
            " Local tools ",
            McpEndpoint::parse("http://localhost:3000/mcp").unwrap(),
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
}
