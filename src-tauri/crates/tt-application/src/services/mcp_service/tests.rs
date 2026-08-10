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
        McpDiscoveredTool, McpDiscoveryResult, McpTextContent, McpToolCallResult, McpToolDiagnostic,
    },
    repositories::mcp_server_repository::{McpRegistrationScan, McpRegistrationStorageIssue},
};

use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tt_domain::models::{
    mcp::{
        McpEndpoint, McpProtocolVersionPreference, McpRegistrationId, McpRequestHeaders,
        McpServerRegistration, McpServerState, McpToolPermission,
    },
    tool::{ToolDescriptor, ToolId},
};
use tt_ports::{
    mcp::{McpCallIssue, McpCallOutcome, McpGateway, McpKnownResponse},
    repositories::mcp_server_repository::McpServerRepository,
};

use crate::dto::mcp_dto::{McpCallOutcomeDto, McpKnownResponseDto};

use super::{McpService, model_tools::validate_model_input_schema};

#[test]
fn model_tools_require_object_root_input_schema() {
    let descriptor = ToolDescriptor {
        id: ToolId::new(
            &tt_domain::models::tool::ToolProviderId::parse(
                "mcp/550e8400-e29b-41d4-a716-446655440000",
            )
            .unwrap(),
            "search",
        )
        .unwrap(),
        title: None,
        description: None,
        input_schema: json!({ "type": "string" }),
        output_schema: None,
        annotations: json!({}),
    };

    assert!(validate_model_input_schema(&descriptor).is_err());
}

#[derive(Default)]
struct MemoryRepository {
    registrations: StdMutex<BTreeMap<McpRegistrationId, McpServerRegistration>>,
    catalogs: StdMutex<BTreeMap<McpRegistrationId, (String, McpDiscoveryResult)>>,
    scan_issues: StdMutex<Vec<McpRegistrationStorageIssue>>,
    fail_scan: AtomicBool,
    fail_catalog_save: AtomicBool,
    catalog_loads: AtomicUsize,
}

#[async_trait]
impl McpServerRepository for MemoryRepository {
    async fn scan(&self) -> Result<McpRegistrationScan, DomainError> {
        if self.fail_scan.load(Ordering::Relaxed) {
            return Err(DomainError::InternalError(
                "fixture registration scan failed".to_string(),
            ));
        }
        Ok(McpRegistrationScan {
            registrations: self
                .registrations
                .lock()
                .unwrap()
                .values()
                .cloned()
                .collect(),
            issues: self.scan_issues.lock().unwrap().clone(),
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
        self.catalog_loads.fetch_add(1, Ordering::Relaxed);
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

    async fn remove_catalog_snapshot(&self, id: &McpRegistrationId) -> Result<(), DomainError> {
        self.catalogs.lock().unwrap().remove(id);
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
    request_headers: StdMutex<Vec<BTreeMap<String, String>>>,
    protocol_versions: StdMutex<Vec<McpProtocolVersionPreference>>,
    discovery_calls: AtomicUsize,
    discovery_revision: AtomicUsize,
    fail_discovery: AtomicBool,
}

#[async_trait]
impl McpGateway for FixedGateway {
    async fn discover_tools(
        &self,
        _endpoint: &McpEndpoint,
        request_headers: &McpRequestHeaders,
        protocol_version: McpProtocolVersionPreference,
    ) -> Result<McpDiscoveryResult, DomainError> {
        self.request_headers
            .lock()
            .unwrap()
            .push(request_headers.as_map().clone());
        self.protocol_versions
            .lock()
            .unwrap()
            .push(protocol_version);
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
        request_headers: &McpRequestHeaders,
        protocol_version: McpProtocolVersionPreference,
        native_name: &str,
        arguments: serde_json::Map<String, serde_json::Value>,
        _cancel: CancellationToken,
    ) -> Result<McpCallOutcome, DomainError> {
        self.request_headers
            .lock()
            .unwrap()
            .push(request_headers.as_map().clone());
        self.protocol_versions
            .lock()
            .unwrap()
            .push(protocol_version);
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
            BTreeMap::new(),
            McpProtocolVersionPreference::Auto,
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
            BTreeMap::new(),
            McpProtocolVersionPreference::Auto,
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
async fn connection_update_invalidates_catalog_and_uses_new_headers_and_protocol() {
    let repository = Arc::new(MemoryRepository::default());
    let gateway = Arc::new(FixedGateway::default());
    let service = McpService::new(repository.clone(), gateway.clone());
    let created = service
        .create_server(
            "Fixture".to_string(),
            "http://127.0.0.1:3333/mcp".to_string(),
            BTreeMap::from([("x-api-key".to_string(), "old".to_string())]),
            McpProtocolVersionPreference::Auto,
        )
        .await
        .unwrap();
    service
        .set_server_state(&created.id, McpServerState::Active)
        .await
        .unwrap();
    service.discover_tools(&created.id).await.unwrap();

    let updated = service
        .update_server(
            &created.id,
            "Updated".to_string(),
            "https://user:pass@example.com/mcp?tenant=updated".to_string(),
            BTreeMap::from([("authorization".to_string(), "Bearer new".to_string())]),
            McpProtocolVersionPreference::V2025_06_18,
        )
        .await
        .unwrap();
    assert_eq!(updated.display_name, "Updated");
    assert_eq!(
        updated.endpoint,
        "https://user:pass@example.com/mcp?tenant=updated"
    );
    assert_eq!(
        updated.protocol_version,
        McpProtocolVersionPreference::V2025_06_18
    );
    assert!(repository.catalogs.lock().unwrap().is_empty());
    assert!(service.catalog_snapshots.read().unwrap().is_empty());

    service.discover_tools(&created.id).await.unwrap();
    assert_eq!(gateway.discovery_calls.load(Ordering::Relaxed), 2);
    assert_eq!(
        gateway.request_headers.lock().unwrap().last(),
        Some(&BTreeMap::from([(
            "authorization".to_string(),
            "Bearer new".to_string(),
        )]))
    );
    assert_eq!(
        gateway.protocol_versions.lock().unwrap().last(),
        Some(&McpProtocolVersionPreference::V2025_06_18)
    );
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
            BTreeMap::new(),
            McpProtocolVersionPreference::Auto,
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
            BTreeMap::new(),
            McpProtocolVersionPreference::Auto,
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
            BTreeMap::from([("x-api-key".to_string(), "fixture-secret".to_string())]),
            McpProtocolVersionPreference::Auto,
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

    service.start_call("call-1").await.unwrap();
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
        McpCallOutcomeDto::KnownResponse {
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
    assert_eq!(
        gateway.request_headers.lock().unwrap().as_slice(),
        [BTreeMap::from([(
            "x-api-key".to_string(),
            "fixture-secret".to_string(),
        )])]
    );

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
            BTreeMap::new(),
            McpProtocolVersionPreference::Auto,
        )
        .await
        .unwrap();
    service
        .set_server_state(&created.id, McpServerState::Active)
        .await
        .unwrap();

    service.start_call("call-early").await.unwrap();
    service.cancel_call("call-early").await.unwrap();
    let outcome = service
        .test_call(
            "call-early",
            &created.id,
            "search".to_string(),
            "{}".to_string(),
        )
        .await
        .unwrap();

    assert!(matches!(outcome, McpCallOutcomeDto::NotSent { .. }));
    assert!(gateway.calls.lock().unwrap().is_empty());
    assert!(service.calls.calls.lock().await.is_empty());
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
            BTreeMap::new(),
            McpProtocolVersionPreference::Auto,
        )
        .await
        .unwrap();
    service.start_call("call-paused").await.unwrap();

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
        McpCallOutcomeDto::NotSent { ref code, .. }
            if code == "mcp.call_server_paused"
    ));
    assert!(gateway.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn model_catalog_is_cached_only_and_ask_executes_like_allow() {
    let repository = Arc::new(MemoryRepository::default());
    let gateway = Arc::new(FixedGateway::default());
    let service = McpService::new(repository, gateway.clone());
    let created = service
        .create_server(
            "My Server".to_string(),
            "http://127.0.0.1:3333/mcp".to_string(),
            BTreeMap::new(),
            McpProtocolVersionPreference::Auto,
        )
        .await
        .unwrap();
    service
        .set_server_state(&created.id, McpServerState::Active)
        .await
        .unwrap();
    service.discover_tools(&created.id).await.unwrap();
    service
        .set_tool_permission(&created.id, "search".to_string(), McpToolPermission::Ask)
        .await
        .unwrap();
    service.clear_catalog_memory();

    let tool_id = ToolId::parse(format!("mcp/{}:search", created.id)).unwrap();
    let resolved = service
        .resolve_permitted_model_tools_cached(std::slice::from_ref(&tool_id))
        .await
        .unwrap();
    assert_eq!(resolved.tools.len(), 1);
    assert!(resolved.diagnostics.is_empty());
    assert_eq!(gateway.discovery_calls.load(Ordering::Relaxed), 1);

    let outcome = service
        .call_permitted_tool(
            &tool_id,
            json!({ "query": "rust" }),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(matches!(outcome, McpCallOutcome::KnownResponse(_)));
    assert_eq!(gateway.calls.lock().unwrap().len(), 1);

    service
        .set_tool_permission(&created.id, "search".to_string(), McpToolPermission::Off)
        .await
        .unwrap();
    let outcome = service
        .call_permitted_tool(&tool_id, json!({}), CancellationToken::new())
        .await
        .unwrap();
    assert!(matches!(
        outcome,
        McpCallOutcome::NotSent(McpCallIssue { ref code, .. })
            if code == "mcp.call_permission_off"
    ));
    assert_eq!(gateway.calls.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn model_catalog_skips_cache_when_no_tool_is_permitted() {
    let repository = Arc::new(MemoryRepository::default());
    let gateway = Arc::new(FixedGateway::default());
    let service = McpService::new(repository.clone(), gateway.clone());
    let created = service
        .create_server(
            "No permissions".to_string(),
            "http://127.0.0.1:3333/mcp".to_string(),
            BTreeMap::new(),
            McpProtocolVersionPreference::Auto,
        )
        .await
        .unwrap();
    service
        .set_server_state(&created.id, McpServerState::Active)
        .await
        .unwrap();

    let listed = service.list_legacy_tools_cached().await.unwrap();

    assert!(listed.tools.is_empty());
    assert!(listed.diagnostics.is_empty());
    assert_eq!(repository.catalog_loads.load(Ordering::Relaxed), 0);
    assert_eq!(gateway.discovery_calls.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn model_catalog_is_cached_only_and_localizes_registration_failures() {
    let repository = Arc::new(MemoryRepository::default());
    let gateway = Arc::new(FixedGateway::default());
    let service = McpService::new(repository.clone(), gateway.clone());

    let healthy = service
        .create_server(
            "Healthy".to_string(),
            "http://127.0.0.1:3333/mcp".to_string(),
            BTreeMap::new(),
            McpProtocolVersionPreference::Auto,
        )
        .await
        .unwrap();
    service
        .set_server_state(&healthy.id, McpServerState::Active)
        .await
        .unwrap();
    service.discover_tools(&healthy.id).await.unwrap();
    service
        .set_tool_permission(&healthy.id, "search".to_string(), McpToolPermission::Allow)
        .await
        .unwrap();
    service
        .set_tool_permission(&healthy.id, "scalar".to_string(), McpToolPermission::Allow)
        .await
        .unwrap();
    service
        .set_tool_permission(&healthy.id, "removed".to_string(), McpToolPermission::Ask)
        .await
        .unwrap();

    let healthy_id = McpRegistrationId::parse(&healthy.id).unwrap();
    let (healthy_endpoint, mut snapshot) = repository
        .catalogs
        .lock()
        .unwrap()
        .get(&healthy_id)
        .cloned()
        .unwrap();
    snapshot.tools.push(McpDiscoveredTool {
        native_name: "scalar".to_string(),
        title: Some("Scalar".to_string()),
        description: None,
        input_schema: json!({ "type": "string" }),
        output_schema: None,
        annotations: json!({}),
    });
    repository
        .catalogs
        .lock()
        .unwrap()
        .insert(healthy_id.clone(), (healthy_endpoint, snapshot.clone()));

    let corrupt = service
        .create_server(
            "Corrupt".to_string(),
            "http://127.0.0.1:3334/mcp".to_string(),
            BTreeMap::new(),
            McpProtocolVersionPreference::Auto,
        )
        .await
        .unwrap();
    service
        .set_tool_permission(&corrupt.id, "search".to_string(), McpToolPermission::Ask)
        .await
        .unwrap();
    service
        .set_server_state(&corrupt.id, McpServerState::Active)
        .await
        .unwrap();
    repository.catalogs.lock().unwrap().insert(
        McpRegistrationId::parse(&corrupt.id).unwrap(),
        ("https://wrong.example/mcp".to_string(), snapshot.clone()),
    );

    let missing = service
        .create_server(
            "Missing".to_string(),
            "http://127.0.0.1:3335/mcp".to_string(),
            BTreeMap::new(),
            McpProtocolVersionPreference::Auto,
        )
        .await
        .unwrap();
    service
        .set_tool_permission(&missing.id, "search".to_string(), McpToolPermission::Allow)
        .await
        .unwrap();
    service
        .set_server_state(&missing.id, McpServerState::Active)
        .await
        .unwrap();

    service.clear_catalog_memory();
    repository.catalog_loads.store(0, Ordering::Relaxed);
    let listed = service.list_legacy_tools_cached().await.unwrap();

    assert_eq!(listed.tools.len(), 1);
    assert_eq!(listed.tools[0].tool_id.native_name(), "search");
    assert_eq!(listed.tools[0].server_display_name, "Healthy");
    let wire = serde_json::to_value(&listed).unwrap();
    assert_eq!(wire["tools"][0]["toolId"], listed.tools[0].tool_id.as_str());
    assert!(wire["tools"][0].get("permission").is_none());
    assert!(wire["tools"][0].get("outputSchema").is_none());
    assert!(listed.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "mcp.model_input_schema_unsupported"
            && diagnostic
                .tool_id
                .as_ref()
                .is_some_and(|id| id.native_name() == "scalar")
    }));
    assert!(listed.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "mcp.tool_not_in_cached_catalog"
            && diagnostic
                .tool_id
                .as_ref()
                .is_some_and(|id| id.native_name() == "removed")
    }));
    assert!(
        listed
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "mcp.catalog_snapshot_invalid")
    );
    assert!(
        listed
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "mcp.catalog_not_cached")
    );
    assert_eq!(repository.catalog_loads.load(Ordering::Relaxed), 3);
    assert_eq!(gateway.discovery_calls.load(Ordering::Relaxed), 1);

    let healthy_tool = ToolId::parse(format!("mcp/{}:search", healthy.id)).unwrap();
    let corrupt_tool = ToolId::parse(format!("mcp/{}:search", corrupt.id)).unwrap();
    let resolved = service
        .resolve_permitted_model_tools_cached(&[healthy_tool, corrupt_tool])
        .await
        .unwrap();
    assert_eq!(resolved.tools.len(), 1);
    assert!(
        resolved
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "mcp.catalog_snapshot_invalid")
    );
    assert_eq!(gateway.discovery_calls.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn legacy_call_uses_shared_permission_gate_and_preserves_raw_json() {
    let repository = Arc::new(MemoryRepository::default());
    let gateway = Arc::new(FixedGateway::default());
    let service = McpService::new(repository, gateway.clone());
    let created = service
        .create_server(
            "Fixture".to_string(),
            "http://127.0.0.1:3333/mcp".to_string(),
            BTreeMap::new(),
            McpProtocolVersionPreference::Auto,
        )
        .await
        .unwrap();
    service
        .set_server_state(&created.id, McpServerState::Active)
        .await
        .unwrap();
    let tool_id = ToolId::parse(format!("mcp/{}:search", created.id)).unwrap();

    service.start_call("legacy-off").await.unwrap();
    let outcome = service
        .call_legacy_tool("legacy-off", &tool_id, String::new())
        .await
        .unwrap();
    assert!(matches!(
        outcome,
        McpCallOutcomeDto::NotSent { ref code, .. }
            if code == "mcp.call_permission_off"
    ));
    assert!(gateway.calls.lock().unwrap().is_empty());

    service
        .set_tool_permission(&created.id, "search".to_string(), McpToolPermission::Ask)
        .await
        .unwrap();
    service.start_call("legacy-empty").await.unwrap();
    let outcome = service
        .call_legacy_tool("legacy-empty", &tool_id, String::new())
        .await
        .unwrap();
    assert!(matches!(outcome, McpCallOutcomeDto::KnownResponse { .. }));

    service
        .set_tool_permission(&created.id, "search".to_string(), McpToolPermission::Allow)
        .await
        .unwrap();
    service.start_call("legacy-integer").await.unwrap();
    service
        .call_legacy_tool(
            "legacy-integer",
            &tool_id,
            r#"{"value":9007199254740993}"#.to_string(),
        )
        .await
        .unwrap();
    let calls = gateway.calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    assert!(calls[0].1.is_empty());
    assert_eq!(calls[1].1["value"].to_string(), "9007199254740993");
}

#[tokio::test]
async fn legacy_call_rejects_invalid_arguments_before_the_gateway() {
    let repository = Arc::new(MemoryRepository::default());
    let gateway = Arc::new(FixedGateway::default());
    let service = McpService::new(repository, gateway.clone());
    let created = service
        .create_server(
            "Fixture".to_string(),
            "http://127.0.0.1:3333/mcp".to_string(),
            BTreeMap::new(),
            McpProtocolVersionPreference::Auto,
        )
        .await
        .unwrap();
    service
        .set_tool_permission(&created.id, "search".to_string(), McpToolPermission::Allow)
        .await
        .unwrap();
    service
        .set_server_state(&created.id, McpServerState::Active)
        .await
        .unwrap();
    let tool_id = ToolId::parse(format!("mcp/{}:search", created.id)).unwrap();

    for (call_id, arguments, expected_code) in [
        (
            "legacy-json",
            "{".to_string(),
            "mcp.call_arguments_invalid_json",
        ),
        (
            "legacy-array",
            "[]".to_string(),
            "mcp.call_arguments_not_object",
        ),
        (
            "legacy-large",
            "x".repeat(super::MAX_ARGUMENTS_JSON_BYTES + 1),
            "mcp.call_arguments_size_limit",
        ),
    ] {
        service.start_call(call_id).await.unwrap();
        let outcome = service
            .call_legacy_tool(call_id, &tool_id, arguments)
            .await
            .unwrap();
        assert!(matches!(
            outcome,
            McpCallOutcomeDto::NotSent { ref code, .. } if code == expected_code
        ));
    }
    assert!(gateway.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn empty_agent_selection_does_not_touch_mcp_storage() {
    let repository = Arc::new(MemoryRepository::default());
    repository.fail_scan.store(true, Ordering::Relaxed);
    let service = McpService::new(repository, Arc::new(FixedGateway::default()));

    let resolved = service
        .resolve_permitted_model_tools_cached(&[])
        .await
        .unwrap();

    assert!(resolved.tools.is_empty());
    assert!(resolved.diagnostics.is_empty());
}

#[tokio::test]
async fn model_catalog_reports_registration_storage_issues() {
    let repository = Arc::new(MemoryRepository::default());
    let registration_id = McpRegistrationId::parse("550e8400-e29b-41d4-a716-446655440000").unwrap();
    repository
        .scan_issues
        .lock()
        .unwrap()
        .push(McpRegistrationStorageIssue {
            registration_id: Some(registration_id.clone()),
            file_name: format!("{registration_id}.json"),
            message: "invalid registration JSON".to_string(),
        });
    let service = McpService::new(repository, Arc::new(FixedGateway::default()));
    let tool_id = ToolId::parse(format!("mcp/{registration_id}:search")).unwrap();

    let listed = service.list_permitted_model_tools_cached().await.unwrap();
    let resolved = service
        .resolve_permitted_model_tools_cached(&[tool_id])
        .await
        .unwrap();

    assert_eq!(listed.diagnostics[0].code, "mcp.registration_storage_issue");
    assert_eq!(
        resolved.diagnostics[0].code,
        "mcp.registration_storage_issue"
    );
    assert!(
        resolved.diagnostics[0]
            .message
            .contains("invalid registration JSON")
    );
}
