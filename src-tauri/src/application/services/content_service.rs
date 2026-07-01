use serde::Serialize;
use url::Url;

use crate::application::errors::ApplicationError;
use crate::application::services::external_import_service::ExternalImportDownloader;
use crate::domain::errors::DomainError;
use crate::domain::repositories::content_repository::ContentRepository;
use std::sync::Arc;

/// Content Service
pub struct ContentService {
    content_repository: Arc<dyn ContentRepository>,
    external_import_downloader: Arc<dyn ExternalImportDownloader>,
}

impl ContentService {
    /// Create a new ContentService
    pub fn new(
        content_repository: Arc<dyn ContentRepository>,
        external_import_downloader: Arc<dyn ExternalImportDownloader>,
    ) -> Self {
        Self {
            content_repository,
            external_import_downloader,
        }
    }

    /// Initialize default content
    pub async fn initialize_default_content(&self, user_handle: &str) -> Result<(), DomainError> {
        tracing::debug!("Synchronizing default content");

        self.content_repository
            .copy_default_content_to_user(user_handle)
            .await?;

        tracing::debug!("Default content synchronized successfully");
        Ok(())
    }

    /// Check if default content is initialized
    pub async fn is_default_content_initialized(
        &self,
        user_handle: &str,
    ) -> Result<bool, DomainError> {
        tracing::debug!("Checking if default content is initialized");

        // Check if content is initialized
        let is_initialized = self
            .content_repository
            .is_default_content_initialized(user_handle)
            .await?;

        tracing::debug!("Default content initialized: {}", is_initialized);

        Ok(is_initialized)
    }

    pub async fn download_external_import_url(
        &self,
        url: &str,
    ) -> Result<ExternalImportDownloadResult, ApplicationError> {
        let parsed_url = parse_external_import_url(url)?;
        let downloaded = self
            .external_import_downloader
            .fetch_bytes(parsed_url.clone(), None)
            .await?;
        let content_type = downloaded
            .content_type
            .unwrap_or_default()
            .to_ascii_lowercase();
        let file_name = derive_file_name(&parsed_url, downloaded.content_disposition.as_deref());
        let is_png_content = content_type.starts_with("image/png");
        let is_png_file_name = file_name.to_ascii_lowercase().ends_with(".png");

        if !is_png_content && !is_png_file_name {
            return Err(ApplicationError::ValidationError(
                "Only PNG imports are supported".to_string(),
            ));
        }

        Ok(ExternalImportDownloadResult {
            data: downloaded.bytes,
            file_name: if is_png_file_name {
                file_name
            } else {
                format!("{file_name}.png")
            },
            mime_type: "image/png".to_string(),
        })
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalImportDownloadResult {
    pub data: Vec<u8>,
    pub file_name: String,
    pub mime_type: String,
}

fn parse_external_import_url(raw: &str) -> Result<Url, ApplicationError> {
    let url = Url::parse(raw.trim())
        .map_err(|_| ApplicationError::ValidationError("Invalid import URL".to_string()))?;

    match url.scheme() {
        "http" | "https" => Ok(url),
        _ => Err(ApplicationError::ValidationError(
            "Unsupported URL protocol".to_string(),
        )),
    }
}

fn derive_file_name(url: &Url, content_disposition: Option<&str>) -> String {
    if let Some(name) = content_disposition.and_then(parse_filename_from_content_disposition) {
        return sanitize_file_name(&name);
    }

    let from_url = url
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        .unwrap_or("shared-character.png");

    sanitize_file_name(from_url)
}

fn parse_filename_from_content_disposition(value: &str) -> Option<String> {
    let utf8_prefix = "filename*=UTF-8''";
    if let Some(start) = value.find(utf8_prefix) {
        let encoded = value[start + utf8_prefix.len()..]
            .split(';')
            .next()
            .unwrap_or("")
            .trim();

        if !encoded.is_empty() {
            return Some(encoded.to_string());
        }
    }

    let marker = "filename=";
    if let Some(start) = value.find(marker) {
        let raw = value[start + marker.len()..]
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .trim_matches('"');

        if !raw.is_empty() {
            return Some(raw.to_string());
        }
    }

    None
}

fn sanitize_file_name(input: &str) -> String {
    let sanitized = input
        .chars()
        .map(|character| match character {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            control if control.is_control() => '_',
            other => other,
        })
        .collect::<String>()
        .trim()
        .trim_end_matches(['.', ' '])
        .to_string();

    if sanitized.is_empty() {
        "shared-character.png".to_string()
    } else {
        sanitized
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::services::external_import_service::DownloadedBytes;
    use crate::domain::repositories::content_repository::ContentItem;
    use async_trait::async_trait;
    use std::path::Path;

    #[tokio::test]
    async fn download_external_import_url_returns_png_payload() {
        let service = ContentService::new(
            Arc::new(TestContentRepository),
            Arc::new(TestExternalImportDownloader {
                bytes: vec![1, 2, 3],
                content_type: Some("image/png"),
                content_disposition: Some("attachment; filename=\"Alice.png\""),
            }),
        );

        let result = service
            .download_external_import_url("https://example.com/share")
            .await
            .expect("download png import");

        assert_eq!(result.data, vec![1, 2, 3]);
        assert_eq!(result.file_name, "Alice.png");
        assert_eq!(result.mime_type, "image/png");
    }

    struct TestExternalImportDownloader {
        bytes: Vec<u8>,
        content_type: Option<&'static str>,
        content_disposition: Option<&'static str>,
    }

    #[async_trait]
    impl ExternalImportDownloader for TestExternalImportDownloader {
        async fn fetch_bytes(
            &self,
            _url: Url,
            _limit: Option<
                crate::application::services::external_import_service::DownloadByteLimit,
            >,
        ) -> Result<DownloadedBytes, DomainError> {
            Ok(DownloadedBytes {
                bytes: self.bytes.clone(),
                content_type: self.content_type.map(str::to_string),
                content_disposition: self.content_disposition.map(str::to_string),
            })
        }

        async fn fetch_to_file(&self, _url: Url, _path: &Path) -> Result<(), DomainError> {
            unimplemented!("not used by these tests")
        }
    }

    struct TestContentRepository;

    #[async_trait]
    impl ContentRepository for TestContentRepository {
        async fn copy_default_content_to_user(
            &self,
            _user_handle: &str,
        ) -> Result<(), DomainError> {
            unimplemented!("not used by these tests")
        }

        async fn get_content_index(&self) -> Result<Vec<ContentItem>, DomainError> {
            unimplemented!("not used by these tests")
        }

        async fn is_default_content_initialized(
            &self,
            _user_handle: &str,
        ) -> Result<bool, DomainError> {
            unimplemented!("not used by these tests")
        }
    }
}
