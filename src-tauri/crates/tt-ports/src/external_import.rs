use std::path::Path;

use async_trait::async_trait;
use serde::Serialize;
use tt_domain::errors::DomainError;
use url::Url;

pub fn parse_external_import_url(raw: &str) -> Result<Url, DomainError> {
    let url = Url::parse(raw.trim())
        .map_err(|_| DomainError::InvalidData("Invalid import URL".to_string()))?;

    match url.scheme() {
        "http" | "https" => Ok(url),
        _ => Err(DomainError::InvalidData(
            "Unsupported URL protocol".to_string(),
        )),
    }
}

#[derive(Clone, Copy)]
pub struct DownloadByteLimit {
    pub label: &'static str,
    pub max_bytes: usize,
}

pub struct DownloadedFile {
    pub content_type: Option<String>,
    pub content_disposition: Option<String>,
}

/// Serialized as SillyTavern's `X-Custom-Content-Type` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentKind {
    Character,
    Lorebook,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ImportContent {
    /// `name` is only a label; the downloaded file decides the format.
    File {
        url: Url,
        kind: ContentKind,
        name: Option<String>,
    },
    Card {
        card_json: String,
        image: Url,
        name: String,
    },
}

/// Asked before a download's first request and before it follows each redirect.
#[async_trait]
pub trait DownloadSources: Send + Sync {
    async fn allow(&self, url: &Url) -> Result<(), DomainError>;
}

#[async_trait]
pub trait ExternalImportDownloader: Send + Sync {
    async fn fetch_bytes(
        &self,
        url: Url,
        limit: DownloadByteLimit,
        sources: &dyn DownloadSources,
    ) -> Result<Vec<u8>, DomainError>;

    async fn fetch_to_file(
        &self,
        url: Url,
        path: &Path,
        sources: &dyn DownloadSources,
    ) -> Result<DownloadedFile, DomainError>;
}

/// `url` may also be a site ID. A URL no site claims is a character file to download as it is.
#[async_trait]
pub trait ImportContentResolver: Send + Sync {
    async fn resolve(
        &self,
        url: &str,
        sources: &dyn DownloadSources,
    ) -> Result<ImportContent, DomainError>;
}

/// Writes a character card into an image file, which becomes a PNG character card.
#[async_trait]
pub trait CharacterCardEmbedder: Send + Sync {
    async fn embed(&self, image: &Path, card_json: &str) -> Result<(), DomainError>;
}
