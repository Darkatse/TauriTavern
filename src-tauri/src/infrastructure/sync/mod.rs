pub mod http_client;
pub mod lan;

use ttsync_contract::status::StatusResponse;

use crate::domain::errors::DomainError;
use crate::infrastructure::sync_bundle::{FEATURE_BUNDLE_V1, FEATURE_ZSTD_V1};

#[derive(Debug, Clone, Copy)]
pub struct SyncBundleTransport {
    pub prefer_bundle: bool,
    pub use_zstd: bool,
}

pub fn bundle_transport_for_status(
    status: &StatusResponse,
    peer_label: &str,
    require_bundle_zstd: bool,
) -> Result<SyncBundleTransport, DomainError> {
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

    Ok(SyncBundleTransport {
        prefer_bundle: has_bundle,
        use_zstd: has_bundle && has_zstd,
    })
}
