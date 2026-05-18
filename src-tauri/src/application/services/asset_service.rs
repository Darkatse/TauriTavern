use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::domain::errors::DomainError;
use crate::domain::models::asset::{AssetCatalog, AssetCategory};
use crate::domain::repositories::asset_repository::AssetRepository;

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
}

impl AssetService {
    pub fn new(repository: Arc<dyn AssetRepository>) -> Self {
        Self { repository }
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
    use super::{validate_asset_category, validate_asset_file_name};

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
