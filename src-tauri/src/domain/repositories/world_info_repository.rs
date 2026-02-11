use async_trait::async_trait;
use serde_json::Value;
use std::path::Path;

use crate::domain::errors::DomainError;

#[async_trait]
pub trait WorldInfoRepository: Send + Sync {
    async fn get_world_info(
        &self,
        name: &str,
        allow_dummy: bool,
    ) -> Result<Option<Value>, DomainError>;
    async fn save_world_info(&self, name: &str, data: &Value) -> Result<(), DomainError>;
    async fn delete_world_info(&self, name: &str) -> Result<(), DomainError>;
    async fn import_world_info(
        &self,
        file_path: &Path,
        original_filename: &str,
        converted_data: Option<&str>,
    ) -> Result<String, DomainError>;
    async fn list_world_names(&self) -> Result<Vec<String>, DomainError>;
}
