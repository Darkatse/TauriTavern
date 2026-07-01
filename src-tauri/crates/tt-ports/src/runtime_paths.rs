use std::path::{Path, PathBuf};

use async_trait::async_trait;
use tt_domain::errors::DomainError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeModeInfo {
    Standard,
    Portable,
}

#[derive(Debug, Clone)]
pub struct RuntimePathsSnapshot {
    pub mode: RuntimeModeInfo,
    pub app_root: PathBuf,
    pub data_root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimePathConfigInfo {
    pub data_root: PathBuf,
    pub migration_pending: bool,
    pub migration_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimePathsInfo {
    pub mode: RuntimeModeInfo,
    pub data_root: PathBuf,
    pub configured_data_root: Option<PathBuf>,
    pub migration_pending: bool,
    pub migration_error: Option<String>,
}

#[async_trait]
pub trait RuntimePathConfigStore: Send + Sync {
    fn load_config(&self, app_root: &Path) -> Result<Option<RuntimePathConfigInfo>, DomainError>;

    async fn request_data_root_change(
        &self,
        app_root: &Path,
        current_data_root: &Path,
        raw_target: &str,
    ) -> Result<(), DomainError>;
}
