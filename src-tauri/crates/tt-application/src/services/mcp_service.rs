use std::{
    collections::{BTreeSet, HashMap, hash_map::Entry},
    sync::{Arc, RwLock},
};

use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::{
    dto::mcp_dto::{
        ListMcpServersResultDto, McpCallDiagnosticDto, McpDiscoveryResultDto, McpKnownResponseDto,
        McpServerDto, McpStaleToolDto, McpStorageIssueDto, McpTestCallOutcomeDto,
        McpTextContentDto, McpToolDiagnosticDto, McpToolDto,
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
use tt_ports::{
    mcp::{McpCallOutcome, McpDiscoveryResult, McpGateway, McpKnownResponse, McpToolDiagnostic},
    repositories::mcp_server_repository::McpServerRepository,
};

const MAX_ARGUMENTS_JSON_BYTES: usize = 256 * 1024;

struct CatalogSnapshot {
    protocol_version: String,
    server_name: Option<String>,
    server_version: Option<String>,
    catalog: ToolCatalog,
    diagnostics: Vec<McpToolDiagnostic>,
}

pub struct McpService {
    repository: Arc<dyn McpServerRepository>,
    gateway: Arc<dyn McpGateway>,
    mutation_lock: Mutex<()>,
    catalog_snapshots: RwLock<HashMap<McpRegistrationId, Arc<CatalogSnapshot>>>,
    test_calls: TestCallRegistry,
}

impl McpService {
    pub fn new(repository: Arc<dyn McpServerRepository>, gateway: Arc<dyn McpGateway>) -> Self {
        Self {
            repository,
            gateway,
            mutation_lock: Mutex::new(()),
            catalog_snapshots: RwLock::new(HashMap::new()),
            test_calls: TestCallRegistry::default(),
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

    pub async fn test_call(
        &self,
        call_id: &str,
        registration_id: &str,
        native_name: String,
        arguments_json: String,
    ) -> Result<McpTestCallOutcomeDto, ApplicationError> {
        let Some(cancel) = self.test_calls.get(call_id).await else {
            return Ok(not_sent(
                "mcp.call_not_started",
                "The test call was not prepared or was cancelled before it started",
            ));
        };
        let result = self
            .test_call_inner(registration_id, native_name, arguments_json, cancel)
            .await;
        self.test_calls.complete(call_id).await;
        result
    }

    pub async fn start_test_call(&self, call_id: &str) -> Result<(), ApplicationError> {
        self.test_calls.start(call_id).await
    }

    pub async fn cancel_test_call(&self, call_id: &str) -> Result<(), ApplicationError> {
        self.test_calls.cancel(call_id).await;
        Ok(())
    }

    async fn test_call_inner(
        &self,
        registration_id: &str,
        native_name: String,
        arguments_json: String,
        cancel: CancellationToken,
    ) -> Result<McpTestCallOutcomeDto, ApplicationError> {
        if cancel.is_cancelled() {
            return Ok(not_sent(
                "mcp.call_cancelled_before_send",
                "The tool request was cancelled before it started",
            ));
        }

        let id = match McpRegistrationId::parse(registration_id) {
            Ok(id) => id,
            Err(error) => return Ok(not_sent("mcp.call_registration_invalid", error.to_string())),
        };
        if let Err(error) = validate_native_tool_name(&native_name) {
            return Ok(not_sent("mcp.call_tool_name_invalid", error.to_string()));
        }
        if arguments_json.len() > MAX_ARGUMENTS_JSON_BYTES {
            return Ok(not_sent(
                "mcp.call_arguments_size_limit",
                format!("Arguments JSON exceeds {MAX_ARGUMENTS_JSON_BYTES} bytes"),
            ));
        }
        let arguments = match serde_json::from_str::<serde_json::Value>(&arguments_json) {
            Ok(serde_json::Value::Object(arguments)) => arguments,
            Ok(_) => {
                return Ok(not_sent(
                    "mcp.call_arguments_not_object",
                    "Arguments must be a JSON object",
                ));
            }
            Err(error) => {
                return Ok(not_sent(
                    "mcp.call_arguments_invalid_json",
                    format!("Arguments are not valid JSON: {error}"),
                ));
            }
        };

        let Some(registration) = self.repository.load(&id).await? else {
            return Ok(not_sent(
                "mcp.call_registration_not_found",
                format!("MCP registration not found: {id}"),
            ));
        };
        if registration.state() != McpServerState::Active {
            return Ok(not_sent(
                "mcp.call_server_paused",
                format!("MCP registration `{id}` must be Active before a test call"),
            ));
        }
        let outcome = self
            .gateway
            .call_tool(registration.endpoint(), &native_name, arguments, cancel)
            .await?;
        Ok(map_call_outcome(outcome))
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

fn not_sent(code: impl Into<String>, message: impl Into<String>) -> McpTestCallOutcomeDto {
    McpTestCallOutcomeDto::NotSent {
        code: code.into(),
        message: message.into(),
    }
}

fn map_call_outcome(outcome: McpCallOutcome) -> McpTestCallOutcomeDto {
    match outcome {
        McpCallOutcome::KnownResponse(response) => McpTestCallOutcomeDto::KnownResponse {
            response: match response {
                McpKnownResponse::ToolResult(result) => McpKnownResponseDto::ToolResult {
                    is_error: result.is_error,
                    text_blocks: result
                        .text
                        .into_iter()
                        .map(|content| McpTextContentDto {
                            index: content.index,
                            text: content.text,
                        })
                        .collect(),
                    structured_json: result.structured_content.map(|value| {
                        serde_json::to_string_pretty(&value)
                            .expect("serde_json::Value is always serializable")
                    }),
                    diagnostics: result
                        .diagnostics
                        .into_iter()
                        .map(|diagnostic| McpCallDiagnosticDto {
                            code: diagnostic.code,
                            message: diagnostic.message,
                            content_index: diagnostic.content_index,
                        })
                        .collect(),
                },
                McpKnownResponse::ServerError(error) => McpKnownResponseDto::ServerError {
                    code: error.code,
                    message: error.message,
                    data_json: error.data.map(|value| value.to_string()),
                },
                McpKnownResponse::Unsupported(response) => {
                    McpKnownResponseDto::UnsupportedResponse {
                        response_type: response.response_type,
                        message: response.message,
                    }
                }
            },
        },
        McpCallOutcome::NotSent(issue) => McpTestCallOutcomeDto::NotSent {
            code: issue.code,
            message: issue.message,
        },
        McpCallOutcome::OutcomeUnknown(issue) => McpTestCallOutcomeDto::OutcomeUnknown {
            code: issue.code,
            message: issue.message,
        },
    }
}

#[derive(Default)]
struct TestCallRegistry {
    calls: Mutex<HashMap<String, CancellationToken>>,
}

impl TestCallRegistry {
    async fn start(&self, call_id: &str) -> Result<(), ApplicationError> {
        let mut calls = self.calls.lock().await;
        match calls.entry(call_id.to_string()) {
            Entry::Vacant(entry) => {
                entry.insert(CancellationToken::new());
                Ok(())
            }
            Entry::Occupied(_) => Err(ApplicationError::Conflict(format!(
                "mcp.call_id_in_use: call `{call_id}` is already active"
            ))),
        }
    }

    async fn get(&self, call_id: &str) -> Option<CancellationToken> {
        self.calls.lock().await.get(call_id).cloned()
    }

    async fn cancel(&self, call_id: &str) {
        if let Some(cancel) = self.calls.lock().await.remove(call_id) {
            cancel.cancel();
        }
    }

    async fn complete(&self, call_id: &str) {
        self.calls.lock().await.remove(call_id);
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
    use std::{
        collections::BTreeMap,
        sync::{
            Mutex as StdMutex,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        },
    };

    use async_trait::async_trait;
    use serde_json::json;
    use tt_domain::errors::DomainError;
    use tt_ports::{
        mcp::{
            McpDiscoveredTool, McpDiscoveryResult, McpTextContent, McpToolCallResult,
            McpToolDiagnostic,
        },
        repositories::mcp_server_repository::{McpRegistrationScan, McpRegistrationStorageIssue},
    };

    use super::*;

    #[derive(Default)]
    struct MemoryRepository {
        registrations: StdMutex<BTreeMap<McpRegistrationId, McpServerRegistration>>,
        catalogs: StdMutex<BTreeMap<McpRegistrationId, (String, McpDiscoveryResult)>>,
        fail_catalog_save: AtomicBool,
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

        async fn load_catalog_snapshot(
            &self,
            id: &McpRegistrationId,
            endpoint: &McpEndpoint,
        ) -> Result<Option<McpDiscoveryResult>, DomainError> {
            let catalogs = self.catalogs.lock().unwrap();
            match catalogs.get(id) {
                Some((stored_endpoint, snapshot)) if stored_endpoint == endpoint.as_str() => {
                    Ok(Some(snapshot.clone()))
                }
                Some((stored_endpoint, _)) => Err(DomainError::InvalidData(format!(
                    "catalog endpoint `{stored_endpoint}` does not match `{}`",
                    endpoint.as_str()
                ))),
                None => Ok(None),
            }
        }

        async fn save_catalog_snapshot(
            &self,
            id: &McpRegistrationId,
            endpoint: &McpEndpoint,
            snapshot: &McpDiscoveryResult,
        ) -> Result<(), DomainError> {
            if self.fail_catalog_save.load(Ordering::Relaxed) {
                return Err(DomainError::InternalError(
                    "fixture catalog save failed".to_string(),
                ));
            }
            self.catalogs.lock().unwrap().insert(
                id.clone(),
                (endpoint.as_str().to_string(), snapshot.clone()),
            );
            Ok(())
        }

        async fn remove(&self, id: &McpRegistrationId) -> Result<(), DomainError> {
            self.catalogs.lock().unwrap().remove(id);
            self.registrations.lock().unwrap().remove(id);
            Ok(())
        }
    }

    #[derive(Default)]
    struct FixedGateway {
        calls: StdMutex<Vec<(String, serde_json::Map<String, serde_json::Value>)>>,
        discovery_calls: AtomicUsize,
        discovery_revision: AtomicUsize,
        fail_discovery: AtomicBool,
    }

    #[async_trait]
    impl McpGateway for FixedGateway {
        async fn discover_tools(
            &self,
            _endpoint: &McpEndpoint,
        ) -> Result<McpDiscoveryResult, DomainError> {
            self.discovery_calls.fetch_add(1, Ordering::Relaxed);
            if self.fail_discovery.load(Ordering::Relaxed) {
                return Err(DomainError::Transient(
                    "fixture discovery failed".to_string(),
                ));
            }
            let revision = self.discovery_revision.load(Ordering::Relaxed);
            Ok(McpDiscoveryResult {
                protocol_version: "2026-07-28".to_string(),
                server_name: Some("fixture".to_string()),
                server_version: Some(format!("1.{revision}")),
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

        async fn call_tool(
            &self,
            _endpoint: &McpEndpoint,
            native_name: &str,
            arguments: serde_json::Map<String, serde_json::Value>,
            _cancel: CancellationToken,
        ) -> Result<McpCallOutcome, DomainError> {
            self.calls
                .lock()
                .unwrap()
                .push((native_name.to_string(), arguments));
            Ok(McpCallOutcome::KnownResponse(McpKnownResponse::ToolResult(
                McpToolCallResult {
                    is_error: false,
                    text: vec![McpTextContent {
                        index: 0,
                        text: "done".to_string(),
                    }],
                    structured_content: Some(json!({ "ok": true })),
                    diagnostics: Vec::new(),
                },
            )))
        }
    }

    #[tokio::test]
    async fn registration_discovery_keeps_authority_off_by_default_and_reports_stale_settings() {
        let service = McpService::new(
            Arc::new(MemoryRepository::default()),
            Arc::new(FixedGateway::default()),
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

    #[tokio::test]
    async fn catalog_persists_across_services_and_reprojects_current_permission() {
        let repository = Arc::new(MemoryRepository::default());
        let first_gateway = Arc::new(FixedGateway::default());
        let first_service = McpService::new(repository.clone(), first_gateway.clone());
        let created = first_service
            .create_server(
                "Fixture".to_string(),
                "http://127.0.0.1:3333/mcp".to_string(),
            )
            .await
            .unwrap();
        first_service
            .set_server_state(&created.id, McpServerState::Active)
            .await
            .unwrap();

        first_service.discover_tools(&created.id).await.unwrap();
        first_service
            .set_tool_permission(&created.id, "search".to_string(), McpToolPermission::Allow)
            .await
            .unwrap();
        assert_eq!(first_gateway.discovery_calls.load(Ordering::Relaxed), 1);

        let second_gateway = Arc::new(FixedGateway::default());
        second_gateway.fail_discovery.store(true, Ordering::Relaxed);
        let second_service = McpService::new(repository, second_gateway.clone());
        let restored = second_service.discover_tools(&created.id).await.unwrap();

        assert_eq!(restored.server_version.as_deref(), Some("1.0"));
        assert_eq!(restored.tools[0].permission, McpToolPermission::Allow);
        assert_eq!(second_gateway.discovery_calls.load(Ordering::Relaxed), 0);

        second_service.clear_catalog_memory();
        second_service.discover_tools(&created.id).await.unwrap();
        assert_eq!(second_gateway.discovery_calls.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn refresh_keeps_live_catalog_usable_when_persistence_fails() {
        let repository = Arc::new(MemoryRepository::default());
        let gateway = Arc::new(FixedGateway::default());
        let service = McpService::new(repository.clone(), gateway.clone());
        let created = service
            .create_server(
                "Fixture".to_string(),
                "http://127.0.0.1:3333/mcp".to_string(),
            )
            .await
            .unwrap();
        service
            .set_server_state(&created.id, McpServerState::Active)
            .await
            .unwrap();
        service.discover_tools(&created.id).await.unwrap();

        gateway.discovery_revision.store(2, Ordering::Relaxed);
        let refreshed = service.refresh_tools(&created.id).await.unwrap();
        assert_eq!(refreshed.server_version.as_deref(), Some("1.2"));
        assert_eq!(gateway.discovery_calls.load(Ordering::Relaxed), 2);

        gateway.fail_discovery.store(true, Ordering::Relaxed);
        assert!(service.refresh_tools(&created.id).await.is_err());
        assert_eq!(gateway.discovery_calls.load(Ordering::Relaxed), 3);

        service.clear_catalog_memory();
        let restored = service.discover_tools(&created.id).await.unwrap();
        assert_eq!(restored.server_version.as_deref(), Some("1.2"));
        assert_eq!(gateway.discovery_calls.load(Ordering::Relaxed), 3);

        gateway.fail_discovery.store(false, Ordering::Relaxed);
        gateway.discovery_revision.store(3, Ordering::Relaxed);
        repository.fail_catalog_save.store(true, Ordering::Relaxed);
        let memory_only = service.refresh_tools(&created.id).await.unwrap();
        assert_eq!(memory_only.server_version.as_deref(), Some("1.3"));
        assert_eq!(memory_only.diagnostics.len(), 1);
        assert_eq!(
            memory_only.diagnostics[0].code,
            "mcp.catalog_persistence_failed"
        );
        assert_eq!(gateway.discovery_calls.load(Ordering::Relaxed), 4);

        let restored = service.discover_tools(&created.id).await.unwrap();
        assert_eq!(restored.server_version.as_deref(), Some("1.3"));
        assert_eq!(restored.diagnostics.len(), 1);

        service.clear_catalog_memory();
        let restored = service.discover_tools(&created.id).await.unwrap();
        assert_eq!(restored.server_version.as_deref(), Some("1.2"));
        assert_eq!(gateway.discovery_calls.load(Ordering::Relaxed), 4);
    }

    #[tokio::test]
    async fn paused_gate_precedes_cache_and_remove_clears_both_copies() {
        let repository = Arc::new(MemoryRepository::default());
        let gateway = Arc::new(FixedGateway::default());
        let service = McpService::new(repository.clone(), gateway.clone());
        let created = service
            .create_server(
                "Fixture".to_string(),
                "http://127.0.0.1:3333/mcp".to_string(),
            )
            .await
            .unwrap();
        service
            .set_server_state(&created.id, McpServerState::Active)
            .await
            .unwrap();
        service.discover_tools(&created.id).await.unwrap();
        service
            .set_server_state(&created.id, McpServerState::Paused)
            .await
            .unwrap();

        assert!(service.discover_tools(&created.id).await.is_err());
        assert_eq!(gateway.discovery_calls.load(Ordering::Relaxed), 1);

        service.remove_server(&created.id).await.unwrap();
        let id = McpRegistrationId::parse(&created.id).unwrap();
        assert!(repository.catalogs.lock().unwrap().get(&id).is_none());
        assert!(service.catalog_snapshots.read().unwrap().get(&id).is_none());
    }

    #[tokio::test]
    async fn explicit_test_call_preserves_json_and_ignores_saved_permission() {
        let repository = Arc::new(MemoryRepository::default());
        let gateway = Arc::new(FixedGateway::default());
        let service = McpService::new(repository, gateway.clone());
        let created = service
            .create_server(
                "Fixture".to_string(),
                "http://127.0.0.1:3333/mcp".to_string(),
            )
            .await
            .unwrap();
        service
            .set_tool_permission(&created.id, "search".to_string(), McpToolPermission::Ask)
            .await
            .unwrap();
        service
            .set_server_state(&created.id, McpServerState::Active)
            .await
            .unwrap();

        service.start_test_call("call-1").await.unwrap();
        let outcome = service
            .test_call(
                "call-1",
                &created.id,
                "search".to_string(),
                r#"{"value":9007199254740993}"#.to_string(),
            )
            .await
            .unwrap();

        assert!(matches!(
            &outcome,
            McpTestCallOutcomeDto::KnownResponse {
                response: McpKnownResponseDto::ToolResult {
                    is_error: false,
                    ..
                }
            }
        ));
        let wire = serde_json::to_value(&outcome).unwrap();
        assert_eq!(wire["outcome"], "known_response");
        assert_eq!(wire["response"]["kind"], "tool_result");
        assert_eq!(wire["response"]["structuredJson"], "{\n  \"ok\": true\n}");
        {
            let calls = gateway.calls.lock().unwrap();
            assert_eq!(calls.len(), 1);
            assert_eq!(calls[0].0, "search");
            assert_eq!(calls[0].1["value"].to_string(), "9007199254740993");
        }

        let listed = service.list_servers().await.unwrap();
        assert_eq!(
            listed.servers[0].tool_permissions.get("search"),
            Some(&McpToolPermission::Ask)
        );
    }

    #[tokio::test]
    async fn cancelled_prepared_call_is_not_sent_or_retained() {
        let repository = Arc::new(MemoryRepository::default());
        let gateway = Arc::new(FixedGateway::default());
        let service = McpService::new(repository, gateway.clone());
        let created = service
            .create_server(
                "Fixture".to_string(),
                "http://127.0.0.1:3333/mcp".to_string(),
            )
            .await
            .unwrap();
        service
            .set_server_state(&created.id, McpServerState::Active)
            .await
            .unwrap();

        service.start_test_call("call-early").await.unwrap();
        service.cancel_test_call("call-early").await.unwrap();
        let outcome = service
            .test_call(
                "call-early",
                &created.id,
                "search".to_string(),
                "{}".to_string(),
            )
            .await
            .unwrap();

        assert!(matches!(outcome, McpTestCallOutcomeDto::NotSent { .. }));
        assert!(gateway.calls.lock().unwrap().is_empty());
        assert!(service.test_calls.calls.lock().await.is_empty());
    }

    #[tokio::test]
    async fn paused_registration_is_rejected_before_the_gateway() {
        let repository = Arc::new(MemoryRepository::default());
        let gateway = Arc::new(FixedGateway::default());
        let service = McpService::new(repository, gateway.clone());
        let created = service
            .create_server(
                "Fixture".to_string(),
                "http://127.0.0.1:3333/mcp".to_string(),
            )
            .await
            .unwrap();
        service.start_test_call("call-paused").await.unwrap();

        let outcome = service
            .test_call(
                "call-paused",
                &created.id,
                "search".to_string(),
                "{}".to_string(),
            )
            .await
            .unwrap();

        assert!(matches!(
            outcome,
            McpTestCallOutcomeDto::NotSent { ref code, .. }
                if code == "mcp.call_server_paused"
        ));
        assert!(gateway.calls.lock().unwrap().is_empty());
    }
}
