use std::{collections::BTreeSet, sync::Arc};

use crate::{
    dto::mcp_dto::{McpDiscoveryResultDto, McpStaleToolDto, McpToolDiagnosticDto, McpToolDto},
    errors::ApplicationError,
};
use tt_domain::models::{
    mcp::{McpRegistrationId, McpServerRegistration, McpServerState, validate_native_tool_name},
    tool::{ToolCatalog, ToolDescriptor, ToolId},
};
use tt_ports::mcp::{McpDiscoveryResult, McpToolDiagnostic};

use super::McpService;

pub(super) struct CatalogSnapshot {
    protocol_version: String,
    server_name: Option<String>,
    server_version: Option<String>,
    pub(super) catalog: ToolCatalog,
    pub(super) diagnostics: Vec<McpToolDiagnostic>,
}

impl McpService {
    pub fn clear_catalog_memory(&self) {
        self.catalog_snapshots
            .write()
            .expect("MCP catalog snapshot lock poisoned")
            .clear();
    }

    pub async fn discover_tools(
        &self,
        registration_id: &str,
    ) -> Result<McpDiscoveryResultDto, ApplicationError> {
        let (id, registration) = self.require_active_registration(registration_id).await?;
        let snapshot = self
            .catalog_snapshots
            .read()
            .expect("MCP catalog snapshot lock poisoned")
            .get(&id)
            .cloned();
        let snapshot = match snapshot {
            Some(snapshot) => snapshot,
            None => {
                match self
                    .repository
                    .load_catalog_snapshot(&id, registration.endpoint())
                    .await?
                {
                    Some(discovery) => {
                        let snapshot = catalog_snapshot(&id, &discovery)?;
                        self.publish_catalog(&id, snapshot)
                    }
                    None => self.discover_catalog(&id, &registration).await?,
                }
            }
        };
        Ok(discovery_dto(&registration, &snapshot))
    }

    pub async fn refresh_tools(
        &self,
        registration_id: &str,
    ) -> Result<McpDiscoveryResultDto, ApplicationError> {
        let (id, registration) = self.require_active_registration(registration_id).await?;
        let snapshot = self.discover_catalog(&id, &registration).await?;
        Ok(discovery_dto(&registration, &snapshot))
    }

    pub(super) async fn cached_catalog(
        &self,
        registration: &McpServerRegistration,
    ) -> Result<Option<Arc<CatalogSnapshot>>, ApplicationError> {
        let id = registration.id();
        if let Some(snapshot) = self
            .catalog_snapshots
            .read()
            .expect("MCP catalog snapshot lock poisoned")
            .get(id)
            .cloned()
        {
            return Ok(Some(snapshot));
        }
        let Some(discovery) = self
            .repository
            .load_catalog_snapshot(id, registration.endpoint())
            .await?
        else {
            return Ok(None);
        };
        let snapshot = catalog_snapshot(id, &discovery)?;
        Ok(Some(self.publish_catalog(id, snapshot)))
    }

    async fn discover_catalog(
        &self,
        id: &McpRegistrationId,
        registration: &McpServerRegistration,
    ) -> Result<Arc<CatalogSnapshot>, ApplicationError> {
        let discovery = self.gateway.discover_tools(registration.endpoint()).await?;
        let mut snapshot = catalog_snapshot(id, &discovery)?;
        if let Err(error) = self
            .repository
            .save_catalog_snapshot(id, registration.endpoint(), &discovery)
            .await
        {
            tracing::warn!(registration_id = %id, %error, "MCP catalog remains memory-only");
            snapshot.diagnostics.push(McpToolDiagnostic {
                code: "mcp.catalog_persistence_failed".to_string(),
                native_name: None,
                message: format!(
                    "Tools are available for this session, but the catalog snapshot could not be saved: {error}"
                ),
            });
        }
        Ok(self.publish_catalog(id, snapshot))
    }

    fn publish_catalog(
        &self,
        id: &McpRegistrationId,
        snapshot: CatalogSnapshot,
    ) -> Arc<CatalogSnapshot> {
        let snapshot = Arc::new(snapshot);
        self.catalog_snapshots
            .write()
            .expect("MCP catalog snapshot lock poisoned")
            .insert(id.clone(), snapshot.clone());
        snapshot
    }

    async fn require_active_registration(
        &self,
        registration_id: &str,
    ) -> Result<(McpRegistrationId, McpServerRegistration), ApplicationError> {
        let id = McpRegistrationId::parse(registration_id)?;
        let registration = self.require_registration(&id).await?;
        if registration.state() != McpServerState::Active {
            return Err(ApplicationError::ValidationError(format!(
                "mcp.server_paused: registration `{id}` must be Active before discovery"
            )));
        }
        Ok((id, registration))
    }
}

fn catalog_snapshot(
    id: &McpRegistrationId,
    discovery: &McpDiscoveryResult,
) -> Result<CatalogSnapshot, ApplicationError> {
    let mut descriptors = Vec::with_capacity(discovery.tools.len());
    for tool in &discovery.tools {
        validate_native_tool_name(&tool.native_name)?;
        descriptors.push(ToolDescriptor {
            id: ToolId::new(&id.provider_id(), &tool.native_name)?,
            title: tool.title.clone(),
            description: tool.description.clone(),
            input_schema: tool.input_schema.clone(),
            output_schema: tool.output_schema.clone(),
            annotations: tool.annotations.clone(),
        });
    }
    Ok(CatalogSnapshot {
        protocol_version: discovery.protocol_version.clone(),
        server_name: discovery.server_name.clone(),
        server_version: discovery.server_version.clone(),
        catalog: ToolCatalog::try_from_descriptors(descriptors)?,
        diagnostics: discovery.diagnostics.clone(),
    })
}

fn discovery_dto(
    registration: &McpServerRegistration,
    snapshot: &CatalogSnapshot,
) -> McpDiscoveryResultDto {
    let discovered_names = snapshot
        .catalog
        .iter()
        .map(|descriptor| descriptor.id.native_name().to_string())
        .collect::<BTreeSet<_>>();
    let tools = snapshot
        .catalog
        .iter()
        .map(|descriptor| McpToolDto {
            id: descriptor.id.clone(),
            native_name: descriptor.id.native_name().to_string(),
            title: descriptor.title.clone(),
            description: descriptor.description.clone(),
            input_schema: descriptor.input_schema.clone(),
            output_schema: descriptor.output_schema.clone(),
            annotations: descriptor.annotations.clone(),
            permission: registration.permission_for(descriptor.id.native_name()),
        })
        .collect();
    let stale_tools = registration
        .tool_permissions()
        .iter()
        .filter(|(native_name, _)| !discovered_names.contains(*native_name))
        .map(|(native_name, permission)| McpStaleToolDto {
            native_name: native_name.clone(),
            permission: *permission,
        })
        .collect();

    McpDiscoveryResultDto {
        registration_id: registration.id().to_string(),
        protocol_version: snapshot.protocol_version.clone(),
        server_name: snapshot.server_name.clone(),
        server_version: snapshot.server_version.clone(),
        tools,
        diagnostics: snapshot
            .diagnostics
            .iter()
            .map(|diagnostic| McpToolDiagnosticDto {
                code: diagnostic.code.clone(),
                native_name: diagnostic.native_name.clone(),
                message: diagnostic.message.clone(),
            })
            .collect(),
        stale_tools,
    }
}
