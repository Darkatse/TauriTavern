use serde::{Deserialize, Serialize};
use ttsync_contract::dataset::DatasetSelection;
use ttsync_contract::status::StatusResponse;
use ttsync_core::dataset::{ResolvedDatasetPolicy, tauri_tavern_default_selection};

use crate::domain::errors::DomainError;
use crate::infrastructure::sync_bundle::{FEATURE_BUNDLE_V1, FEATURE_ZSTD_V1};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncV2OperationOptions {
    #[serde(default = "tauri_tavern_default_selection")]
    pub selection: DatasetSelection,
    #[serde(default)]
    pub require_bundle_zstd: bool,
}

impl Default for SyncV2OperationOptions {
    fn default() -> Self {
        Self {
            selection: tauri_tavern_default_selection(),
            require_bundle_zstd: false,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SyncV2BundleTransport {
    pub prefer_bundle: bool,
    pub use_zstd: bool,
}

impl SyncV2OperationOptions {
    pub fn validate(self) -> Result<Self, DomainError> {
        ResolvedDatasetPolicy::from_selection(&self.selection)
            .map_err(|error| DomainError::InvalidData(error.to_string()))?;
        Ok(self)
    }
}

pub fn resolve_sync_v2_options(
    options: Option<SyncV2OperationOptions>,
) -> Result<SyncV2OperationOptions, DomainError> {
    options.unwrap_or_default().validate()
}

pub fn bundle_transport_for_status(
    status: &StatusResponse,
    peer_label: &str,
    require_bundle_zstd: bool,
) -> Result<SyncV2BundleTransport, DomainError> {
    let has_bundle = status.features.iter().any(|f| f == FEATURE_BUNDLE_V1);
    let has_zstd = status.features.iter().any(|f| f == FEATURE_ZSTD_V1);

    if require_bundle_zstd && !has_bundle {
        return Err(DomainError::InvalidData(format!(
            "{peer_label} does not support bundle_v1"
        )));
    }
    if require_bundle_zstd && !has_zstd {
        return Err(DomainError::InvalidData(format!(
            "{peer_label} does not support zstd_v1"
        )));
    }

    Ok(SyncV2BundleTransport {
        prefer_bundle: has_bundle,
        use_zstd: has_bundle && has_zstd,
    })
}
