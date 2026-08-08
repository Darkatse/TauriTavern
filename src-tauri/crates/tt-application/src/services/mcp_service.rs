use std::{collections::BTreeSet, sync::Arc};

use tokio::sync::Mutex;

use crate::{
    dto::mcp_dto::{
        ListMcpServersResultDto, McpDiscoveryResultDto, McpServerDto, McpStaleToolDto,
        McpStorageIssueDto, McpToolDiagnosticDto, McpToolDto,
    },
    errors::ApplicationError,
};
use tt_domain::models::{
    mcp::{
        McpEndpoint, McpRegistrationId, McpServerRegistration, McpServerState, McpToolPermission,
        validate_native_tool_name,
    },
    tool::{ToolCatalog, ToolDescriptor, ToolId},
};
use tt_ports::{mcp::McpGateway, repositories::mcp_server_repository::McpServerRepository};

pub struct McpService {
    repository: Arc<dyn McpServerRepository>,
    gateway: Arc<dyn McpGateway>,
    mutation_lock: Mutex<()>,
}

impl McpService {
    pub fn new(repository: Arc<dyn McpServerRepository>, gateway: Arc<dyn McpGateway>) -> Self {
        Self {
            repository,
            gateway,
            mutation_lock: Mutex::new(()),
        }
    }

    pub async fn list_servers(&self) -> Result<ListMcpServersResultDto, ApplicationError> {
        let scan = self.repository.scan().await?;
        Ok(ListMcpServersResultDto {
            servers: scan.registrations.iter().map(server_dto).collect(),
            storage_issues: scan
                .issues
                .into_iter()
                .map(|issue| McpStorageIssueDto {
                    file_name: issue.file_name,
                    message: issue.message,
                })
                .collect(),
        })
    }

    pub async fn create_server(
        &self,
        display_name: String,
        endpoint: String,
    ) -> Result<McpServerDto, ApplicationError> {
        let endpoint = McpEndpoint::parse(endpoint)?;
        let registration = McpServerRegistration::new_paused(display_name, endpoint)?;
        let _guard = self.mutation_lock.lock().await;
        self.repository.save(&registration).await?;
        Ok(server_dto(&registration))
    }

    pub async fn rename_server(
        &self,
        registration_id: &str,
        display_name: String,
    ) -> Result<McpServerDto, ApplicationError> {
        let id = McpRegistrationId::parse(registration_id)?;
        let _guard = self.mutation_lock.lock().await;
        let mut registration = self.require_registration(&id).await?;
        registration.rename(display_name)?;
        self.repository.save(&registration).await?;
        Ok(server_dto(&registration))
    }

    pub async fn set_server_state(
        &self,
        registration_id: &str,
        state: McpServerState,
    ) -> Result<McpServerDto, ApplicationError> {
        let id = McpRegistrationId::parse(registration_id)?;
        let _guard = self.mutation_lock.lock().await;
        let mut registration = self.require_registration(&id).await?;
        registration.set_state(state);
        self.repository.save(&registration).await?;
        Ok(server_dto(&registration))
    }

    pub async fn set_tool_permission(
        &self,
        registration_id: &str,
        native_name: String,
        permission: McpToolPermission,
    ) -> Result<McpServerDto, ApplicationError> {
        let id = McpRegistrationId::parse(registration_id)?;
        let _guard = self.mutation_lock.lock().await;
        let mut registration = self.require_registration(&id).await?;
        registration.set_tool_permission(native_name, permission)?;
        self.repository.save(&registration).await?;
        Ok(server_dto(&registration))
    }

    pub async fn remove_server(&self, registration_id: &str) -> Result<(), ApplicationError> {
        let id = McpRegistrationId::parse(registration_id)?;
        let _guard = self.mutation_lock.lock().await;
        self.require_registration(&id).await?;
        self.repository.remove(&id).await.map_err(Into::into)
    }

    pub async fn discover_tools(
        &self,
        registration_id: &str,
    ) -> Result<McpDiscoveryResultDto, ApplicationError> {
        let id = McpRegistrationId::parse(registration_id)?;
        let registration = self.require_registration(&id).await?;
        if registration.state() != McpServerState::Active {
            return Err(ApplicationError::ValidationError(format!(
                "mcp.server_paused: registration `{id}` must be Active before discovery"
            )));
        }

        let discovery = self.gateway.discover_tools(registration.endpoint()).await?;
        let mut descriptors = Vec::with_capacity(discovery.tools.len());
        for tool in discovery.tools {
            validate_native_tool_name(&tool.native_name)?;
            descriptors.push(ToolDescriptor {
                id: ToolId::new(&id.provider_id(), &tool.native_name)?,
                title: tool.title,
                description: tool.description,
                input_schema: tool.input_schema,
                output_schema: tool.output_schema,
                annotations: tool.annotations,
            });
        }
        let catalog = ToolCatalog::try_from_descriptors(descriptors)?;
        let discovered_names = catalog
            .iter()
            .map(|descriptor| descriptor.id.native_name().to_string())
            .collect::<BTreeSet<_>>();
        let tools = catalog
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

        Ok(McpDiscoveryResultDto {
            registration_id: id.to_string(),
            protocol_version: discovery.protocol_version,
            server_name: discovery.server_name,
            server_version: discovery.server_version,
            tools,
            diagnostics: discovery
                .diagnostics
                .into_iter()
                .map(|diagnostic| McpToolDiagnosticDto {
                    code: diagnostic.code,
                    native_name: diagnostic.native_name,
                    message: diagnostic.message,
                })
                .collect(),
            stale_tools,
        })
    }

    async fn require_registration(
        &self,
        id: &McpRegistrationId,
    ) -> Result<McpServerRegistration, ApplicationError> {
        self.repository
            .load(id)
            .await?
            .ok_or_else(|| ApplicationError::NotFound(format!("MCP registration not found: {id}")))
    }
}

fn server_dto(registration: &McpServerRegistration) -> McpServerDto {
    McpServerDto {
        id: registration.id().to_string(),
        display_name: registration.display_name().to_string(),
        endpoint: registration.endpoint().as_str().to_string(),
        state: registration.state(),
        tool_permissions: registration.tool_permissions().clone(),
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, sync::Mutex as StdMutex};

    use async_trait::async_trait;
    use serde_json::json;
    use tt_domain::errors::DomainError;
    use tt_ports::{
        mcp::{McpDiscoveredTool, McpDiscoveryResult, McpToolDiagnostic},
        repositories::mcp_server_repository::{McpRegistrationScan, McpRegistrationStorageIssue},
    };

    use super::*;

    #[derive(Default)]
    struct MemoryRepository {
        registrations: StdMutex<BTreeMap<McpRegistrationId, McpServerRegistration>>,
    }

    #[async_trait]
    impl McpServerRepository for MemoryRepository {
        async fn scan(&self) -> Result<McpRegistrationScan, DomainError> {
            Ok(McpRegistrationScan {
                registrations: self
                    .registrations
                    .lock()
                    .unwrap()
                    .values()
                    .cloned()
                    .collect(),
                issues: Vec::<McpRegistrationStorageIssue>::new(),
            })
        }

        async fn load(
            &self,
            id: &McpRegistrationId,
        ) -> Result<Option<McpServerRegistration>, DomainError> {
            Ok(self.registrations.lock().unwrap().get(id).cloned())
        }

        async fn save(&self, registration: &McpServerRegistration) -> Result<(), DomainError> {
            self.registrations
                .lock()
                .unwrap()
                .insert(registration.id().clone(), registration.clone());
            Ok(())
        }

        async fn remove(&self, id: &McpRegistrationId) -> Result<(), DomainError> {
            self.registrations.lock().unwrap().remove(id);
            Ok(())
        }
    }

    struct FixedGateway;

    #[async_trait]
    impl McpGateway for FixedGateway {
        async fn discover_tools(
            &self,
            _endpoint: &McpEndpoint,
        ) -> Result<McpDiscoveryResult, DomainError> {
            Ok(McpDiscoveryResult {
                protocol_version: "2026-07-28".to_string(),
                server_name: Some("fixture".to_string()),
                server_version: Some("1.0".to_string()),
                tools: vec![McpDiscoveredTool {
                    native_name: "search".to_string(),
                    title: Some("Search".to_string()),
                    description: None,
                    input_schema: json!({ "type": "object" }),
                    output_schema: None,
                    annotations: json!({ "readOnlyHint": true }),
                }],
                diagnostics: Vec::<McpToolDiagnostic>::new(),
            })
        }
    }

    #[tokio::test]
    async fn registration_discovery_keeps_authority_off_by_default_and_reports_stale_settings() {
        let service = McpService::new(
            Arc::new(MemoryRepository::default()),
            Arc::new(FixedGateway),
        );
        let created = service
            .create_server(
                "Fixture".to_string(),
                "http://127.0.0.1:3333/mcp".to_string(),
            )
            .await
            .unwrap();
        assert_eq!(created.state, McpServerState::Paused);
        assert!(service.discover_tools(&created.id).await.is_err());

        service
            .set_tool_permission(&created.id, "missing".to_string(), McpToolPermission::Allow)
            .await
            .unwrap();
        service
            .set_server_state(&created.id, McpServerState::Active)
            .await
            .unwrap();

        let discovery = service.discover_tools(&created.id).await.unwrap();

        assert_eq!(discovery.tools.len(), 1);
        assert_eq!(discovery.tools[0].permission, McpToolPermission::Off);
        assert_eq!(
            discovery.tools[0].id.as_str(),
            format!("mcp/{}:search", created.id)
        );
        assert_eq!(discovery.stale_tools.len(), 1);
        assert_eq!(discovery.stale_tools[0].native_name, "missing");
    }
}
