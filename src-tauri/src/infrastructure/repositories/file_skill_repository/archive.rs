use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::Path;

use zip::ZipWriter;
use zip::read::ZipFile;

use super::package::collect_skill_files;
use super::paths::normalize_skill_path;
use super::{MAX_FILES, MAX_SINGLE_FILE_BYTES, MAX_TOTAL_BYTES, MAX_ZIP_COMPRESSION_RATIO};
use crate::domain::errors::DomainError;
use crate::infrastructure::zipkit;

pub(super) fn extract_archive(archive_path: &Path, destination: &Path) -> Result<(), DomainError> {
    let file = fs::File::open(archive_path).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to open Skill archive '{}': {}",
            archive_path.display(),
            error
        ))
    })?;
    let mut archive = zip::ZipArchive::new(file).map_err(|error| {
        DomainError::InvalidData(format!(
            "Failed to read Skill archive '{}': {}",
            archive_path.display(),
            error
        ))
    })?;
    if archive.len() > MAX_FILES {
        return Err(DomainError::InvalidData(format!(
            "Skill archive must contain <= {MAX_FILES} entries"
        )));
    }

    let mut total_bytes = 0u64;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| {
            DomainError::InvalidData(format!("Failed to read Skill archive entry: {error}"))
        })?;
        if zip_entry_is_symlink(&entry) {
            return Err(DomainError::InvalidData(format!(
                "Skill archive entry cannot be a symlink: {}",
                entry.name()
            )));
        }
        if entry.size() > MAX_SINGLE_FILE_BYTES {
            return Err(DomainError::InvalidData(format!(
                "Skill archive entry '{}' exceeds {} bytes",
                entry.name(),
                MAX_SINGLE_FILE_BYTES
            )));
        }
        if entry.compressed_size() > 0
            && entry.size() / entry.compressed_size() > MAX_ZIP_COMPRESSION_RATIO
        {
            return Err(DomainError::InvalidData(format!(
                "Skill archive entry '{}' has an excessive compression ratio",
                entry.name()
            )));
        }
        total_bytes = total_bytes
            .checked_add(entry.size())
            .ok_or_else(|| DomainError::InvalidData("Skill archive is too large".to_string()))?;
        if total_bytes > MAX_TOTAL_BYTES {
            return Err(DomainError::InvalidData(format!(
                "Skill archive exceeds {} bytes",
                MAX_TOTAL_BYTES
            )));
        }

        let (entry_path, display_name) = zipkit::enclosed_zip_entry_path_with_name(&entry)?;
        let raw_path = entry_path.to_string_lossy().to_string();
        if is_ignored_archive_path(&raw_path) {
            continue;
        }
        let path = normalize_skill_path(&raw_path)?;
        if entry.is_dir() {
            fs::create_dir_all(destination.join(path)).map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to create Skill archive directory '{}': {}",
                    display_name, error
                ))
            })?;
            continue;
        }

        let target = destination.join(path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to create Skill archive parent '{}': {}",
                    parent.display(),
                    error
                ))
            })?;
        }
        let mut output = fs::File::create(&target).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to create Skill archive file '{}': {}",
                target.display(),
                error
            ))
        })?;
        std::io::copy(&mut entry, &mut output).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to extract Skill archive file '{}': {}",
                target.display(),
                error
            ))
        })?;
    }
    Ok(())
}

pub(super) fn export_skill_dir(root: &Path) -> Result<Vec<u8>, DomainError> {
    let files = collect_skill_files(root)?;
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for file in &files {
        let path = root.join(&file.path);
        writer
            .start_file(file.path.as_str(), zipkit::export_file_options(&path))
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to add Skill export file '{}': {}",
                    file.path, error
                ))
            })?;
        let bytes = fs::read(&path).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read Skill export file '{}': {}",
                path.display(),
                error
            ))
        })?;
        writer.write_all(&bytes).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to write Skill export file '{}': {}",
                file.path, error
            ))
        })?;
    }

    let cursor = writer.finish().map_err(|error| {
        DomainError::InternalError(format!("Failed to finish Skill export archive: {error}"))
    })?;
    Ok(cursor.into_inner())
}

fn zip_entry_is_symlink<R: Read + ?Sized>(entry: &ZipFile<'_, R>) -> bool {
    entry
        .unix_mode()
        .is_some_and(|mode| mode & 0o170000 == 0o120000)
}

fn is_ignored_archive_path(path: &str) -> bool {
    path.split('/').any(|segment| segment == "__MACOSX")
        || path.ends_with(".DS_Store")
        || path
            .split('/')
            .next_back()
            .is_some_and(|name| name.starts_with("._"))
}
