use serde::Serialize;
use url::Url;

use crate::errors::ApplicationError;
use crate::services::external_import_service::{DownloadByteLimit, ExternalImportDownloader};
use std::sync::Arc;
use tt_domain::errors::DomainError;
use tt_ports::repositories::content_repository::ContentRepository;

/// Upper bound for a single remote image payload fetched on behalf of a user action.
///
/// An initial judgement call rather than a measurement: it clears real chat images with room to
/// spare, while a hostile URL still cannot make the device buffer without bound.
const REMOTE_MEDIA_BYTE_LIMIT: DownloadByteLimit = DownloadByteLimit {
    label: "Remote image",
    max_bytes: 32 * 1024 * 1024,
};

const SHARED_CHARACTER_FILE_NAME: &str = "shared-character.png";

/// Content Service
pub struct ContentService {
    content_repository: Arc<dyn ContentRepository>,
    external_import_downloader: Arc<dyn ExternalImportDownloader>,
    remote_media_downloader: Arc<dyn ExternalImportDownloader>,
}

impl ContentService {
    /// Create a new ContentService
    pub fn new(
        content_repository: Arc<dyn ContentRepository>,
        external_import_downloader: Arc<dyn ExternalImportDownloader>,
        remote_media_downloader: Arc<dyn ExternalImportDownloader>,
    ) -> Self {
        Self {
            content_repository,
            external_import_downloader,
            remote_media_downloader,
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
        let file_name = derive_file_name(
            &parsed_url,
            downloaded.content_disposition.as_deref(),
            SHARED_CHARACTER_FILE_NAME,
        );
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

    /// Downloads a remote image the user explicitly selected in a message.
    ///
    /// A page can render a cross-origin image but cannot read its bytes: without CORS the browser
    /// keeps the payload opaque. The host fetches instead, because native HTTP is not bound by that
    /// policy. `url` is therefore page-supplied and untrusted, so the host owns the policy: the
    /// request runs on the bounded `RemoteMedia` profile and the checks below reject anything that
    /// is not a plain `http(s)` image within the size limit.
    pub async fn download_remote_image(
        &self,
        url: &str,
    ) -> Result<RemoteImageDownloadResult, ApplicationError> {
        let parsed_url = parse_remote_image_url(url)?;
        let downloaded = self
            .remote_media_downloader
            .fetch_bytes(parsed_url.clone(), Some(REMOTE_MEDIA_BYTE_LIMIT))
            .await?;

        let mime_type = normalize_image_mime_type(downloaded.content_type.as_deref())?;
        let extension = image_extension(&mime_type);
        let file_name = derive_file_name(
            &parsed_url,
            downloaded.content_disposition.as_deref(),
            &format!("image.{extension}"),
        );
        let file_name = if has_file_extension(&file_name) {
            file_name
        } else {
            format!("{file_name}.{extension}")
        };

        Ok(RemoteImageDownloadResult {
            data: downloaded.bytes,
            file_name,
            mime_type,
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteImageDownloadResult {
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

fn parse_remote_image_url(raw: &str) -> Result<Url, ApplicationError> {
    let url = Url::parse(raw.trim())
        .map_err(|_| ApplicationError::ValidationError("Invalid remote image URL".to_string()))?;

    if !matches!(url.scheme(), "http" | "https") {
        return Err(ApplicationError::ValidationError(
            "Remote image URL must use http or https".to_string(),
        ));
    }
    if url.host_str().is_none() {
        return Err(ApplicationError::ValidationError(
            "Remote image URL must include a host".to_string(),
        ));
    }
    // The native client would forward embedded credentials; the page never carries them.
    if !url.username().is_empty() || url.password().is_some() {
        return Err(ApplicationError::ValidationError(
            "Remote image URL must not include credentials".to_string(),
        ));
    }

    Ok(url)
}

fn normalize_image_mime_type(content_type: Option<&str>) -> Result<String, ApplicationError> {
    let mime_type = content_type
        .unwrap_or_default()
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();

    if !mime_type.starts_with("image/") {
        return Err(ApplicationError::ValidationError(format!(
            "Remote URL did not return an image (content type: {})",
            if mime_type.is_empty() {
                "unknown"
            } else {
                mime_type.as_str()
            }
        )));
    }

    Ok(mime_type)
}

fn image_extension(mime_type: &str) -> &'static str {
    match mime_type {
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "image/avif" => "avif",
        "image/bmp" => "bmp",
        "image/svg+xml" => "svg",
        "image/x-icon" => "ico",
        _ => "png",
    }
}

fn has_file_extension(file_name: &str) -> bool {
    file_name
        .rsplit_once('.')
        .is_some_and(|(stem, extension)| !stem.is_empty() && !extension.is_empty())
}

fn derive_file_name(url: &Url, content_disposition: Option<&str>, fallback: &str) -> String {
    if let Some(name) = content_disposition.and_then(parse_filename_from_content_disposition) {
        return sanitize_file_name(&name, fallback);
    }

    let from_url = url
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        .unwrap_or(fallback);

    sanitize_file_name(from_url, fallback)
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

fn sanitize_file_name(input: &str, fallback: &str) -> String {
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
        fallback.to_string()
    } else {
        sanitized
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::external_import_service::DownloadedBytes;
    use async_trait::async_trait;
    use std::path::Path;
    use tt_ports::repositories::content_repository::ContentItem;

    /// The slot a test does not exercise still has to be filled.
    fn unused_downloader() -> TestExternalImportDownloader {
        TestExternalImportDownloader {
            bytes: Vec::new(),
            content_type: None,
            content_disposition: None,
        }
    }

    #[tokio::test]
    async fn download_external_import_url_returns_png_payload() {
        let service = ContentService::new(
            Arc::new(TestContentRepository),
            Arc::new(TestExternalImportDownloader {
                bytes: vec![1, 2, 3],
                content_type: Some("image/png"),
                content_disposition: Some("attachment; filename=\"Alice.png\""),
            }),
            Arc::new(unused_downloader()),
        );

        let result = service
            .download_external_import_url("https://example.com/share")
            .await
            .expect("download png import");

        assert_eq!(result.data, vec![1, 2, 3]);
        assert_eq!(result.file_name, "Alice.png");
        assert_eq!(result.mime_type, "image/png");
    }

    #[tokio::test]
    async fn download_remote_image_accepts_non_png_images_and_completes_the_file_name() {
        let service = ContentService::new(
            Arc::new(TestContentRepository),
            Arc::new(unused_downloader()),
            Arc::new(TestExternalImportDownloader {
                bytes: vec![9, 8, 7],
                content_type: Some("image/webp; charset=binary"),
                content_disposition: None,
            }),
        );

        let result = service
            .download_remote_image("https://cdn.example.com/media/original")
            .await
            .expect("download remote image");

        assert_eq!(result.data, vec![9, 8, 7]);
        assert_eq!(result.mime_type, "image/webp");
        assert_eq!(result.file_name, "original.webp");
    }

    #[tokio::test]
    async fn download_remote_image_rejects_payloads_that_are_not_images() {
        let service = ContentService::new(
            Arc::new(TestContentRepository),
            Arc::new(unused_downloader()),
            Arc::new(TestExternalImportDownloader {
                bytes: b"<html></html>".to_vec(),
                content_type: Some("text/html"),
                content_disposition: None,
            }),
        );

        let result = service
            .download_remote_image("https://example.com/tracker")
            .await;

        assert!(matches!(
            result,
            Err(ApplicationError::ValidationError(_))
        ));
    }

    #[tokio::test]
    async fn download_remote_image_rejects_unusable_urls() {
        let service = ContentService::new(
            Arc::new(TestContentRepository),
            Arc::new(unused_downloader()),
            Arc::new(TestExternalImportDownloader {
                bytes: vec![1],
                content_type: Some("image/png"),
                content_disposition: None,
            }),
        );

        for url in [
            "file:///etc/passwd",
            "data:image/png;base64,AAAA",
            "https://user:secret@example.com/a.png",
            "not a url",
        ] {
            let result = service.download_remote_image(url).await;
            assert!(
                matches!(result, Err(ApplicationError::ValidationError(_))),
                "{url}"
            );
        }
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
            _limit: Option<crate::services::external_import_service::DownloadByteLimit>,
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
