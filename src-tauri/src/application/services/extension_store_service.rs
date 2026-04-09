use std::sync::Arc;

use serde_json::Value;

use crate::application::errors::ApplicationError;
use crate::domain::repositories::extension_store_repository::ExtensionStoreRepository;

const DEFAULT_TABLE: &str = "main";

pub struct ExtensionStoreService {
    repository: Arc<dyn ExtensionStoreRepository>,
}

impl ExtensionStoreService {
    pub fn new(repository: Arc<dyn ExtensionStoreRepository>) -> Self {
        Self { repository }
    }

    fn resolve_table<'a>(&self, table: Option<&'a str>) -> &'a str {
        table
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(DEFAULT_TABLE)
    }

    pub async fn get_json(
        &self,
        namespace: &str,
        table: Option<&str>,
        key: &str,
    ) -> Result<Value, ApplicationError> {
        let table = self.resolve_table(table);
        Ok(self.repository.get_json(namespace, table, key).await?)
    }

    pub async fn set_json(
        &self,
        namespace: &str,
        table: Option<&str>,
        key: &str,
        value: Value,
    ) -> Result<(), ApplicationError> {
        let table = self.resolve_table(table);
        self.repository
            .set_json(namespace, table, key, value)
            .await?;
        Ok(())
    }

    pub async fn update_json(
        &self,
        namespace: &str,
        table: Option<&str>,
        key: &str,
        value: Value,
    ) -> Result<(), ApplicationError> {
        let table = self.resolve_table(table);
        self.repository
            .update_json(namespace, table, key, value)
            .await?;
        Ok(())
    }

    pub async fn rename_json_key(
        &self,
        namespace: &str,
        table: Option<&str>,
        key: &str,
        new_key: &str,
    ) -> Result<(), ApplicationError> {
        let table = self.resolve_table(table);
        self.repository
            .rename_json_key(namespace, table, key, new_key)
            .await?;
        Ok(())
    }

    pub async fn delete_json(
        &self,
        namespace: &str,
        table: Option<&str>,
        key: &str,
    ) -> Result<(), ApplicationError> {
        let table = self.resolve_table(table);
        self.repository.delete_json(namespace, table, key).await?;
        Ok(())
    }

    pub async fn list_json_keys(
        &self,
        namespace: &str,
        table: Option<&str>,
    ) -> Result<Vec<String>, ApplicationError> {
        let table = self.resolve_table(table);
        Ok(self.repository.list_json_keys(namespace, table).await?)
    }

    pub async fn list_tables(&self, namespace: &str) -> Result<Vec<String>, ApplicationError> {
        Ok(self.repository.list_tables(namespace).await?)
    }

    pub async fn delete_table(&self, namespace: &str, table: &str) -> Result<(), ApplicationError> {
        self.repository.delete_table(namespace, table).await?;
        Ok(())
    }

    pub async fn get_blob(
        &self,
        namespace: &str,
        table: Option<&str>,
        key: &str,
    ) -> Result<Vec<u8>, ApplicationError> {
        let table = self.resolve_table(table);
        Ok(self.repository.get_blob(namespace, table, key).await?)
    }

    pub async fn set_blob(
        &self,
        namespace: &str,
        table: Option<&str>,
        key: &str,
        bytes: Vec<u8>,
    ) -> Result<(), ApplicationError> {
        let table = self.resolve_table(table);
        self.repository
            .set_blob(namespace, table, key, bytes)
            .await?;
        Ok(())
    }

    pub async fn delete_blob(
        &self,
        namespace: &str,
        table: Option<&str>,
        key: &str,
    ) -> Result<(), ApplicationError> {
        let table = self.resolve_table(table);
        self.repository.delete_blob(namespace, table, key).await?;
        Ok(())
    }

    pub async fn list_blob_keys(
        &self,
        namespace: &str,
        table: Option<&str>,
    ) -> Result<Vec<String>, ApplicationError> {
        let table = self.resolve_table(table);
        Ok(self.repository.list_blob_keys(namespace, table).await?)
    }
}
