use async_trait::async_trait;
use serde_json::Value;
use std::path::Path;

use tt_contracts::byte_commit::CommitBegin;
use tt_domain::errors::DomainError;

#[async_trait]
pub trait WorldInfoRepository: Send + Sync {
    /// Return the document bytes without materializing its fields.
    async fn read_world_info_json(&self, name: &str) -> Result<Option<Vec<u8>>, DomainError>;

    /// Used by Rust use cases that need to interpret or transform the document.
    async fn get_world_info(&self, name: &str) -> Result<Option<Value>, DomainError> {
        match self.read_world_info_json(name).await? {
            Some(bytes) => serde_json::from_slice(&bytes).map(Some).map_err(|error| {
                DomainError::InvalidData(format!("Invalid world info {name}: {error}"))
            }),
            None => Ok(None),
        }
    }

    async fn write_world_info_json(&self, name: &str, json: Vec<u8>) -> Result<(), DomainError>;

    async fn save_world_info(&self, name: &str, data: &Value) -> Result<(), DomainError> {
        let json = serde_json::to_vec(data).map_err(|error| {
            DomainError::InvalidData(format!("Failed to encode world info {name}: {error}"))
        })?;
        self.write_world_info_json(name, json).await
    }

    /// Receive the complete upstream edit request (`{ name, data }`), not a document.
    /// Its target is resolved at finish so all HTTP writers use the same byte path.
    async fn begin_commit(&self) -> Result<CommitBegin, DomainError>;
    async fn append_commit(
        &self,
        session_id: &str,
        offset: u64,
        bytes: &[u8],
    ) -> Result<u64, DomainError>;
    /// Consume the received replacement request and publish its document atomically.
    async fn finish_commit(&self, session_id: &str, expected_size: u64) -> Result<(), DomainError>;
    async fn abort_commit(&self, session_id: &str) -> Result<(), DomainError>;
    async fn delete_world_info(&self, name: &str) -> Result<(), DomainError>;
    async fn import_world_info(
        &self,
        file_path: &Path,
        original_filename: &str,
        converted_data: Option<&str>,
    ) -> Result<String, DomainError>;
    async fn list_world_names(&self) -> Result<Vec<String>, DomainError>;
}
