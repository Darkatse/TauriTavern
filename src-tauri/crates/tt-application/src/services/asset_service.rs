use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;
use url::Url;

use crate::errors::ApplicationError;
use crate::services::external_import_service::ExternalImportDownloader;
use tt_domain::errors::DomainError;
use tt_domain::models::asset::{AssetCatalog, AssetCategory};
use tt_ports::repositories::asset_repository::AssetRepository;

const UNSAFE_EXTENSIONS: &[&str] = &[
    ".php",
    ".exe",
    ".com",
    ".dll",
    ".pif",
    ".application",
    ".gadget",
    ".msi",
    ".jar",
    ".cmd",
    ".bat",
    ".reg",
    ".sh",
    ".py",
    ".js",
    ".jse",
    ".jsp",
    ".pdf",
    ".html",
    ".htm",
    ".hta",
    ".vb",
    ".vbs",
    ".vbe",
    ".cpl",
    ".msc",
    ".scr",
    ".sql",
    ".iso",
    ".img",
    ".dmg",
    ".ps1",
    ".ps1xml",
    ".ps2",
    ".ps2xml",
    ".psc1",
    ".psc2",
    ".msh",
    ".msh1",
    ".msh2",
    ".mshxml",
    ".msh1xml",
    ".msh2xml",
    ".scf",
    ".lnk",
    ".inf",
    ".doc",
    ".docm",
    ".docx",
    ".dot",
    ".dotm",
    ".dotx",
    ".xls",
    ".xlsm",
    ".xlsx",
    ".xlt",
    ".xltm",
    ".xltx",
    ".xlam",
    ".ppt",
    ".pptm",
    ".pptx",
    ".pot",
    ".potm",
    ".potx",
    ".ppam",
    ".ppsx",
    ".ppsm",
    ".pps",
    ".sldx",
    ".sldm",
    ".ws",
];

const RESERVED_WINDOWS_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

pub struct AssetService {
    repository: Arc<dyn AssetRepository>,
    external_import_downloader: Arc<dyn ExternalImportDownloader>,
}

impl AssetService {
    pub fn new(
        repository: Arc<dyn AssetRepository>,
        external_import_downloader: Arc<dyn ExternalImportDownloader>,
    ) -> Self {
        Self {
            repository,
            external_import_downloader,
        }
    }

    pub async fn list_assets(&self) -> Result<AssetCatalog, DomainError> {
        self.repository.list_assets().await
    }

    pub async fn delete_asset_file(
        &self,
        category: &str,
        filename: &str,
    ) -> Result<(), DomainError> {
        let category = validate_asset_category(category)?;
        let filename = validate_asset_file_name(filename)?;
        self.repository.delete_asset_file(category, &filename).await
    }

    pub async fn stage_asset_file(
        &self,
        category: &str,
        filename: &str,
    ) -> Result<(AssetCategory, PathBuf), DomainError> {
        let category = validate_asset_category(category)?;
        let filename = validate_asset_file_name(filename)?;
        let path = self.repository.stage_asset_file(&filename).await?;
        Ok((category, path))
    }

    pub async fn commit_staged_asset_file(
        &self,
        category: AssetCategory,
        filename: &str,
    ) -> Result<(), DomainError> {
        let filename = validate_asset_file_name(filename)?;
        self.repository
            .commit_staged_asset_file(category, &filename)
            .await
    }

    pub async fn discard_staged_asset_file(&self, filename: &str) -> Result<(), DomainError> {
        let filename = validate_asset_file_name(filename)?;
        self.repository.discard_staged_asset_file(&filename).await
    }

    pub async fn list_character_assets(
        &self,
        name: &str,
        category: &str,
    ) -> Result<Vec<String>, DomainError> {
        let name = validate_character_name(name)?;
        let category = validate_asset_category(category)?;
        self.repository.list_character_assets(&name, category).await
    }

    pub fn validate_download_request(
        &self,
        category: &str,
        filename: &str,
    ) -> Result<AssetCategory, DomainError> {
        let category = validate_asset_category(category)?;
        let _ = validate_asset_file_name(filename)?;
        Ok(category)
    }

    pub async fn download_asset(
        &self,
        url: &str,
        category: &str,
        filename: &str,
    ) -> Result<AssetDownloadResult, ApplicationError> {
        let category = self.validate_download_request(category, filename)?;
        let parsed_url = parse_asset_download_url(url)?;
        let host = parsed_url
            .host_str()
            .ok_or_else(|| {
                ApplicationError::ValidationError("Asset download URL host is required".to_string())
            })?
            .to_ascii_lowercase();

        if !is_import_host_whitelisted(&host) {
            return Err(ApplicationError::NotFound(format!(
                "Asset import host is not whitelisted: {}",
                host
            )));
        }

        if category == AssetCategory::Character {
            let downloaded = self
                .external_import_downloader
                .fetch_bytes(parsed_url, None)
                .await?;
            let mime_type = mime_guess::from_path(filename)
                .first_or_octet_stream()
                .essence_str()
                .to_string();
            return Ok(AssetDownloadResult {
                data: downloaded.bytes,
                mime_type,
            });
        }

        let (category, temp_path) = self.stage_asset_file(category.as_str(), filename).await?;

        if let Err(error) = self
            .external_import_downloader
            .fetch_to_file(parsed_url, &temp_path)
            .await
        {
            if let Err(cleanup_error) = self.discard_staged_asset_file(filename).await {
                return Err(ApplicationError::InternalError(format!(
                    "{}; additionally failed to remove partial asset download: {}",
                    error, cleanup_error
                )));
            }

            return Err(error.into());
        }

        if let Err(error) = self.commit_staged_asset_file(category, filename).await {
            let error = ApplicationError::from(error);
            if let Err(cleanup_error) = self.discard_staged_asset_file(filename).await {
                return Err(ApplicationError::InternalError(format!(
                    "{}; additionally failed to remove staged asset download: {}",
                    error, cleanup_error
                )));
            }

            return Err(error);
        }

        Ok(AssetDownloadResult {
            data: Vec::new(),
            mime_type: "application/octet-stream".to_string(),
        })
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetDownloadResult {
    pub data: Vec<u8>,
    pub mime_type: String,
}

pub fn validate_asset_category(input: &str) -> Result<AssetCategory, DomainError> {
    AssetCategory::from_id(input)
        .ok_or_else(|| DomainError::InvalidData("Unsupported asset category.".to_string()))
}

pub fn validate_asset_file_name(input: &str) -> Result<String, DomainError> {
    if input.is_empty() {
        return Err(DomainError::InvalidData(
            "Illegal character in filename; only alphanumeric, '_', '-' are accepted.".to_string(),
        ));
    }

    if !input
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || ch == '.')
    {
        return Err(DomainError::InvalidData(
            "Illegal character in filename; only alphanumeric, '_', '-' are accepted.".to_string(),
        ));
    }

    if input.starts_with('.') {
        return Err(DomainError::InvalidData(
            "Filename cannot start with '.'".to_string(),
        ));
    }

    if input.ends_with('.') {
        return Err(DomainError::InvalidData(
            "Filename cannot end with '.'".to_string(),
        ));
    }

    if input.as_bytes().len() > 255 || is_reserved_windows_name(input) {
        return Err(DomainError::InvalidData(
            "Reserved or long filename.".to_string(),
        ));
    }

    let extension = Path::new(input)
        .extension()
        .map(|extension| format!(".{}", extension.to_string_lossy().to_lowercase()))
        .unwrap_or_default();

    if UNSAFE_EXTENSIONS.contains(&extension.as_str()) {
        return Err(DomainError::InvalidData(
            "Forbidden file extension.".to_string(),
        ));
    }

    Ok(input.to_string())
}

fn is_reserved_windows_name(input: &str) -> bool {
    let stem = input
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    RESERVED_WINDOWS_NAMES.contains(&stem.as_str())
}

fn parse_asset_download_url(raw: &str) -> Result<Url, ApplicationError> {
    let url = Url::parse(raw.trim()).map_err(|_| {
        ApplicationError::ValidationError("Asset download URL must be valid".to_string())
    })?;
    match url.scheme() {
        "http" | "https" => Ok(url),
        _ => Err(ApplicationError::ValidationError(
            "Unsupported asset download URL protocol".to_string(),
        )),
    }
}

fn is_import_host_whitelisted(host: &str) -> bool {
    matches!(
        host,
        "localhost"
            | "127.0.0.1"
            | "::1"
            | "cdn.discordapp.com"
            | "files.catbox.moe"
            | "raw.githubusercontent.com"
    )
}

fn validate_character_name(input: &str) -> Result<String, DomainError> {
    if input.is_empty() || input.trim() != input {
        return Err(DomainError::InvalidData(
            "Invalid character name.".to_string(),
        ));
    }

    if input.contains('/') || input.contains('\\') || input.contains('\0') {
        return Err(DomainError::InvalidData(
            "Invalid character name.".to_string(),
        ));
    }

    let mut components = Path::new(input).components();
    match (components.next(), components.next()) {
        (Some(std::path::Component::Normal(_)), None) => Ok(input.to_string()),
        _ => Err(DomainError::InvalidData(
            "Invalid character name.".to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::{is_import_host_whitelisted, validate_asset_category, validate_asset_file_name};

    #[test]
    fn import_host_whitelist_matches_default_content_sources() {
        assert!(is_import_host_whitelisted("localhost"));
        assert!(is_import_host_whitelisted("raw.githubusercontent.com"));
        assert!(is_import_host_whitelisted("cdn.discordapp.com"));
        assert!(is_import_host_whitelisted("files.catbox.moe"));
        assert!(!is_import_host_whitelisted("example.com"));
    }

    #[test]
    fn validates_upstream_asset_categories() {
        for category in [
            "bgm",
            "ambient",
            "blip",
            "live2d",
            "vrm",
            "character",
            "temp",
        ] {
            validate_asset_category(category).unwrap();
        }
    }

    #[test]
    fn rejects_unsupported_asset_category() {
        assert!(validate_asset_category("extension").is_err());
    }

    #[test]
    fn validates_asset_file_names_like_upstream() {
        assert_eq!(
            validate_asset_file_name("theme-song_01.mp3").unwrap(),
            "theme-song_01.mp3"
        );
        assert!(validate_asset_file_name("bad/name.mp3").is_err());
        assert!(validate_asset_file_name(".hidden.mp3").is_err());
        assert!(validate_asset_file_name("trailing-dot.").is_err());
        assert!(validate_asset_file_name("payload.js").is_err());
        assert!(validate_asset_file_name("CON.mp3").is_err());
        assert!(validate_asset_file_name(&"a".repeat(256)).is_err());
    }
}
