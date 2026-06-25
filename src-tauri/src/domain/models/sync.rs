use serde::{Deserialize, Serialize};
use ttsync_contract::dataset::DatasetSelection;
use ttsync_core::dataset::{ResolvedDatasetPolicy, tauri_tavern_default_selection};

use crate::domain::errors::DomainError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncOperationOptions {
    #[serde(default = "tauri_tavern_default_selection")]
    pub selection: DatasetSelection,
    #[serde(default)]
    pub require_bundle_zstd: bool,
}

impl Default for SyncOperationOptions {
    fn default() -> Self {
        Self {
            selection: tauri_tavern_default_selection(),
            require_bundle_zstd: false,
        }
    }
}

impl SyncOperationOptions {
    pub fn validate(self) -> Result<Self, DomainError> {
        ResolvedDatasetPolicy::from_selection(&self.selection)
            .map_err(|error| DomainError::InvalidData(error.to_string()))?;
        Ok(self)
    }
}

pub fn resolve_sync_options(
    options: Option<SyncOperationOptions>,
) -> Result<SyncOperationOptions, DomainError> {
    options.unwrap_or_default().validate()
}
