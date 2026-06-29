use super::range::ByteRange;
use crate::application::client_asset_paths::UserDataAssetKind;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HostResourceBinaryAsset {
    pub(crate) bytes: Vec<u8>,
    pub(crate) mime_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HostResourceFileStat {
    pub(crate) len: u64,
    pub(crate) mime_type: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ThumbnailKind {
    Avatar,
    Persona,
    Background,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ThumbnailAssetRequest {
    pub(crate) kind: ThumbnailKind,
    pub(crate) file: String,
    pub(crate) use_thumbnails: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HostResourceStoreError {
    NotFound(String),
    Forbidden(String),
    PayloadTooLarge { size_bytes: u64, limit_bytes: u64 },
    Internal(String),
}

impl HostResourceStoreError {
    pub(crate) fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound(message.into())
    }

    pub(crate) fn forbidden(message: impl Into<String>) -> Self {
        Self::Forbidden(message.into())
    }

    pub(crate) const fn payload_too_large(size_bytes: u64, limit_bytes: u64) -> Self {
        Self::PayloadTooLarge {
            size_bytes,
            limit_bytes,
        }
    }

    pub(crate) fn internal(message: impl Into<String>) -> Self {
        Self::Internal(message.into())
    }
}

pub(crate) trait HostResourceAssetStore: Send + Sync {
    fn read_user_css(&self) -> Result<Vec<u8>, HostResourceStoreError>;

    fn stat_third_party_asset(
        &self,
        extension_folder: &str,
        relative_path: &Path,
    ) -> Result<HostResourceFileStat, HostResourceStoreError>;

    fn read_third_party_asset(
        &self,
        extension_folder: &str,
        relative_path: &Path,
        max_len: Option<u64>,
    ) -> Result<HostResourceBinaryAsset, HostResourceStoreError>;

    fn stat_user_data_asset(
        &self,
        kind: UserDataAssetKind,
        relative_path: &Path,
    ) -> Result<HostResourceFileStat, HostResourceStoreError>;

    fn read_user_data_asset(
        &self,
        kind: UserDataAssetKind,
        relative_path: &Path,
    ) -> Result<Vec<u8>, HostResourceStoreError>;

    fn read_user_data_asset_range(
        &self,
        kind: UserDataAssetKind,
        relative_path: &Path,
        range: ByteRange,
    ) -> Result<Vec<u8>, HostResourceStoreError>;

    fn read_thumbnail_asset(
        &self,
        request: ThumbnailAssetRequest,
    ) -> Result<HostResourceBinaryAsset, HostResourceStoreError>;
}
