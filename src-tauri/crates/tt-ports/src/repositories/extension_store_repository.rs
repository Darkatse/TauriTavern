use async_trait::async_trait;

use crate::byte_reader::ByteReader;
use tt_contracts::byte_commit::CommitBegin;
use tt_contracts::extension_store::{EntryKind, WriteOperation};
use tt_domain::errors::DomainError;

#[async_trait]
pub trait ExtensionStoreRepository: Send + Sync {
    /// Open one published version; replacing its path does not change this reader.
    async fn open_entry(
        &self,
        namespace: &str,
        table: &str,
        key: &str,
        kind: EntryKind,
    ) -> Result<Option<Box<dyn ByteReader>>, DomainError>;

    async fn begin_commit(
        &self,
        namespace: &str,
        table: &str,
        key: &str,
        operation: WriteOperation,
    ) -> Result<CommitBegin, DomainError>;

    async fn append_commit(
        &self,
        session_id: &str,
        offset: u64,
        bytes: &[u8],
    ) -> Result<u64, DomainError>;

    async fn finish_commit(&self, session_id: &str, expected_size: u64) -> Result<(), DomainError>;

    async fn abort_commit(&self, session_id: &str) -> Result<(), DomainError>;

    async fn rename_json_key(
        &self,
        namespace: &str,
        table: &str,
        key: &str,
        new_key: &str,
    ) -> Result<(), DomainError>;

    async fn delete_entry(
        &self,
        namespace: &str,
        table: &str,
        key: &str,
        kind: EntryKind,
    ) -> Result<(), DomainError>;

    async fn list_keys(
        &self,
        namespace: &str,
        table: &str,
        kind: EntryKind,
    ) -> Result<Vec<String>, DomainError>;

    async fn list_tables(&self, namespace: &str) -> Result<Vec<String>, DomainError>;

    async fn delete_table(&self, namespace: &str, table: &str) -> Result<(), DomainError>;
}
