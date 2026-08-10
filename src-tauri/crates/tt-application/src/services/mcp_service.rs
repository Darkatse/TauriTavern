use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use tokio::sync::Mutex;

use crate::{
    dto::mcp_dto::{ListMcpServersResultDto, McpServerDto, McpStorageIssueDto},
    errors::ApplicationError,
};
use tt_domain::models::mcp::{
    McpEndpoint, McpRegistrationId, McpServerRegistration, McpServerState, McpToolPermission,
};
use tt_ports::{mcp::McpGateway, repositories::mcp_server_repository::McpServerRepository};

mod call;
mod catalog;
mod legacy;
mod model_tools;
mod permitted_call;
mod test_call;

#[cfg(test)]
mod tests;

use call::CallRegistry;
use catalog::CatalogSnapshot;
pub(crate) use model_tools::{McpModelTool, McpModelToolDiagnostic};

pub(super) const MAX_ARGUMENTS_JSON_BYTES: usize = 256 * 1024;

pub struct McpService {
    repository: Arc<dyn McpServerRepository>,
    gateway: Arc<dyn McpGateway>,
    mutation_lock: Mutex<()>,
    catalog_snapshots: RwLock<HashMap<McpRegistrationId, Arc<CatalogSnapshot>>>,
    calls: CallRegistry,
}

impl McpService {
    pub fn new(repository: Arc<dyn McpServerRepository>, gateway: Arc<dyn McpGateway>) -> Self {
        Self {
            repository,
            gateway,
            mutation_lock: Mutex::new(()),
            catalog_snapshots: RwLock::new(HashMap::new()),
            calls: CallRegistry::default(),
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
        self.repository.remove(&id).await?;
        self.catalog_snapshots
            .write()
            .expect("MCP catalog snapshot lock poisoned")
            .remove(&id);
        Ok(())
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
