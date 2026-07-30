use std::path::Path;

use async_trait::async_trait;
use tt_domain::errors::DomainError;
use url::Url;

#[derive(Clone, Copy)]
pub struct DownloadByteLimit {
    pub label: &'static str,
    pub max_bytes: usize,
}

pub struct DownloadedBytes {
    pub bytes: Vec<u8>,
    pub content_type: Option<String>,
    pub content_disposition: Option<String>,
}

#[async_trait]
pub trait ExternalImportDownloader: Send + Sync {
    async fn fetch_bytes(
        &self,
        url: Url,
        limit: Option<DownloadByteLimit>,
    ) -> Result<DownloadedBytes, DomainError>;

    async fn fetch_to_file(&self, url: Url, path: &Path) -> Result<(), DomainError>;
}
