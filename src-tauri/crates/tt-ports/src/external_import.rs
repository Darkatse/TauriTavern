use std::path::Path;

use async_trait::async_trait;
use tt_domain::errors::DomainError;
use url::Url;

#[derive(Clone, Copy)]
pub struct DownloadByteLimit {
    pub label: &'static str,
    pub max_bytes: usize,
}

pub struct DownloadedFile {
    pub content_type: Option<String>,
    pub content_disposition: Option<String>,
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
