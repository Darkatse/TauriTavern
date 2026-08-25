use std::sync::Arc;

use crate::errors::ApplicationError;
use tt_domain::models::{
    mcp::{McpRegistrationId, McpServerRegistration, McpServerState, McpToolPermission},
    tool::{ToolDescriptor, ToolId},
};

use super::{McpService, catalog::CatalogSnapshot};

#[derive(Debug, Clone)]
pub(crate) struct McpModelTool {
    pub registration_id: McpRegistrationId,
    pub server_display_name: String,
    pub descriptor: ToolDescriptor,
    pub permission: McpToolPermission,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct McpModelToolDiagnostic {
    pub tool_id: Option<ToolId>,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct McpModelToolResolution {
    pub tools: Vec<McpModelTool>,
    pub diagnostics: Vec<McpModelToolDiagnostic>,
}

enum CachedModelCatalog {
    Available(Arc<CatalogSnapshot>),
    Missing,
    Invalid(String),
}

impl McpService {
    pub(crate) async fn list_permitted_model_tools_cached(
        &self,
    ) -> Result<McpModelToolResolution, ApplicationError> {
        let scan = self.repository.scan().await?;
        let mut resolution = McpModelToolResolution::default();
        resolution
            .diagnostics
            .extend(scan.issues.into_iter().map(|issue| McpModelToolDiagnostic {
                tool_id: None,
                code: "mcp.registration_storage_issue".to_string(),
                message: format!(
                    "MCP registration file `{}` could not be loaded: {}",
                    issue.file_name, issue.message
                ),
            }));

        for registration in scan.registrations {
            if registration.state() != McpServerState::Active
                || registration.tool_permissions().is_empty()
            {
                continue;
            }
            let snapshot = match self.cached_model_catalog(&registration).await? {
                CachedModelCatalog::Available(snapshot) => snapshot,
                CachedModelCatalog::Missing => {
                    resolution.diagnostics.push(McpModelToolDiagnostic {
                        tool_id: None,
                        code: "mcp.catalog_not_cached".to_string(),
                        message: format!(
                            "MCP server `{}` has no cached tool catalog; refresh it in MCP Manager",
                            registration.display_name()
                        ),
                    });
                    continue;
                }
                CachedModelCatalog::Invalid(message) => {
                    resolution.diagnostics.push(McpModelToolDiagnostic {
                        tool_id: None,
                        code: "mcp.catalog_snapshot_invalid".to_string(),
                        message: format!(
                            "MCP server `{}` has an invalid cached tool catalog: {message}",
                            registration.display_name()
                        ),
                    });
                    continue;
                }
            };
            resolution
                .diagnostics
                .extend(
                    snapshot
                        .diagnostics
                        .iter()
                        .map(|diagnostic| McpModelToolDiagnostic {
                            tool_id: diagnostic.native_name.as_deref().and_then(|native_name| {
                                ToolId::new(&registration.id().provider_id(), native_name).ok()
                            }),
                            code: diagnostic.code.clone(),
                            message: diagnostic.message.clone(),
                        }),
                );
            for native_name in registration.tool_permissions().keys() {
                let tool_id = ToolId::new(&registration.id().provider_id(), native_name)
                    .expect("saved MCP permission names are validated");
                if snapshot.catalog.get(&tool_id).is_none() {
                    resolution.diagnostics.push(model_tool_diagnostic(
                        &tool_id,
                        "mcp.tool_not_in_cached_catalog",
                        format!(
                            "Tool `{native_name}` is absent from the cached catalog; refresh `{}` in MCP Manager",
                            registration.display_name()
                        ),
                    ));
                }
            }
            for descriptor in snapshot.catalog.iter() {
                let permission = registration.permission_for(descriptor.id.native_name());
                if permission == McpToolPermission::Off {
                    continue;
                }
                match materialize_model_tool(&registration, descriptor, permission) {
                    Ok(tool) => resolution.tools.push(tool),
                    Err(diagnostic) => resolution.diagnostics.push(diagnostic),
                }
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

    pub(crate) async fn resolve_permitted_model_tools_cached(
        &self,
        selected: &[ToolId],
    ) -> Result<McpModelToolResolution, ApplicationError> {
        if selected.is_empty() {
            return Ok(McpModelToolResolution::default());
        }
        let scan = self.repository.scan().await?;
        let storage_issues = scan
            .issues
            .into_iter()
            .filter_map(|issue| issue.registration_id.map(|id| (id, issue.message)))
            .collect::<std::collections::HashMap<_, _>>();
        let registrations = scan
            .registrations
            .into_iter()
            .map(|registration| (registration.id().clone(), registration))
            .collect::<std::collections::HashMap<_, _>>();
        let mut resolution = McpModelToolResolution::default();

        for tool_id in selected {
            let registration_id = match McpRegistrationId::from_provider_id(tool_id.provider_id()) {
                Ok(id) => id,
                Err(error) => {
                    resolution.diagnostics.push(model_tool_diagnostic(
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
                    .push(model_tool_diagnostic(tool_id, code, message));
                continue;
            };
            if registration.state() != McpServerState::Active {
                resolution.diagnostics.push(model_tool_diagnostic(
                    tool_id,
                    "mcp.server_paused",
                    format!("MCP server `{}` is paused", registration.display_name()),
                ));
                continue;
            }
            if registration.permission_for(tool_id.native_name()) == McpToolPermission::Off {
                resolution.diagnostics.push(model_tool_diagnostic(
                    tool_id,
                    "mcp.tool_permission_off",
                    "The tool is Off in MCP Manager",
                ));
                continue;
            }
            let snapshot = match self.cached_model_catalog(registration).await? {
                CachedModelCatalog::Available(snapshot) => snapshot,
                CachedModelCatalog::Missing => {
                    resolution.diagnostics.push(model_tool_diagnostic(
                        tool_id,
                        "mcp.catalog_not_cached",
                        format!(
                            "MCP server `{}` has no cached tool catalog; refresh it in MCP Manager",
                            registration.display_name()
                        ),
                    ));
                    continue;
                }
                CachedModelCatalog::Invalid(message) => {
                    resolution.diagnostics.push(model_tool_diagnostic(
                        tool_id,
                        "mcp.catalog_snapshot_invalid",
                        format!(
                            "MCP server `{}` has an invalid cached tool catalog: {message}",
                            registration.display_name()
                        ),
                    ));
                    continue;
                }
            };
            let Some(descriptor) = snapshot.catalog.get(tool_id) else {
                resolution.diagnostics.push(model_tool_diagnostic(
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
            match materialize_model_tool(
                registration,
                descriptor,
                registration.permission_for(tool_id.native_name()),
            ) {
                Ok(tool) => resolution.tools.push(tool),
                Err(diagnostic) => resolution.diagnostics.push(diagnostic),
            }
        }
        Ok(resolution)
    }

    async fn cached_model_catalog(
        &self,
        registration: &McpServerRegistration,
    ) -> Result<CachedModelCatalog, ApplicationError> {
        match self.cached_catalog(registration).await {
            Ok(Some(snapshot)) => Ok(CachedModelCatalog::Available(snapshot)),
            Ok(None) => Ok(CachedModelCatalog::Missing),
            Err(
                ApplicationError::ValidationError(message) | ApplicationError::NotFound(message),
            ) => Ok(CachedModelCatalog::Invalid(message)),
            Err(error) => Err(error),
        }
    }
}

fn materialize_model_tool(
    registration: &McpServerRegistration,
    descriptor: &ToolDescriptor,
    permission: McpToolPermission,
) -> Result<McpModelTool, McpModelToolDiagnostic> {
    validate_model_input_schema(descriptor).map_err(|message| {
        model_tool_diagnostic(
            &descriptor.id,
            "mcp.model_input_schema_unsupported",
            message,
        )
    })?;
    let mut descriptor = descriptor.clone();
    if let Some(override_) = registration.description_override_for(descriptor.id.native_name()) {
        descriptor
            .apply_description_override(override_)
            .map_err(|error| {
                model_tool_diagnostic(
                    &descriptor.id,
                    "mcp.tool_description_override_invalid",
                    error.to_string(),
                )
            })?;
    }
    Ok(McpModelTool {
        registration_id: registration.id().clone(),
        server_display_name: registration.display_name().to_string(),
        descriptor,
        permission,
    })
}

pub(super) fn validate_model_input_schema(descriptor: &ToolDescriptor) -> Result<(), String> {
    if descriptor
        .input_schema
        .get("type")
        .and_then(serde_json::Value::as_str)
        == Some("object")
    {
        return Ok(());
    }
    Err(format!(
        "MCP tool `{}` cannot be advertised to a model because its input schema root is not explicitly type object",
        descriptor.id
    ))
}

pub(super) fn model_tool_diagnostic(
    tool_id: &ToolId,
    code: impl Into<String>,
    message: impl Into<String>,
) -> McpModelToolDiagnostic {
    McpModelToolDiagnostic {
        tool_id: Some(tool_id.clone()),
        code: code.into(),
        message: message.into(),
    }
}
