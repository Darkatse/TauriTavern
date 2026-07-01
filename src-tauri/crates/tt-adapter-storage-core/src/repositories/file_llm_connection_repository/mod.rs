use std::path::{Path, PathBuf};

use async_trait::async_trait;
use tokio::fs;
use uuid::Uuid;

use crate::file_system::{
    list_files_with_extension, read_json_file, replace_file_with_fallback,
};
use tt_domain::errors::DomainError;
use tt_domain::models::llm_connection::{
    LLM_CONNECTION_KIND, LLM_CONNECTION_SCHEMA_VERSION, LlmConnectionDefinition, LlmConnectionId,
    LlmConnectionSummary,
};
use tt_ports::repositories::llm_connection_repository::LlmConnectionRepository;

pub struct FileLlmConnectionRepository {
    root: PathBuf,
}

impl FileLlmConnectionRepository {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn connections_dir(&self) -> PathBuf {
        self.root.join("connections")
    }

    fn staging_dir(&self) -> PathBuf {
        self.root.join(".staging")
    }

    fn connection_path(&self, id: &LlmConnectionId) -> PathBuf {
        self.connections_dir().join(format!("{}.json", id.as_str()))
    }

    async fn load_connection_file(
        &self,
        path: &Path,
    ) -> Result<LlmConnectionDefinition, DomainError> {
        let file_id = path
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or_else(|| {
                DomainError::InvalidData(format!(
                    "LLM connection filename is not valid UTF-8: {}",
                    path.display()
                ))
            })?;
        let file_id = LlmConnectionId::parse(file_id).map_err(DomainError::InvalidData)?;
        let connection: LlmConnectionDefinition = read_json_file(path).await?;
        validate_connection_file_identity(&connection, &file_id, path)?;
        Ok(connection)
    }
}

#[async_trait]
impl LlmConnectionRepository for FileLlmConnectionRepository {
    async fn list_connections(&self) -> Result<Vec<LlmConnectionSummary>, DomainError> {
        let mut files = list_files_with_extension(&self.connections_dir(), "json").await?;
        files.sort();

        let mut connections = Vec::with_capacity(files.len());
        for path in files {
            connections.push(self.load_connection_file(&path).await?.summary());
        }
        Ok(connections)
    }

    async fn load_connection(
        &self,
        id: &LlmConnectionId,
    ) -> Result<Option<LlmConnectionDefinition>, DomainError> {
        let path = self.connection_path(id);
        if !path.exists() {
            return Ok(None);
        }
        Ok(Some(self.load_connection_file(&path).await?))
    }

    async fn save_connection(
        &self,
        connection: &LlmConnectionDefinition,
    ) -> Result<(), DomainError> {
        validate_connection_file_identity(
            connection,
            &connection.id,
            &self.connection_path(&connection.id),
        )?;

        fs::create_dir_all(self.connections_dir())
            .await
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to create LLM connection directory: {error}"
                ))
            })?;
        fs::create_dir_all(self.staging_dir())
            .await
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to create LLM connection staging: {error}"
                ))
            })?;

        let target = self.connection_path(&connection.id);
        let temp = self.staging_dir().join(format!(
            "{}.{}.json",
            connection.id.as_str(),
            Uuid::new_v4().simple()
        ));
        let json = serde_json::to_string_pretty(connection).map_err(|error| {
            DomainError::InvalidData(format!("Failed to serialize LLM connection: {error}"))
        })?;
        fs::write(&temp, json.as_bytes()).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to write LLM connection staging file {}: {}",
                temp.display(),
                error
            ))
        })?;
        replace_file_with_fallback(&temp, &target).await
    }

    async fn delete_connection(&self, id: &LlmConnectionId) -> Result<(), DomainError> {
        let path = self.connection_path(id);
        fs::remove_file(&path).await.map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                DomainError::NotFound(format!("LLM connection not found: {}", id.as_str()))
            } else {
                DomainError::InternalError(format!(
                    "Failed to delete LLM connection {}: {}",
                    path.display(),
                    error
                ))
            }
        })
    }
}

fn validate_connection_file_identity(
    connection: &LlmConnectionDefinition,
    expected_id: &LlmConnectionId,
    path: &Path,
) -> Result<(), DomainError> {
    if connection.schema_version != LLM_CONNECTION_SCHEMA_VERSION {
        return Err(DomainError::InvalidData(format!(
            "LLM connection schemaVersion {} is unsupported: {}",
            connection.schema_version,
            path.display()
        )));
    }
    if connection.kind != LLM_CONNECTION_KIND {
        return Err(DomainError::InvalidData(format!(
            "LLM connection kind must be {}: {}",
            LLM_CONNECTION_KIND,
            path.display()
        )));
    }
    if connection.id != *expected_id {
        return Err(DomainError::InvalidData(format!(
            "LLM connection id `{}` does not match filename `{}`: {}",
            connection.id.as_str(),
            expected_id.as_str(),
            path.display()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde_json::json;
    use tt_domain::errors::DomainError;
    use tt_ports::repositories::llm_connection_repository::LlmConnectionRepository;

    use super::{
        FileLlmConnectionRepository, LLM_CONNECTION_KIND, LLM_CONNECTION_SCHEMA_VERSION,
        LlmConnectionId,
    };

    static NEXT_TEST_DIR_ID: AtomicU64 = AtomicU64::new(0);

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new() -> Self {
            let suffix = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time should be after unix epoch")
                .as_nanos();
            let counter = NEXT_TEST_DIR_ID.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "tauritavern-llm-connection-repository-test-{}-{}-{}",
                std::process::id(),
                suffix,
                counter
            ));
            std::fs::create_dir_all(path.join("connections")).expect("create temp dir");
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn connection_json(id: &str, kind: &str, schema_version: u32) -> serde_json::Value {
        json!({
            "schemaVersion": schema_version,
            "kind": kind,
            "id": id,
            "displayName": "Test Connection",
            "provider": {
                "chatCompletionSource": "openai"
            },
            "auth": {
                "secretRef": {
                    "key": "api_key_openai",
                    "id": "secret-id"
                }
            }
        })
    }

    async fn write_connection_file(dir: &TestDir, file_id: &str, value: serde_json::Value) {
        let path = dir.path().join("connections").join(format!("{file_id}.json"));
        let json = serde_json::to_vec_pretty(&value).expect("serialize connection json");
        tokio::fs::write(path, json).await.expect("write connection");
    }

    fn assert_invalid_data_contains(error: DomainError, expected: &str) {
        match error {
            DomainError::InvalidData(message) => assert!(
                message.contains(expected),
                "expected `{message}` to contain `{expected}`"
            ),
            other => panic!("expected invalid data error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn load_connection_rejects_id_that_does_not_match_filename() {
        let dir = TestDir::new();
        write_connection_file(
            &dir,
            "expected",
            connection_json("actual", LLM_CONNECTION_KIND, LLM_CONNECTION_SCHEMA_VERSION),
        )
        .await;
        let repository = FileLlmConnectionRepository::new(dir.path().to_path_buf());
        let id = LlmConnectionId::parse("expected").expect("valid id");

        let error = repository
            .load_connection(&id)
            .await
            .expect_err("mismatched id should fail");

        assert_invalid_data_contains(error, "does not match filename");
    }

    #[tokio::test]
    async fn load_connection_rejects_wrong_kind() {
        let dir = TestDir::new();
        write_connection_file(
            &dir,
            "expected",
            connection_json("expected", "wrong.kind", LLM_CONNECTION_SCHEMA_VERSION),
        )
        .await;
        let repository = FileLlmConnectionRepository::new(dir.path().to_path_buf());
        let id = LlmConnectionId::parse("expected").expect("valid id");

        let error = repository
            .load_connection(&id)
            .await
            .expect_err("wrong kind should fail");

        assert_invalid_data_contains(error, "kind must be");
    }

    #[tokio::test]
    async fn load_connection_rejects_wrong_schema_version() {
        let dir = TestDir::new();
        write_connection_file(
            &dir,
            "expected",
            connection_json("expected", LLM_CONNECTION_KIND, LLM_CONNECTION_SCHEMA_VERSION + 1),
        )
        .await;
        let repository = FileLlmConnectionRepository::new(dir.path().to_path_buf());
        let id = LlmConnectionId::parse("expected").expect("valid id");

        let error = repository
            .load_connection(&id)
            .await
            .expect_err("wrong schema version should fail");

        assert_invalid_data_contains(error, "schemaVersion");
    }
}
