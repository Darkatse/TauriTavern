use std::sync::Arc;

use tt_contracts::byte_commit::CommitBegin;
use tt_contracts::extension_store::{EntryKind, WriteOperation};
use tt_ports::byte_reader::ByteReader;
use tt_ports::repositories::extension_store_repository::ExtensionStoreRepository;

use crate::errors::ApplicationError;

pub struct ExtensionStoreService {
    repository: Arc<dyn ExtensionStoreRepository>,
}

fn resolve_table(table: Option<&str>) -> &str {
    table
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("main")
}

impl ExtensionStoreService {
    pub fn new(repository: Arc<dyn ExtensionStoreRepository>) -> Self {
        Self { repository }
    }

    pub async fn open_entry(
        &self,
        namespace: &str,
        table: Option<&str>,
        key: &str,
        kind: EntryKind,
    ) -> Result<Option<Box<dyn ByteReader>>, ApplicationError> {
        Ok(self
            .repository
            .open_entry(namespace, resolve_table(table), key, kind)
            .await?)
    }

    pub async fn begin_commit(
        &self,
        namespace: &str,
        table: Option<&str>,
        key: &str,
        operation: WriteOperation,
    ) -> Result<CommitBegin, ApplicationError> {
        Ok(self
            .repository
            .begin_commit(namespace, resolve_table(table), key, operation)
            .await?)
    }

    pub async fn append_commit(
        &self,
        session_id: &str,
        offset: u64,
        bytes: &[u8],
    ) -> Result<u64, ApplicationError> {
        Ok(self
            .repository
            .append_commit(session_id, offset, bytes)
            .await?)
    }

    pub async fn finish_commit(
        &self,
        session_id: &str,
        expected_size: u64,
    ) -> Result<(), ApplicationError> {
        Ok(self
            .repository
            .finish_commit(session_id, expected_size)
            .await?)
    }

    pub async fn abort_commit(&self, session_id: &str) -> Result<(), ApplicationError> {
        Ok(self.repository.abort_commit(session_id).await?)
    }

    pub async fn rename_json_key(
        &self,
        namespace: &str,
        table: Option<&str>,
        key: &str,
        new_key: &str,
    ) -> Result<(), ApplicationError> {
        Ok(self
            .repository
            .rename_json_key(namespace, resolve_table(table), key, new_key)
            .await?)
    }

    pub async fn delete_entry(
        &self,
        namespace: &str,
        table: Option<&str>,
        key: &str,
        kind: EntryKind,
    ) -> Result<(), ApplicationError> {
        Ok(self
            .repository
            .delete_entry(namespace, resolve_table(table), key, kind)
            .await?)
    }

    pub async fn list_keys(
        &self,
        namespace: &str,
        table: Option<&str>,
        kind: EntryKind,
    ) -> Result<Vec<String>, ApplicationError> {
        Ok(self
            .repository
            .list_keys(namespace, resolve_table(table), kind)
            .await?)
    }

    pub async fn list_tables(&self, namespace: &str) -> Result<Vec<String>, ApplicationError> {
        Ok(self.repository.list_tables(namespace).await?)
    }

    pub async fn delete_table(&self, namespace: &str, table: &str) -> Result<(), ApplicationError> {
        Ok(self.repository.delete_table(namespace, table).await?)
    }
}
