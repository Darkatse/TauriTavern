use async_trait::async_trait;
use std::path::PathBuf;

use tt_domain::errors::DomainError;
use tt_domain::models::asset::{AssetCatalog, AssetCategory};

#[async_trait]
pub trait AssetRepository: Send + Sync {
    async fn list_assets(&self) -> Result<AssetCatalog, DomainError>;

    async fn stage_asset_file(&self, filename: &str) -> Result<PathBuf, DomainError>;

    async fn commit_staged_asset_file(
        &self,
        category: AssetCategory,
        filename: &str,
    ) -> Result<(), DomainError>;

    async fn discard_staged_asset_file(&self, filename: &str) -> Result<(), DomainError>;

    async fn delete_asset_file(
        &self,
        category: AssetCategory,
        filename: &str,
    ) -> Result<(), DomainError>;

    async fn list_character_assets(
        &self,
        character_name: &str,
        category: AssetCategory,
    ) -> Result<Vec<String>, DomainError>;
}
