use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use std::path::{Path, PathBuf};
use tokio::fs as tokio_fs;

use crate::domain::errors::DomainError;
use crate::domain::models::extension::ExtensionAssetPayload;

use super::FileExtensionRepository;

pub(super) async fn read_third_party_asset(
    repository: &FileExtensionRepository,
    extension_name: &str,
    relative_path: &str,
    location_hint: Option<&str>,
) -> Result<ExtensionAssetPayload, DomainError> {
    let extension_folder_name = repository.normalize_extension_name(extension_name)?;
    let normalized_relative = normalize_asset_relative_path(relative_path)?;

    for base_dir in third_party_candidate_dirs(repository, location_hint) {
        let extension_root = base_dir.join(&extension_folder_name);
        let asset_path = extension_root.join(&normalized_relative);

        if !asset_path.is_file() {
            continue;
        }

        let bytes = tokio_fs::read(&asset_path).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read extension asset '{}': {}",
                asset_path.display(),
                error
            ))
        })?;

        let mime_type = mime_guess::from_path(&asset_path)
            .first_or_octet_stream()
            .essence_str()
            .to_string();

        return Ok(ExtensionAssetPayload {
            content_base64: BASE64_STANDARD.encode(bytes),
            mime_type,
        });
    }

    Err(DomainError::NotFound(format!(
        "Third-party extension asset not found: {}/{}",
        extension_name, relative_path
    )))
}

fn third_party_candidate_dirs<'a>(
    repository: &'a FileExtensionRepository,
    location_hint: Option<&str>,
) -> [&'a Path; 2] {
    match location_hint.map(|value| value.to_ascii_lowercase()) {
        Some(ref value) if value == "global" => [
            &repository.global_extensions_dir,
            &repository.user_extensions_dir,
        ],
        _ => [
            &repository.user_extensions_dir,
            &repository.global_extensions_dir,
        ],
    }
}

fn normalize_asset_relative_path(relative_path: &str) -> Result<PathBuf, DomainError> {
    let normalized = relative_path.trim().replace('\\', "/");
    let normalized = normalized.trim_matches('/');
    if normalized.is_empty() {
        return Err(DomainError::InvalidData(
            "Extension asset path cannot be empty".to_string(),
        ));
    }

    let mut sanitized = PathBuf::new();
    for segment in normalized.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return Err(DomainError::InvalidData(format!(
                "Invalid extension asset path: {}",
                relative_path
            )));
        }
        sanitized.push(segment);
    }

    Ok(sanitized)
}
