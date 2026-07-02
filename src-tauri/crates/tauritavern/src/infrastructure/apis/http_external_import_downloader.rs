use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use futures_util::TryStreamExt;
use reqwest::header::{CONTENT_DISPOSITION, CONTENT_TYPE, HeaderMap, HeaderName};
use tokio::io::AsyncWriteExt;
use url::Url;

use tt_adapter_http::{HttpClientPool, HttpClientProfile};
use tt_domain::errors::DomainError;
use tt_ports::external_import::{DownloadByteLimit, DownloadedBytes, ExternalImportDownloader};

pub struct HttpExternalImportDownloader {
    http_clients: Arc<HttpClientPool>,
}

impl HttpExternalImportDownloader {
    pub fn new(http_clients: Arc<HttpClientPool>) -> Self {
        Self { http_clients }
    }
}

#[async_trait]
impl ExternalImportDownloader for HttpExternalImportDownloader {
    async fn fetch_bytes(
        &self,
        url: Url,
        limit: Option<DownloadByteLimit>,
    ) -> Result<DownloadedBytes, DomainError> {
        let client = self.http_clients.client(HttpClientProfile::Download)?;
        let response = client.get(url).send().await.map_err(internal_error)?;

        if !response.status().is_success() {
            return Err(DomainError::InternalError(format!(
                "Upstream responded with HTTP {}",
                response.status()
            )));
        }

        if let Some(limit) = limit {
            if response
                .content_length()
                .is_some_and(|length| length > limit.max_bytes as u64)
            {
                return Err(limit_error(limit));
            }
        }

        let headers = response.headers().clone();
        let bytes = match limit {
            Some(limit) => read_limited_response(response, limit).await?,
            None => response.bytes().await.map_err(internal_error)?.to_vec(),
        };

        Ok(DownloadedBytes {
            bytes,
            content_type: header_string(&headers, CONTENT_TYPE),
            content_disposition: header_string(&headers, CONTENT_DISPOSITION),
        })
    }

    async fn fetch_to_file(&self, url: Url, path: &Path) -> Result<(), DomainError> {
        let client = self.http_clients.client(HttpClientProfile::Download)?;
        let response = client.get(url).send().await.map_err(internal_error)?;

        if !response.status().is_success() {
            return Err(DomainError::InternalError(format!(
                "Upstream responded with HTTP {}",
                response.status()
            )));
        }

        let mut file = tokio::fs::File::create(path)
            .await
            .map_err(internal_error)?;
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.try_next().await.map_err(internal_error)? {
            file.write_all(&chunk).await.map_err(internal_error)?;
        }
        file.flush().await.map_err(internal_error)
    }
}

async fn read_limited_response(
    response: reqwest::Response,
    limit: DownloadByteLimit,
) -> Result<Vec<u8>, DomainError> {
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.try_next().await.map_err(internal_error)? {
        let next_len = bytes
            .len()
            .checked_add(chunk.len())
            .ok_or_else(|| limit_error(limit))?;
        if next_len > limit.max_bytes {
            return Err(limit_error(limit));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn header_string(headers: &HeaderMap, name: HeaderName) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned)
}

fn limit_error(limit: DownloadByteLimit) -> DomainError {
    DomainError::InvalidData(format!(
        "{} must be <= {} bytes",
        limit.label, limit.max_bytes
    ))
}

fn internal_error(error: impl std::fmt::Display) -> DomainError {
    DomainError::InternalError(error.to_string())
}
