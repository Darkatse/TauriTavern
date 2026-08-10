use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::file_system::{delete_file, list_files_with_extension, read_json_file, write_json_file};
use tt_domain::{
    errors::DomainError,
    models::mcp::{
        McpEndpoint, McpProtocolVersionPreference, McpRegistrationId, McpServerRegistration,
        McpServerState, McpToolPermission,
    },
};
use tt_ports::mcp::McpDiscoveryResult;
use tt_ports::repositories::mcp_server_repository::{
    McpRegistrationScan, McpRegistrationStorageIssue, McpServerRepository,
};

const MCP_REGISTRATION_SCHEMA_VERSION: u32 = 1;
const MCP_REGISTRATION_KIND: &str = "tauritavern.mcpServerRegistration";
const MCP_CATALOG_SCHEMA_VERSION: u32 = 1;
const MCP_CATALOG_KIND: &str = "tauritavern.mcpCatalogSnapshot";

pub struct FileMcpServerRepository {
    root: PathBuf,
}

impl FileMcpServerRepository {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn registrations_dir(&self) -> PathBuf {
        self.root.join("registrations")
    }

    fn registration_path(&self, id: &McpRegistrationId) -> PathBuf {
        self.registrations_dir().join(format!("{id}.json"))
    }

    fn catalogs_dir(&self) -> PathBuf {
        self.root.join("catalogs")
    }

    fn catalog_path(&self, id: &McpRegistrationId) -> PathBuf {
        self.catalogs_dir().join(format!("{id}.json"))
    }

    async fn load_file(
        &self,
        path: &Path,
        expected_id: &McpRegistrationId,
    ) -> Result<McpServerRegistration, DomainError> {
        let stored: StoredMcpRegistrationV1 = read_json_file(path).await?;
        stored.into_domain(expected_id, path)
    }

    async fn load_catalog_file(
        &self,
        path: &Path,
        expected_id: &McpRegistrationId,
        expected_endpoint: &McpEndpoint,
    ) -> Result<McpDiscoveryResult, DomainError> {
        let stored: StoredMcpCatalogSnapshotV1 = read_json_file(path).await?;
        stored.into_port(expected_id, expected_endpoint, path)
    }
}

#[async_trait]
impl McpServerRepository for FileMcpServerRepository {
    async fn scan(&self) -> Result<McpRegistrationScan, DomainError> {
        let mut paths = list_files_with_extension(&self.registrations_dir(), "json").await?;
        paths.sort();
        let mut scan = McpRegistrationScan::default();

        for path in paths {
            let file_name = path
                .file_name()
                .map(|value| value.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string());
            let Some(file_id) = path.file_stem().and_then(|value| value.to_str()) else {
                scan.issues.push(McpRegistrationStorageIssue {
                    registration_id: None,
                    file_name,
                    message: "mcp.registration_filename_utf8: filename is not valid UTF-8"
                        .to_string(),
                });
                continue;
            };
            let id = match McpRegistrationId::parse(file_id) {
                Ok(id) => id,
                Err(error) => {
                    scan.issues.push(McpRegistrationStorageIssue {
                        registration_id: None,
                        file_name,
                        message: error.to_string(),
                    });
                    continue;
                }
            };
            match self.load_file(&path, &id).await {
                Ok(registration) => scan.registrations.push(registration),
                Err(DomainError::InvalidData(message) | DomainError::NotFound(message)) => {
                    scan.issues.push(McpRegistrationStorageIssue {
                        registration_id: Some(id),
                        file_name,
                        message,
                    });
                }
                Err(error) => return Err(error),
            }
        }

        Ok(scan)
    }

    async fn load(
        &self,
        id: &McpRegistrationId,
    ) -> Result<Option<McpServerRegistration>, DomainError> {
        let path = self.registration_path(id);
        if !path.exists() {
            return Ok(None);
        }
        self.load_file(&path, id).await.map(Some)
    }

    async fn save(&self, registration: &McpServerRegistration) -> Result<(), DomainError> {
        write_json_file(
            &self.registration_path(registration.id()),
            &StoredMcpRegistrationV1::from_domain(registration),
        )
        .await
    }

    async fn load_catalog_snapshot(
        &self,
        id: &McpRegistrationId,
        endpoint: &McpEndpoint,
    ) -> Result<Option<McpDiscoveryResult>, DomainError> {
        let path = self.catalog_path(id);
        if !path.exists() {
            return Ok(None);
        }
        self.load_catalog_file(&path, id, endpoint).await.map(Some)
    }

    async fn save_catalog_snapshot(
        &self,
        id: &McpRegistrationId,
        endpoint: &McpEndpoint,
        snapshot: &McpDiscoveryResult,
    ) -> Result<(), DomainError> {
        write_json_file(
            &self.catalog_path(id),
            &StoredMcpCatalogSnapshotV1::from_port(id, endpoint, snapshot),
        )
        .await
    }

    async fn remove_catalog_snapshot(&self, id: &McpRegistrationId) -> Result<(), DomainError> {
        delete_file(&self.catalog_path(id)).await
    }

    async fn remove(&self, id: &McpRegistrationId) -> Result<(), DomainError> {
        delete_file(&self.registration_path(id)).await?;
        if let Err(error) = delete_file(&self.catalog_path(id)).await {
            tracing::warn!(registration_id = %id, %error, "Failed to clean up removed MCP catalog snapshot");
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredMcpRegistrationV1 {
    schema_version: u32,
    kind: String,
    id: String,
    display_name: String,
    endpoint: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    headers: BTreeMap<String, String>,
    #[serde(default)]
    protocol_version: McpProtocolVersionPreference,
    state: McpServerState,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    tool_permissions: BTreeMap<String, McpToolPermission>,
}

impl StoredMcpRegistrationV1 {
    fn from_domain(registration: &McpServerRegistration) -> Self {
        Self {
            schema_version: MCP_REGISTRATION_SCHEMA_VERSION,
            kind: MCP_REGISTRATION_KIND.to_string(),
            id: registration.id().to_string(),
            display_name: registration.display_name().to_string(),
            endpoint: registration.endpoint().as_str().to_string(),
            headers: registration.request_headers().as_map().clone(),
            protocol_version: registration.protocol_version(),
            state: registration.state(),
            tool_permissions: registration.tool_permissions().clone(),
        }
    }

    fn into_domain(
        self,
        expected_id: &McpRegistrationId,
        path: &Path,
    ) -> Result<McpServerRegistration, DomainError> {
        if self.schema_version != MCP_REGISTRATION_SCHEMA_VERSION {
            return Err(DomainError::InvalidData(format!(
                "mcp.registration_schema_unsupported: schemaVersion {} in {}",
                self.schema_version,
                path.display()
            )));
        }
        if self.kind != MCP_REGISTRATION_KIND {
            return Err(DomainError::InvalidData(format!(
                "mcp.registration_kind_invalid: kind `{}` in {}",
                self.kind,
                path.display()
            )));
        }
        let id = McpRegistrationId::parse(&self.id)?;
        if id != *expected_id {
            return Err(DomainError::InvalidData(format!(
                "mcp.registration_id_mismatch: id `{id}` does not match filename `{expected_id}` in {}",
                path.display()
            )));
        }
        McpServerRegistration::try_new(
            id,
            self.display_name,
            McpEndpoint::parse(self.endpoint)?,
            self.headers,
            self.protocol_version,
            self.state,
            self.tool_permissions,
        )
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredMcpCatalogSnapshotV1 {
    schema_version: u32,
    kind: String,
    registration_id: String,
    endpoint: String,
    catalog: McpDiscoveryResult,
}

impl StoredMcpCatalogSnapshotV1 {
    fn from_port(
        id: &McpRegistrationId,
        endpoint: &McpEndpoint,
        snapshot: &McpDiscoveryResult,
    ) -> Self {
        Self {
            schema_version: MCP_CATALOG_SCHEMA_VERSION,
            kind: MCP_CATALOG_KIND.to_string(),
            registration_id: id.to_string(),
            endpoint: endpoint.as_str().to_string(),
            catalog: snapshot.clone(),
        }
    }

    fn into_port(
        self,
        expected_id: &McpRegistrationId,
        expected_endpoint: &McpEndpoint,
        path: &Path,
    ) -> Result<McpDiscoveryResult, DomainError> {
        if self.schema_version != MCP_CATALOG_SCHEMA_VERSION {
            return Err(DomainError::InvalidData(format!(
                "mcp.catalog_schema_unsupported: schemaVersion {} in {}",
                self.schema_version,
                path.display()
            )));
        }
        if self.kind != MCP_CATALOG_KIND {
            return Err(DomainError::InvalidData(format!(
                "mcp.catalog_kind_invalid: kind `{}` in {}",
                self.kind,
                path.display()
            )));
        }
        let id = McpRegistrationId::parse(&self.registration_id)?;
        if id != *expected_id {
            return Err(DomainError::InvalidData(format!(
                "mcp.catalog_registration_mismatch: id `{id}` does not match registration `{expected_id}` in {}",
                path.display()
            )));
        }
        if self.endpoint != expected_endpoint.as_str() {
            return Err(DomainError::InvalidData(format!(
                "mcp.catalog_endpoint_mismatch: endpoint `{}` does not match registration endpoint `{}` in {}",
                self.endpoint,
                expected_endpoint.as_str(),
                path.display()
            )));
        }

        Ok(self.catalog)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    use serde_json::{Value, json};
    use tt_ports::mcp::{McpDiscoveredTool, McpToolDiagnostic};

    use super::*;

    static NEXT_TEST_DIR_ID: AtomicU64 = AtomicU64::new(0);

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let suffix = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let counter = NEXT_TEST_DIR_ID.fetch_add(1, Ordering::Relaxed);
            Self(std::env::temp_dir().join(format!(
                "tauritavern-mcp-repository-test-{}-{suffix}-{counter}",
                std::process::id()
            )))
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn registration(id: &str) -> McpServerRegistration {
        McpServerRegistration::try_new(
            McpRegistrationId::parse(id).unwrap(),
            "Local MCP",
            McpEndpoint::parse("http://127.0.0.1:3333/mcp").unwrap(),
            BTreeMap::from([("x-api-key".to_string(), "fixture-secret".to_string())]),
            McpProtocolVersionPreference::Auto,
            McpServerState::Paused,
            BTreeMap::new(),
        )
        .unwrap()
    }

    fn catalog() -> McpDiscoveryResult {
        McpDiscoveryResult {
            protocol_version: "2026-07-28".to_string(),
            server_name: Some("Fixture".to_string()),
            server_version: Some("1.0".to_string()),
            tools: vec![McpDiscoveredTool {
                native_name: "search".to_string(),
                title: Some("Search".to_string()),
                description: Some("Search fixture data".to_string()),
                input_schema: json!({ "type": "object" }),
                output_schema: None,
                annotations: json!({ "readOnlyHint": true }),
            }],
            diagnostics: vec![McpToolDiagnostic {
                code: "fixture.warning".to_string(),
                native_name: Some("search".to_string()),
                message: "Fixture diagnostic".to_string(),
            }],
        }
    }

    #[tokio::test]
    async fn round_trip_uses_one_strict_file_per_registration() {
        let dir = TestDir::new();
        let repository = FileMcpServerRepository::new(dir.path().to_path_buf());
        let expected = registration("550e8400-e29b-41d4-a716-446655440000");

        repository.save(&expected).await.unwrap();
        let loaded = repository.load(expected.id()).await.unwrap().unwrap();

        assert_eq!(loaded, expected);
        assert!(
            dir.path()
                .join("registrations/550e8400-e29b-41d4-a716-446655440000.json")
                .is_file()
        );
    }

    #[tokio::test]
    async fn catalog_snapshot_is_strict_endpoint_bound_and_removed_with_registration() {
        let dir = TestDir::new();
        let repository = FileMcpServerRepository::new(dir.path().to_path_buf());
        let registration = registration("550e8400-e29b-41d4-a716-446655440000");
        let expected = catalog();
        repository.save(&registration).await.unwrap();
        repository
            .save_catalog_snapshot(registration.id(), registration.endpoint(), &expected)
            .await
            .unwrap();

        let reopened = FileMcpServerRepository::new(dir.path().to_path_buf());
        assert_eq!(
            reopened
                .load_catalog_snapshot(registration.id(), registration.endpoint())
                .await
                .unwrap(),
            Some(expected.clone())
        );
        let other_endpoint = McpEndpoint::parse("https://example.com/mcp").unwrap();
        assert!(
            reopened
                .load_catalog_snapshot(registration.id(), &other_endpoint)
                .await
                .unwrap_err()
                .to_string()
                .contains("mcp.catalog_endpoint_mismatch")
        );

        let catalog_path = dir
            .path()
            .join("catalogs/550e8400-e29b-41d4-a716-446655440000.json");
        let mut stored: Value =
            serde_json::from_slice(&tokio::fs::read(&catalog_path).await.unwrap()).unwrap();
        stored["unknown"] = Value::Bool(true);
        tokio::fs::write(&catalog_path, serde_json::to_vec(&stored).unwrap())
            .await
            .unwrap();
        assert!(
            reopened
                .load_catalog_snapshot(registration.id(), registration.endpoint())
                .await
                .unwrap_err()
                .to_string()
                .contains("unknown field")
        );

        reopened
            .save_catalog_snapshot(registration.id(), registration.endpoint(), &expected)
            .await
            .unwrap();
        reopened
            .remove_catalog_snapshot(registration.id())
            .await
            .unwrap();
        assert!(!catalog_path.exists());
        assert!(reopened.load(registration.id()).await.unwrap().is_some());
        reopened
            .save_catalog_snapshot(registration.id(), registration.endpoint(), &expected)
            .await
            .unwrap();
        reopened.remove(registration.id()).await.unwrap();
        assert!(!catalog_path.exists());
        assert!(
            !dir.path()
                .join("registrations/550e8400-e29b-41d4-a716-446655440000.json")
                .exists()
        );
    }

    #[tokio::test]
    async fn catalog_cleanup_failure_does_not_block_registration_removal() {
        let dir = TestDir::new();
        let repository = FileMcpServerRepository::new(dir.path().to_path_buf());
        let registration = registration("550e8400-e29b-41d4-a716-446655440000");
        repository.save(&registration).await.unwrap();
        let catalog_path = dir
            .path()
            .join("catalogs/550e8400-e29b-41d4-a716-446655440000.json");
        tokio::fs::create_dir_all(&catalog_path).await.unwrap();

        repository.remove(registration.id()).await.unwrap();

        assert!(catalog_path.is_dir());
        assert!(
            !dir.path()
                .join("registrations/550e8400-e29b-41d4-a716-446655440000.json")
                .exists()
        );
    }

    #[tokio::test]
    async fn scan_isolates_corrupt_files_without_hiding_healthy_registrations() {
        let dir = TestDir::new();
        let repository = FileMcpServerRepository::new(dir.path().to_path_buf());
        let expected = registration("550e8400-e29b-41d4-a716-446655440000");
        repository.save(&expected).await.unwrap();
        let corrupt_path = dir
            .path()
            .join("registrations/550e8400-e29b-41d4-a716-446655440001.json");
        tokio::fs::write(&corrupt_path, b"{not json").await.unwrap();

        let scan = repository.scan().await.unwrap();

        assert_eq!(scan.registrations, vec![expected]);
        assert_eq!(scan.issues.len(), 1);
        assert_eq!(
            scan.issues[0].file_name,
            "550e8400-e29b-41d4-a716-446655440001.json"
        );
    }

    #[tokio::test]
    async fn scan_rejects_unknown_fields_and_filename_identity_mismatch() {
        let dir = TestDir::new();
        let registrations = dir.path().join("registrations");
        tokio::fs::create_dir_all(&registrations).await.unwrap();
        let unknown_field = json!({
            "schemaVersion": 1,
            "kind": MCP_REGISTRATION_KIND,
            "id": "550e8400-e29b-41d4-a716-446655440000",
            "displayName": "Unknown field",
            "endpoint": "https://example.com/mcp",
            "state": "paused",
            "unknown": true
        });
        let mismatched_id = json!({
            "schemaVersion": 1,
            "kind": MCP_REGISTRATION_KIND,
            "id": "550e8400-e29b-41d4-a716-446655440001",
            "displayName": "Mismatched ID",
            "endpoint": "https://example.com/mcp",
            "state": "paused"
        });
        for (file_name, value) in [
            ("550e8400-e29b-41d4-a716-446655440000.json", unknown_field),
            ("550e8400-e29b-41d4-a716-446655440002.json", mismatched_id),
        ] {
            tokio::fs::write(
                registrations.join(file_name),
                serde_json::to_vec(&value).unwrap(),
            )
            .await
            .unwrap();
        }

        let scan = FileMcpServerRepository::new(dir.path().to_path_buf())
            .scan()
            .await
            .unwrap();

        assert!(scan.registrations.is_empty());
        assert_eq!(scan.issues.len(), 2);
        assert!(
            scan.issues
                .iter()
                .any(|issue| issue.message.contains("unknown field"))
        );
        assert!(
            scan.issues
                .iter()
                .any(|issue| issue.message.contains("does not match filename"))
        );
    }
}
