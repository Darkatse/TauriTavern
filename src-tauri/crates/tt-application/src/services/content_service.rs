use std::path::Path;
use std::sync::Arc;

use percent_encoding::percent_decode_str;
use url::Url;

use crate::errors::ApplicationError;
use crate::services::external_import_service::{
    AnySource, CharacterCardEmbedder, ContentKind, DownloadSources, DownloadedFile,
    ExternalImportDownloader, ImportContent, ImportContentResolver, parse_external_import_url,
};
use tt_domain::errors::DomainError;
use tt_ports::repositories::content_repository::ContentRepository;

/// Content Service
pub struct ContentService {
    content_repository: Arc<dyn ContentRepository>,
    external_import_downloader: Arc<dyn ExternalImportDownloader>,
    content_resolver: Arc<dyn ImportContentResolver>,
    card_embedder: Arc<dyn CharacterCardEmbedder>,
}

impl ContentService {
    /// Create a new ContentService
    pub fn new(
        content_repository: Arc<dyn ContentRepository>,
        external_import_downloader: Arc<dyn ExternalImportDownloader>,
        content_resolver: Arc<dyn ImportContentResolver>,
        card_embedder: Arc<dyn CharacterCardEmbedder>,
    ) -> Self {
        Self {
            content_repository,
            external_import_downloader,
            content_resolver,
            card_embedder,
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

    /// Downloads into `destination`, which the caller owns, from any source: saving a file is not
    /// an import.
    pub async fn download_external_file(
        &self,
        url: &str,
        destination: &Path,
    ) -> Result<DownloadedFile, ApplicationError> {
        let url = parse_external_import_url(url)?;
        Ok(self
            .external_import_downloader
            .fetch_to_file(url, destination, &AnySource)
            .await?)
    }

    /// Downloads what `url`, a URL or a site ID, names into `destination`, which the caller owns.
    pub async fn download_content(
        &self,
        url: &str,
        destination: &Path,
        sources: &dyn DownloadSources,
    ) -> Result<DownloadedContent, ApplicationError> {
        let downloader = self.external_import_downloader.as_ref();
        match self.content_resolver.resolve(url, sources).await? {
            ImportContent::File { url, kind, name } => {
                let downloaded = downloader
                    .fetch_to_file(url.clone(), destination, sources)
                    .await?;
                let served = derive_file_name(&url, downloaded.content_disposition.as_deref());
                let extension = downloaded
                    .content_type
                    .as_deref()
                    .and_then(extension_for_content_type)
                    .or_else(|| extension_for_file_name(&served))
                    .ok_or_else(|| {
                        ApplicationError::ValidationError(format!(
                            "Only PNG and JSON files can be imported: {served}"
                        ))
                    })?;
                Ok(DownloadedContent {
                    kind,
                    extension,
                    name: file_name(name.as_deref().unwrap_or(&served), extension),
                })
            }
            ImportContent::Card {
                card_json,
                image,
                name,
            } => {
                downloader
                    .fetch_to_file(image, destination, sources)
                    .await?;
                self.card_embedder.embed(destination, &card_json).await?;
                Ok(DownloadedContent {
                    kind: ContentKind::Character,
                    extension: "png",
                    name: file_name(&name, "png"),
                })
            }
        }
    }
}

pub struct DownloadedContent {
    pub kind: ContentKind,
    /// `png` or `json`: the file format.
    pub extension: &'static str,
    pub name: String,
}

fn extension_for_content_type(content_type: &str) -> Option<&'static str> {
    let essence = content_type.split(';').next()?.trim();
    if essence.eq_ignore_ascii_case("image/png") {
        Some("png")
    } else if essence.eq_ignore_ascii_case("application/json") {
        Some("json")
    } else {
        None
    }
}

fn extension_for_file_name(name: &str) -> Option<&'static str> {
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

fn file_name(label: &str, extension: &str) -> String {
    let name = sanitize_file_name(label);
    if extension_for_file_name(&name) == Some(extension) {
        name
    } else {
        format!("{name}.{extension}")
    }
}

fn derive_file_name(url: &Url, content_disposition: Option<&str>) -> String {
    content_disposition
        .and_then(parse_filename_from_content_disposition)
        .or_else(|| {
            url.path_segments()
                .and_then(|mut segments| segments.next_back())
                .map(|segment| percent_decode_str(segment).decode_utf8_lossy().into_owned())
        })
        .unwrap_or_default()
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
        "content".to_string()
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

    async fn download(
        url: &str,
        label: Option<&str>,
        content_type: Option<&str>,
        content_disposition: Option<&str>,
    ) -> Result<(&'static str, String), ApplicationError> {
        let service = ContentService::new(
            Arc::new(TestContentRepository),
            Arc::new(TestExternalImportDownloader {
                content_type: content_type.map(str::to_string),
                content_disposition: content_disposition.map(str::to_string),
            }),
            Arc::new(TestContentResolver {
                label: label.map(str::to_string),
            }),
            Arc::new(TestCardEmbedder),
        );
        let content = service
            .download_content(url, Path::new("unused"), &AnySource)
            .await?;
        Ok((content.extension, content.name))
    }

    #[tokio::test]
    async fn file_format_comes_from_response_type_then_file_name() {
        let png = Some("image/png");
        for (url, label, content_type, disposition, expected) in [
            (
                "https://example.com/share",
                None,
                png,
                Some("attachment; filename=\"Alice.png\""),
                ("png", "Alice.png"),
            ),
            (
                "https://example.com/download/42",
                None,
                png,
                None,
                ("png", "42.png"),
            ),
            (
                "https://example.com/card",
                None,
                Some("application/json; charset=utf-8"),
                None,
                ("json", "card.json"),
            ),
            (
                "https://example.com/Alice%20Smith.json",
                None,
                Some("text/plain; charset=utf-8"),
                None,
                ("json", "Alice Smith.json"),
            ),
            (
                "https://example.com/x",
                None,
                Some("application/octet-stream"),
                Some("attachment; FILENAME*=UTF-8''%E8%A7%92%E8%89%B2.png"),
                ("png", "角色.png"),
            ),
            (
                "https://example.com/files/card.png",
                Some("Alice"),
                Some("application/octet-stream"),
                None,
                ("png", "Alice.png"),
            ),
        ] {
            let (extension, name) = download(url, label, content_type, disposition)
                .await
                .expect("download");
            assert_eq!((extension, name.as_str()), expected, "{url}");
        }

        let error = download(
            "https://example.com/card.webp",
            None,
            Some("image/webp"),
            None,
        )
        .await
        .expect_err("not an import format");
        assert!(matches!(error, ApplicationError::ValidationError(_)));
    }

    /// Resolves every URL as a character file with the site's label.
    struct TestContentResolver {
        label: Option<String>,
    }

    #[async_trait]
    impl ImportContentResolver for TestContentResolver {
        async fn resolve(
            &self,
            url: &str,
            _sources: &dyn DownloadSources,
        ) -> Result<ImportContent, DomainError> {
            Ok(ImportContent::File {
                url: parse_external_import_url(url)?,
                kind: ContentKind::Character,
                name: self.label.clone(),
            })
        }
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
            _sources: &dyn DownloadSources,
        ) -> Result<Vec<u8>, DomainError> {
            unimplemented!("not used by these tests")
        }

        async fn fetch_to_file(
            &self,
            _url: Url,
            _path: &Path,
            _sources: &dyn DownloadSources,
        ) -> Result<DownloadedFile, DomainError> {
            Ok(DownloadedFile {
                content_type: self.content_type.clone(),
                content_disposition: self.content_disposition.clone(),
            })
        }
    }

    struct TestCardEmbedder;

    #[async_trait]
    impl CharacterCardEmbedder for TestCardEmbedder {
        async fn embed(&self, _image: &Path, _card_json: &str) -> Result<(), DomainError> {
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
