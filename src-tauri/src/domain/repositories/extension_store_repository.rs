use async_trait::async_trait;
use serde_json::Value;

use crate::domain::errors::DomainError;

#[async_trait]
pub trait ExtensionStoreRepository: Send + Sync {
    async fn get_json(&self, namespace: &str, table: &str, key: &str)
    -> Result<Value, DomainError>;

    async fn set_json(
        &self,
        namespace: &str,
        table: &str,
        key: &str,
        value: Value,
    ) -> Result<(), DomainError>;

    async fn update_json(
        &self,
        namespace: &str,
        table: &str,
        key: &str,
        value: Value,
    ) -> Result<(), DomainError>;

    async fn rename_json_key(
        &self,
        namespace: &str,
        table: &str,
        key: &str,
        new_key: &str,
    ) -> Result<(), DomainError>;

    async fn delete_json(&self, namespace: &str, table: &str, key: &str)
    -> Result<(), DomainError>;

    async fn list_json_keys(
        &self,
        namespace: &str,
        table: &str,
    ) -> Result<Vec<String>, DomainError>;

    async fn list_tables(&self, namespace: &str) -> Result<Vec<String>, DomainError>;

    async fn delete_table(&self, namespace: &str, table: &str) -> Result<(), DomainError>;

    async fn get_blob(
        &self,
        namespace: &str,
        table: &str,
        key: &str,
    ) -> Result<Vec<u8>, DomainError>;

    async fn set_blob(
        &self,
        namespace: &str,
        table: &str,
        key: &str,
        bytes: Vec<u8>,
    ) -> Result<(), DomainError>;

    async fn delete_blob(&self, namespace: &str, table: &str, key: &str)
    -> Result<(), DomainError>;

    async fn list_blob_keys(
        &self,
        namespace: &str,
        table: &str,
    ) -> Result<Vec<String>, DomainError>;
}
