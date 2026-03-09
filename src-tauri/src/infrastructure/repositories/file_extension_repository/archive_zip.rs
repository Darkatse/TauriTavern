use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use tokio::fs as tokio_fs;
use uuid::Uuid;

use crate::domain::errors::DomainError;
use crate::domain::models::extension::ExtensionManifestMetadata;
use crate::domain::repositories::extension_repository::ExtensionRepository;
use crate::infrastructure::logging::logger;
use crate::infrastructure::zipkit;

use super::FileExtensionRepository;

impl FileExtensionRepository {
    pub(super) async fn create_temp_directory(
        &self,
        parent: &Path,
        prefix: &str,
    ) -> Result<PathBuf, DomainError> {
        for _ in 0..8 {
            let candidate = parent.join(format!(".{}-{}", prefix, Uuid::new_v4()));
            if !candidate.exists() {
                tokio_fs::create_dir_all(&candidate)
                    .await
                    .map_err(|error| {
                        DomainError::InternalError(format!(
                            "Failed to create temporary directory '{}': {}",
                            candidate.display(),
                            error
                        ))
                    })?;
                return Ok(candidate);
            }
        }

        Err(DomainError::InternalError(
            "Failed to allocate temporary directory for extension operation".to_string(),
        ))
    }

    pub(super) async fn cleanup_temp_directory(path: &Path) {
        if path.exists() {
            let _ = tokio_fs::remove_dir_all(path).await;
        }
    }

    fn strip_archive_root(path: &Path) -> Option<PathBuf> {
        let mut components = path.components();
        components.next()?;
        let remainder = components.as_path();

        if remainder.as_os_str().is_empty() {
            None
        } else {
            Some(remainder.to_path_buf())
        }
    }

    pub(super) fn extract_zip_bytes(
        &self,
        bytes: &[u8],
        destination: &Path,
    ) -> Result<(), DomainError> {
        let reader = Cursor::new(bytes);
        let mut archive = zip::ZipArchive::new(reader).map_err(|error| {
            DomainError::InternalError(format!("Failed to read downloaded ZIP archive: {}", error))
        })?;

        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).map_err(|error| {
                DomainError::InternalError(format!("Failed to read ZIP entry: {}", error))
            })?;

            let enclosed_path = zipkit::enclosed_zip_entry_path(&entry)?;

            // Provider archives wrap files in a top-level root folder.
            let relative_path = match Self::strip_archive_root(&enclosed_path) {
                Some(path) => path,
                None => continue,
            };

            let output_path = destination.join(relative_path);

            if entry.is_dir() {
                fs::create_dir_all(&output_path).map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to create directory '{}': {}",
                        output_path.display(),
                        error
                    ))
                })?;
                continue;
            }

            if let Some(parent) = output_path.parent() {
                fs::create_dir_all(parent).map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to create directory '{}': {}",
                        parent.display(),
                        error
                    ))
                })?;
            }

            let mut output_file = fs::File::create(&output_path).map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to create file '{}': {}",
                    output_path.display(),
                    error
                ))
            })?;

            std::io::copy(&mut entry, &mut output_file).map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to write file '{}': {}",
                    output_path.display(),
                    error
                ))
            })?;
        }

        Ok(())
    }

    pub(super) async fn required_manifest_metadata(
        &self,
        extension_path: &Path,
    ) -> Result<ExtensionManifestMetadata, DomainError> {
        match <FileExtensionRepository as ExtensionRepository>::get_manifest_metadata(
            self,
            extension_path,
        )
        .await?
        {
            Some(manifest) => Ok(manifest),
            None => Err(DomainError::InvalidData(
                "Extension manifest not found".to_string(),
            )),
        }
    }

    pub(super) fn short_commit_hash(commit_hash: &str) -> String {
        commit_hash.chars().take(7).collect()
    }

    pub(super) fn replace_directory(
        &self,
        source: &Path,
        destination: &Path,
    ) -> Result<(), DomainError> {
        let destination_name = destination
            .file_name()
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_else(|| "extension".to_string());
        let backup_path =
            destination.with_file_name(format!(".backup-{}-{}", destination_name, Uuid::new_v4()));

        fs::rename(destination, &backup_path).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to move existing extension '{}' to temporary backup '{}': {}",
                destination.display(),
                backup_path.display(),
                error
            ))
        })?;

        if let Err(error) = fs::rename(source, destination) {
            let _ = fs::rename(&backup_path, destination);
            return Err(DomainError::InternalError(format!(
                "Failed to activate updated extension '{}': {}",
                destination.display(),
                error
            )));
        }

        if let Err(error) = fs::remove_dir_all(&backup_path) {
            logger::warn(&format!(
                "Failed to remove extension backup directory '{}': {}",
                backup_path.display(),
                error
            ));
        }

        Ok(())
    }
}

pub(super) fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let path = entry.path();
        let target = dst.join(entry.file_name());

        if ty.is_dir() {
            copy_dir_all(&path, &target)?;
        } else {
            fs::copy(&path, &target)?;
        }
    }
    Ok(())
}
