use std::collections::HashSet;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use tt_adapter_storage_core::file_system::{replace_file_blocking, unique_temp_path};
use tt_contracts::client_asset_paths::validate_path_segment;
use tt_domain::errors::DomainError;
use zip::read::ZipFile;

use super::{file_stem, is_image_filename};
use crate::zipkit;

const MAX_FILES: usize = 1000;
const MAX_SINGLE_FILE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 256 * 1024 * 1024;
const MAX_COMPRESSION_RATIO: u64 = 100;

struct EntryPlan {
    index: usize,
    file_name: String,
    stem: String,
}

struct PreparedSprite {
    temp_path: PathBuf,
    target_path: PathBuf,
    file_name: String,
    stem: String,
}

struct TempFiles(Vec<PathBuf>);

impl Drop for TempFiles {
    fn drop(&mut self) {
        for path in &self.0 {
            let _ = fs::remove_file(path);
        }
    }
}

pub(super) fn import_sprite_pack(
    archive_path: &Path,
    destination: &Path,
) -> Result<usize, DomainError> {
    let file = fs::File::open(archive_path).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            DomainError::NotFound(format!("Sprite pack not found: {}", archive_path.display()))
        } else {
            DomainError::InternalError(format!(
                "Failed to open sprite pack '{}': {}",
                archive_path.display(),
                error
            ))
        }
    })?;
    let mut archive = zip::ZipArchive::new(file).map_err(|error| {
        DomainError::InvalidData(format!(
            "Failed to read sprite pack '{}': {}",
            archive_path.display(),
            error
        ))
    })?;
    let plans = validate_archive(&mut archive)?;
    if plans.is_empty() {
        return Ok(0);
    }

    ensure_destination(destination)?;
    let mut temp_files = TempFiles(Vec::with_capacity(plans.len()));
    let mut prepared = Vec::with_capacity(plans.len());
    for plan in plans {
        let mut entry = archive.by_index(plan.index).map_err(|error| {
            DomainError::InvalidData(format!("Failed to read sprite pack entry: {error}"))
        })?;
        let target_path = destination.join(&plan.file_name);
        let temp_path = unique_temp_path(&target_path);
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to stage sprite '{}': {}",
                    target_path.display(),
                    error
                ))
            })?;
        temp_files.0.push(temp_path.clone());
        let written = io::copy(&mut entry, &mut output).map_err(|error| {
            DomainError::InvalidData(format!(
                "Failed to extract sprite '{}': {}",
                plan.file_name, error
            ))
        })?;
        if written != entry.size() {
            return Err(DomainError::InvalidData(format!(
                "Sprite '{}' was truncated while extracting",
                plan.file_name
            )));
        }
        prepared.push(PreparedSprite {
            temp_path,
            target_path,
            file_name: plan.file_name,
            stem: plan.stem,
        });
    }

    let count = prepared.len();
    for sprite in prepared {
        replace_file_blocking(&sprite.temp_path, &sprite.target_path)?;
        delete_variants(destination, &sprite.stem, &sprite.file_name)?;
    }
    Ok(count)
}

fn validate_archive<R: Read + io::Seek>(
    archive: &mut zip::ZipArchive<R>,
) -> Result<Vec<EntryPlan>, DomainError> {
    if archive.len() > MAX_FILES {
        return Err(DomainError::InvalidData(format!(
            "Sprite pack must contain <= {MAX_FILES} entries"
        )));
    }

    let mut plans = Vec::new();
    let mut stems = HashSet::new();
    let mut total_bytes = 0u64;
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|error| {
            DomainError::InvalidData(format!("Failed to read sprite pack entry: {error}"))
        })?;
        if zipkit::zip_entry_is_symlink(&entry) {
            return Err(DomainError::InvalidData(format!(
                "Sprite pack entry cannot be a symlink: {}",
                entry.name()
            )));
        }

        let (entry_path, display_name) = zipkit::enclosed_zip_entry_path_with_name(&entry)?;
        if contains_parent_segment(display_name) || has_absolute_prefix(display_name) {
            return Err(DomainError::InvalidData(format!(
                "Invalid sprite pack entry path: {display_name}"
            )));
        }
        if entry.is_dir() || is_ignored_path(&entry_path) {
            continue;
        }

        let Some(file_name) = entry_path.file_name().and_then(|name| name.to_str()) else {
            return Err(DomainError::InvalidData(
                "Sprite pack contains a non-UTF-8 filename".to_string(),
            ));
        };
        if !is_image_filename(file_name) {
            continue;
        }
        validate_entry_limits(&entry, &mut total_bytes)?;
        if !validate_path_segment(file_name) {
            return Err(DomainError::InvalidData(format!(
                "Sprite pack contains an invalid image filename: {display_name}"
            )));
        }

        let stem = file_stem(file_name)
            .filter(|stem| validate_path_segment(stem))
            .ok_or_else(|| {
                DomainError::InvalidData(format!(
                    "Sprite pack contains an invalid image filename: {display_name}"
                ))
            })?;
        if !stems.insert(stem.to_lowercase()) {
            return Err(DomainError::InvalidData(format!(
                "Sprite pack contains duplicate sprite name: {stem}"
            )));
        }
        plans.push(EntryPlan {
            index,
            file_name: file_name.to_string(),
            stem: stem.to_string(),
        });
    }
    Ok(plans)
}

fn validate_entry_limits<R: Read + ?Sized>(
    entry: &ZipFile<'_, R>,
    total_bytes: &mut u64,
) -> Result<(), DomainError> {
    if entry.size() > MAX_SINGLE_FILE_BYTES {
        return Err(DomainError::InvalidData(format!(
            "Sprite pack entry '{}' exceeds {} bytes",
            entry.name(),
            MAX_SINGLE_FILE_BYTES
        )));
    }
    if entry.compressed_size() > 0 && entry.size() / entry.compressed_size() > MAX_COMPRESSION_RATIO
    {
        return Err(DomainError::InvalidData(format!(
            "Sprite pack entry '{}' has an excessive compression ratio",
            entry.name()
        )));
    }
    *total_bytes = total_bytes
        .checked_add(entry.size())
        .ok_or_else(|| DomainError::InvalidData("Sprite pack is too large".to_string()))?;
    if *total_bytes > MAX_TOTAL_BYTES {
        return Err(DomainError::InvalidData(format!(
            "Sprite pack exceeds {MAX_TOTAL_BYTES} bytes"
        )));
    }
    Ok(())
}

fn ensure_destination(destination: &Path) -> Result<(), DomainError> {
    match fs::metadata(destination) {
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) => Err(DomainError::NotFound(format!(
            "Sprite set path is not a directory: {}",
            destination.display()
        ))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => fs::create_dir_all(destination)
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to create sprite set '{}': {}",
                    destination.display(),
                    error
                ))
            }),
        Err(error) => Err(DomainError::InternalError(format!(
            "Failed to inspect sprite set '{}': {}",
            destination.display(),
            error
        ))),
    }
}

fn delete_variants(
    directory: &Path,
    sprite_name: &str,
    keep_file_name: &str,
) -> Result<(), DomainError> {
    for entry in fs::read_dir(directory).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to read sprite set '{}': {}",
            directory.display(),
            error
        ))
    })? {
        let entry = entry.map_err(|error| {
            DomainError::InternalError(format!("Failed to read sprite set entry: {error}"))
        })?;
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        if file_name == keep_file_name || file_stem(file_name) != Some(sprite_name) {
            continue;
        }
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(DomainError::InternalError(format!(
                    "Failed to inspect replaced sprite '{}': {}",
                    entry.path().display(),
                    error
                )));
            }
        };
        if metadata.is_file() {
            match fs::remove_file(entry.path()) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(DomainError::InternalError(format!(
                        "Failed to delete replaced sprite '{}': {}",
                        entry.path().display(),
                        error
                    )));
                }
            }
        }
    }
    Ok(())
}

fn contains_parent_segment(path: &str) -> bool {
    path.split(['/', '\\']).any(|segment| segment == "..")
}

fn has_absolute_prefix(path: &str) -> bool {
    path.starts_with('/') || path.starts_with('\\') || path.as_bytes().get(1) == Some(&b':')
}

fn is_ignored_path(path: &Path) -> bool {
    path.components()
        .any(|component| component.as_os_str() == "__MACOSX")
        || path
            .file_name()
            .is_some_and(|name| name == ".DS_Store" || name.to_string_lossy().starts_with("._"))
}
