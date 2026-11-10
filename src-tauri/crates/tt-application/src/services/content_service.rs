use std::path::Path;
use std::sync::Arc;

use percent_encoding::percent_decode_str;
use url::Url;

use crate::errors::ApplicationError;
use crate::services::external_import_service::{
    DownloadedFile, ExternalImportDownloader, parse_external_import_url,
};
use tt_domain::errors::DomainError;
use tt_ports::repositories::content_repository::ContentRepository;

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

    /// Downloads an external URL into `destination`, which the caller owns.
    pub async fn download_external_file(
        &self,
        url: &str,
        destination: &Path,
    ) -> Result<DownloadedFile, ApplicationError> {
        let url = parse_external_import_url(url)?;
        Ok(self
            .external_import_downloader
            .fetch_to_file(url, destination)
            .await?)
    }

    /// Downloads into `destination`, which the caller owns.
    pub async fn download_character_card(
        &self,
        url: Url,
        destination: &Path,
    ) -> Result<DownloadedCharacterCard, ApplicationError> {
        let downloaded = self
            .external_import_downloader
            .fetch_to_file(url.clone(), destination)
            .await?;
        let name = derive_file_name(&url, downloaded.content_disposition.as_deref());
        let extension = downloaded
            .content_type
            .as_deref()
            .and_then(card_extension_for_content_type)
            .or_else(|| card_extension_for_file_name(&name))
            .ok_or_else(|| {
                ApplicationError::ValidationError(format!(
                    "Only PNG and JSON character cards can be imported: {name}"
                ))
            })?;
        let name = if card_extension_for_file_name(&name) == Some(extension) {
            name
        } else {
            format!("{name}.{extension}")
        };
        Ok(DownloadedCharacterCard { extension, name })
    }
}

pub struct DownloadedCharacterCard {
    /// `png` or `json`: the card format.
    pub extension: &'static str,
    pub name: String,
}

fn card_extension_for_content_type(content_type: &str) -> Option<&'static str> {
    let essence = content_type.split(';').next()?.trim();
    if essence.eq_ignore_ascii_case("image/png") {
        Some("png")
    } else if essence.eq_ignore_ascii_case("application/json") {
        Some("json")
    } else {
        None
    }
}

fn card_extension_for_file_name(name: &str) -> Option<&'static str> {
    match Path::new(name)
        .extension()?
        .to_str()?
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => Some("png"),
        "json" => Some("json"),
        _ => None,
    }
}

fn derive_file_name(url: &Url, content_disposition: Option<&str>) -> String {
    let name = content_disposition
        .and_then(parse_filename_from_content_disposition)
        .or_else(|| {
            url.path_segments()
                .and_then(|mut segments| segments.next_back())
                .map(|segment| percent_decode_str(segment).decode_utf8_lossy().into_owned())
        })
        .unwrap_or_default();
    sanitize_file_name(&name)
}

fn parse_filename_from_content_disposition(value: &str) -> Option<String> {
    let lowered = value.to_ascii_lowercase();
    let parameter = |marker: &str| {
        let start = lowered.find(marker)? + marker.len();
        let raw = value[start..].split(';').next()?.trim();
        (!raw.is_empty()).then_some(raw)
    };
    if let Some(encoded) = parameter("filename*=utf-8''") {
        return Some(percent_decode_str(encoded).decode_utf8_lossy().into_owned());
    }
    parameter("filename=")
        .map(|raw| raw.trim_matches('"'))
        .filter(|raw| !raw.is_empty())
        .map(str::to_string)
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
        "character-card".to_string()
    } else {
        sanitized
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::external_import_service::DownloadByteLimit;
    use async_trait::async_trait;
    use tt_ports::repositories::content_repository::ContentItem;

    async fn download_card(
        url: &str,
        content_type: Option<&str>,
        content_disposition: Option<&str>,
    ) -> Result<(&'static str, String), ApplicationError> {
        let service = ContentService::new(
            Arc::new(TestContentRepository),
            Arc::new(TestExternalImportDownloader {
                content_type: content_type.map(str::to_string),
                content_disposition: content_disposition.map(str::to_string),
            }),
        );
        let card = service
            .download_character_card(Url::parse(url).unwrap(), Path::new("unused"))
            .await?;
        Ok((card.extension, card.name))
    }

    #[tokio::test]
    async fn character_card_format_comes_from_response_type_then_file_name() {
        let png = Some("image/png");
        for (url, content_type, disposition, expected) in [
            (
                "https://example.com/share",
                png,
                Some("attachment; filename=\"Alice.png\""),
                ("png", "Alice.png"),
            ),
            (
                "https://example.com/download/42",
                png,
                None,
                ("png", "42.png"),
            ),
            (
                "https://example.com/card",
                Some("application/json; charset=utf-8"),
                None,
                ("json", "card.json"),
            ),
            (
                "https://example.com/Alice%20Smith.json",
                Some("text/plain; charset=utf-8"),
                None,
                ("json", "Alice Smith.json"),
            ),
            (
                "https://example.com/x",
                Some("application/octet-stream"),
                Some("attachment; FILENAME*=UTF-8''%E8%A7%92%E8%89%B2.png"),
                ("png", "角色.png"),
            ),
        ] {
            let (extension, name) = download_card(url, content_type, disposition)
                .await
                .expect("card download");
            assert_eq!((extension, name.as_str()), expected, "{url}");
        }

        let error = download_card("https://example.com/card.webp", Some("image/webp"), None)
            .await
            .expect_err("not a card format");
        assert!(matches!(error, ApplicationError::ValidationError(_)));
    }

    struct TestExternalImportDownloader {
        content_type: Option<String>,
        content_disposition: Option<String>,
    }

    #[async_trait]
    impl ExternalImportDownloader for TestExternalImportDownloader {
        async fn fetch_bytes(
            &self,
            _url: Url,
            _limit: DownloadByteLimit,
        ) -> Result<Vec<u8>, DomainError> {
            unimplemented!("not used by these tests")
        }

        async fn fetch_to_file(
            &self,
            _url: Url,
            _path: &Path,
        ) -> Result<DownloadedFile, DomainError> {
            Ok(DownloadedFile {
                content_type: self.content_type.clone(),
                content_disposition: self.content_disposition.clone(),
            })
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
