use std::path::Path;
use std::time::SystemTime;

use tt_contracts::client_asset_paths::UserDataAssetKind;
use tt_contracts::range::ByteRange;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostResourceSourceRevision(Vec<u8>);

impl HostResourceSourceRevision {
    pub fn new(value: impl Into<Vec<u8>>) -> Self {
        Self(value.into())
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostResourceSourceMetadata {
    pub content_type: String,
    pub content_length: u64,
    pub last_modified: SystemTime,
    pub revision: HostResourceSourceRevision,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThumbnailKind {
    Avatar,
    Persona,
    Background,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThumbnailSelection {
    Original,
    PreferGenerated,
    RequireGenerated,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThumbnailAssetRequest {
    pub kind: ThumbnailKind,
    pub file: String,
    pub selection: ThumbnailSelection,
}

#[derive(Debug, Clone, Copy)]
pub enum HostResourceSourceRequest<'a> {
    UserCss,
    ThirdParty {
        extension_folder: &'a str,
        relative_path: &'a Path,
    },
    UserData {
        kind: UserDataAssetKind,
        relative_path: &'a Path,
    },
    Thumbnail(&'a ThumbnailAssetRequest),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostResourceStoreError {
    NotFound(String),
    Forbidden(String),
    Internal(String),
}

impl HostResourceStoreError {
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound(message.into())
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::Forbidden(message.into())
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal(message.into())
    }
}

pub trait HostResourceBody: Send {
    fn read(self: Box<Self>, range: Option<ByteRange>) -> Result<Vec<u8>, HostResourceStoreError>;
}

pub struct OpenedHostResource {
    pub metadata: HostResourceSourceMetadata,
    body: Box<dyn HostResourceBody>,
}

impl OpenedHostResource {
    pub fn new(metadata: HostResourceSourceMetadata, body: Box<dyn HostResourceBody>) -> Self {
        Self { metadata, body }
    }

    pub fn read(self, range: Option<ByteRange>) -> Result<Vec<u8>, HostResourceStoreError> {
        let expected_len = range
            .map(|range| range.byte_len())
            .unwrap_or(self.metadata.content_length);
        let bytes = self.body.read(range)?;
        if bytes.len() as u64 != expected_len {
            return Err(HostResourceStoreError::internal(format!(
                "Host resource changed while being read: expected {expected_len} bytes, read {}",
                bytes.len()
            )));
        }
        Ok(bytes)
    }
}

pub trait HostResourceAssetStore: Send + Sync {
    fn open(
        &self,
        request: HostResourceSourceRequest<'_>,
    ) -> Result<OpenedHostResource, HostResourceStoreError>;
}
