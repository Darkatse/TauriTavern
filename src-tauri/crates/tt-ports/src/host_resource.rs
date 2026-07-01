use std::path::Path;

use tt_contracts::client_asset_paths::UserDataAssetKind;
use tt_contracts::range::ByteRange;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostResourceBinaryAsset {
    pub bytes: Vec<u8>,
    pub mime_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostResourceFileStat {
    pub len: u64,
    pub mime_type: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThumbnailKind {
    Avatar,
    Persona,
    Background,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThumbnailAssetRequest {
    pub kind: ThumbnailKind,
    pub file: String,
    pub use_thumbnails: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostResourceStoreError {
    NotFound(String),
    Forbidden(String),
    PayloadTooLarge { size_bytes: u64, limit_bytes: u64 },
    Internal(String),
}

impl HostResourceStoreError {
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound(message.into())
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::Forbidden(message.into())
    }

    pub const fn payload_too_large(size_bytes: u64, limit_bytes: u64) -> Self {
        Self::PayloadTooLarge {
            size_bytes,
            limit_bytes,
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal(message.into())
    }
}

pub trait HostResourceAssetStore: Send + Sync {
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
