use std::collections::HashMap;

use crate::errors::ApplicationError;
use tt_domain::models::{
    mcp::{McpRegistrationId, McpServerState, McpToolPermission},
    tool::{ToolDescriptor, ToolId},
};

use super::McpService;

#[derive(Debug, Clone)]
pub(crate) struct AgentMcpTool {
    pub registration_id: McpRegistrationId,
    pub server_display_name: String,
    pub descriptor: ToolDescriptor,
    pub permission: McpToolPermission,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentMcpToolDiagnostic {
    pub tool_id: Option<ToolId>,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct AgentMcpToolResolution {
    pub tools: Vec<AgentMcpTool>,
    pub diagnostics: Vec<AgentMcpToolDiagnostic>,
}

impl McpService {
    pub(crate) async fn list_agent_tools_cached(
        &self,
    ) -> Result<AgentMcpToolResolution, ApplicationError> {
        let scan = self.repository.scan().await?;
        let mut resolution = AgentMcpToolResolution::default();
        resolution
            .diagnostics
            .extend(scan.issues.into_iter().map(|issue| AgentMcpToolDiagnostic {
                tool_id: None,
                code: "mcp.registration_storage_issue".to_string(),
                message: format!(
                    "MCP registration file `{}` could not be loaded: {}",
                    issue.file_name, issue.message
                ),
            }));
        for registration in scan.registrations {
            if registration.state() != McpServerState::Active {
                continue;
            }
            let Some(snapshot) = self.cached_catalog(&registration).await? else {
                resolution.diagnostics.push(AgentMcpToolDiagnostic {
                    tool_id: None,
                    code: "mcp.catalog_not_cached".to_string(),
                    message: format!(
                        "MCP server `{}` has no cached tool catalog; refresh it in MCP Manager",
                        registration.display_name()
                    ),
                });
                continue;
            };
            for descriptor in snapshot.catalog.iter() {
                let permission = registration.permission_for(descriptor.id.native_name());
                if permission == McpToolPermission::Off {
                    continue;
                }
                if let Err(message) = validate_agent_input_schema(descriptor) {
                    resolution.diagnostics.push(AgentMcpToolDiagnostic {
                        tool_id: Some(descriptor.id.clone()),
                        code: "mcp.agent_input_schema_unsupported".to_string(),
                        message,
                    });
                    continue;
                }
                resolution.tools.push(AgentMcpTool {
                    registration_id: registration.id().clone(),
                    server_display_name: registration.display_name().to_string(),
                    descriptor: descriptor.clone(),
                    permission,
                });
            }
        }
        resolution.tools.sort_by(|left, right| {
            left.server_display_name
                .to_lowercase()
                .cmp(&right.server_display_name.to_lowercase())
                .then_with(|| left.descriptor.id.cmp(&right.descriptor.id))
        });
        Ok(resolution)
    }

    pub(crate) async fn resolve_agent_tools_cached(
        &self,
        selected: &[ToolId],
    ) -> Result<AgentMcpToolResolution, ApplicationError> {
        if selected.is_empty() {
            return Ok(AgentMcpToolResolution::default());
        }
        let scan = self.repository.scan().await?;
        let storage_issues = scan
            .issues
            .into_iter()
            .filter_map(|issue| issue.registration_id.map(|id| (id, issue.message)))
            .collect::<HashMap<_, _>>();
        let registrations = scan
            .registrations
            .into_iter()
            .map(|registration| (registration.id().clone(), registration))
            .collect::<HashMap<_, _>>();
        let mut resolution = AgentMcpToolResolution::default();

        for tool_id in selected {
            let registration_id = match McpRegistrationId::from_provider_id(tool_id.provider_id()) {
                Ok(id) => id,
                Err(error) => {
                    resolution.diagnostics.push(agent_tool_diagnostic(
                        tool_id,
                        "mcp.tool_provider_invalid",
                        error.to_string(),
                    ));
                    continue;
                }
            };
            let Some(registration) = registrations.get(&registration_id) else {
                let (code, message) = storage_issues.get(&registration_id).map_or_else(
                    || {
                        (
                            "mcp.registration_not_found",
                            format!("MCP registration `{registration_id}` no longer exists"),
                        )
                    },
                    |message| {
                        (
                            "mcp.registration_storage_issue",
                            format!(
                                "MCP registration `{registration_id}` could not be loaded: {message}"
                            ),
                        )
                    },
                );
                resolution
                    .diagnostics
                    .push(agent_tool_diagnostic(tool_id, code, message));
                continue;
            };
            if registration.state() != McpServerState::Active {
                resolution.diagnostics.push(agent_tool_diagnostic(
                    tool_id,
                    "mcp.server_paused",
                    format!("MCP server `{}` is paused", registration.display_name()),
                ));
                continue;
            }
            if registration.permission_for(tool_id.native_name()) == McpToolPermission::Off {
                resolution.diagnostics.push(agent_tool_diagnostic(
                    tool_id,
                    "mcp.tool_permission_off",
                    "The tool is Off in MCP Manager".to_string(),
                ));
                continue;
            }
            let Some(snapshot) = self.cached_catalog(registration).await? else {
                resolution.diagnostics.push(agent_tool_diagnostic(
                    tool_id,
                    "mcp.catalog_not_cached",
                    format!(
                        "MCP server `{}` has no cached tool catalog; refresh it in MCP Manager",
                        registration.display_name()
                    ),
                ));
                continue;
            };
            let Some(descriptor) = snapshot.catalog.get(tool_id) else {
                resolution.diagnostics.push(agent_tool_diagnostic(
                    tool_id,
                    "mcp.tool_not_in_cached_catalog",
                    format!(
                        "Tool `{}` is absent from the cached catalog; refresh `{}` in MCP Manager",
                        tool_id.native_name(),
                        registration.display_name()
                    ),
                ));
                continue;
            };
            if let Err(message) = validate_agent_input_schema(descriptor) {
                resolution.diagnostics.push(agent_tool_diagnostic(
                    tool_id,
                    "mcp.agent_input_schema_unsupported",
                    message,
                ));
                continue;
            }
            resolution.tools.push(AgentMcpTool {
                registration_id,
                server_display_name: registration.display_name().to_string(),
                descriptor: descriptor.clone(),
                permission: registration.permission_for(tool_id.native_name()),
            });
        }
        Ok(resolution)
    }
}

pub(super) fn validate_agent_input_schema(descriptor: &ToolDescriptor) -> Result<(), String> {
    if descriptor
        .input_schema
        .get("type")
        .and_then(serde_json::Value::as_str)
        == Some("object")
    {
        return Ok(());
    }
    Err(format!(
        "MCP tool `{}` cannot be advertised to an Agent because its input schema root is not explicitly type object",
        descriptor.id
    ))
}

fn agent_tool_diagnostic(
    tool_id: &ToolId,
    code: impl Into<String>,
    message: impl Into<String>,
) -> AgentMcpToolDiagnostic {
    AgentMcpToolDiagnostic {
        tool_id: Some(tool_id.clone()),
        code: code.into(),
        message: message.into(),
    }
}
