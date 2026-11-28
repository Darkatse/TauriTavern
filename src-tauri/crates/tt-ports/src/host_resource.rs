use std::fmt;
use std::path::Path;
use std::time::SystemTime;

use tt_contracts::client_asset_paths::UserDataAssetKind;
use tt_contracts::range::ByteRange;
use tt_domain::errors::DomainError;

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
    fn into_reader(
        self: Box<Self>,
        range: Option<ByteRange>,
    ) -> Result<Box<dyn HostResourceBodyReader>, HostResourceStoreError>;
}

/// Reject source changes before returning the final bytes.
pub trait HostResourceBodyReader: Send {
    /// Returns 0 for EOF or an empty buffer.
    fn read(&mut self, buffer: &mut [u8]) -> Result<usize, HostResourceStoreError>;
}

pub struct HostResourceStream {
    length: u64,
    reader: Box<dyn HostResourceBodyReader>,
}

impl HostResourceStream {
    pub fn length(&self) -> u64 {
        self.length
    }

    pub fn read(&mut self, buffer: &mut [u8]) -> Result<usize, HostResourceStoreError> {
        self.reader.read(buffer)
    }
}

impl fmt::Debug for HostResourceStream {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HostResourceStream")
            .field("length", &self.length)
            .finish_non_exhaustive()
    }
}

pub struct OpenedHostResource {
    pub metadata: HostResourceSourceMetadata,
    body: Box<dyn HostResourceBody>,
}

impl OpenedHostResource {
    pub fn new(metadata: HostResourceSourceMetadata, body: Box<dyn HostResourceBody>) -> Self {
        Self { metadata, body }
    }

    pub fn stream(
        self,
        range: Option<ByteRange>,
    ) -> Result<HostResourceStream, HostResourceStoreError> {
        let length = range
            .map(|range| range.byte_len())
            .unwrap_or(self.metadata.content_length);
        Ok(HostResourceStream {
            length,
            reader: self.body.into_reader(range)?,
        })
    }

    pub fn read(self, range: Option<ByteRange>) -> Result<Vec<u8>, HostResourceStoreError> {
        let mut stream = self.stream(range)?;
        let length = usize::try_from(stream.length)
            .map_err(|_| HostResourceStoreError::internal("Host resource is too large to read"))?;
        let mut bytes = vec![0; length];
        let mut filled = 0;
        while filled < length {
            match stream.read(&mut bytes[filled..])? {
                0 => {
                    return Err(HostResourceStoreError::internal(format!(
                        "Host resource ended after {filled} of {length} bytes"
                    )));
                }
                count => filled += count,
            }
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

#[async_trait::async_trait]
pub trait UserCssStore: Send + Sync {
    /// Puts `factory` in place, keeping a copy of a different stylesheet it replaces.
    async fn reset_user_css(&self, factory: &[u8]) -> Result<(), DomainError>;
}
