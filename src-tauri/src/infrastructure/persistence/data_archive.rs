use base64::engine::general_purpose::STANDARD as BASE64_ENGINE;
use base64::Engine as _;
use chrono::Utc;
use serde::Serialize;
use std::fs::{self, File};
use std::io::Cursor;
use std::path::{Component, Path, PathBuf};
use tauri::AppHandle;
use tokio::fs as tokio_fs;
use uuid::Uuid;
use zip::write::FileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::domain::errors::DomainError;
use crate::infrastructure::paths::resolve_app_data_dir;
use crate::infrastructure::persistence::file_system::DataDirectory;

const DEFAULT_USER_HANDLE: &str = "default-user";
const USER_DIR_MARKERS: &[&str] = &["characters", "chats", "user", "worlds", "groups"];

#[derive(Debug, Clone, Serialize)]
pub struct DataArchiveImportResult {
    pub source_users: Vec<String>,
    pub target_user: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DataArchiveExportResult {
    pub file_name: String,
    pub zip_base64: String,
}

#[derive(Debug, Clone)]
struct SourceUser {
    name: String,
    path: PathBuf,
}

pub fn export_data_archive(app_handle: &AppHandle) -> Result<DataArchiveExportResult, DomainError> {
    let data_root = resolve_data_root(app_handle)?;
    if !data_root.is_dir() {
        return Err(DomainError::NotFound(format!(
            "Data directory not found: {}",
            data_root.display()
        )));
    }

    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let file_options = FileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o644);
    let dir_options = FileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o755);

    writer
        .add_directory("data/", dir_options)
        .map_err(|error| internal_error("Failed to create zip root directory", error))?;

    append_directory_to_zip(
        &mut writer,
        &data_root,
        &data_root,
        "data",
        file_options,
        dir_options,
    )?;

    let bytes = writer
        .finish()
        .map_err(|error| internal_error("Failed to finalize data archive", error))?
        .into_inner();
    let encoded = BASE64_ENGINE.encode(bytes);
    let file_name = format!(
        "tauritavern-data-{}.zip",
        Utc::now().format("%Y%m%d-%H%M%S")
    );

    Ok(DataArchiveExportResult {
        file_name,
        zip_base64: encoded,
    })
}

pub async fn import_data_archive(
    app_handle: &AppHandle,
    archive_path: &Path,
) -> Result<DataArchiveImportResult, DomainError> {
    if !archive_path.is_file() {
        return Err(DomainError::InvalidData(format!(
            "Archive file does not exist: {}",
            archive_path.display()
        )));
    }

    let app_data_dir = resolve_app_data_dir(app_handle)
        .map_err(|error| internal_error("Failed to resolve app data directory", error))?;
    let data_root = app_data_dir.join("data");
    let work_root = app_data_dir.join(format!(".data-import-{}", Uuid::new_v4().simple()));
    let extract_root = work_root.join("extract");
    let normalized_root = work_root.join("normalized");

    tokio_fs::create_dir_all(&extract_root)
        .await
        .map_err(|error| internal_error("Failed to create migration workspace", error))?;

    let import_result = async {
        extract_zip_archive(archive_path, &extract_root)?;

        let source_data_root = resolve_source_data_root(&extract_root)?;
        let source_users = discover_source_users(&source_data_root)?;
        if source_users.is_empty() {
            return Err(DomainError::InvalidData(
                "No user data directory found in archive".to_string(),
            ));
        }

        tokio_fs::create_dir_all(&normalized_root)
            .await
            .map_err(|error| internal_error("Failed to create normalized data directory", error))?;

        build_normalized_data_root(&source_data_root, &normalized_root, &source_users)?;
        replace_data_root(&data_root, &normalized_root).await?;
        DataDirectory::new(data_root.clone()).initialize().await?;

        Ok(DataArchiveImportResult {
            source_users: source_users.into_iter().map(|user| user.name).collect(),
            target_user: DEFAULT_USER_HANDLE.to_string(),
        })
    }
    .await;

    cleanup_directory(&work_root).await;
    import_result
}

fn resolve_data_root(app_handle: &AppHandle) -> Result<PathBuf, DomainError> {
    let app_data_dir = resolve_app_data_dir(app_handle)
        .map_err(|error| internal_error("Failed to resolve app data directory", error))?;
    Ok(app_data_dir.join("data"))
}

fn append_directory_to_zip(
    writer: &mut ZipWriter<Cursor<Vec<u8>>>,
    root: &Path,
    current: &Path,
    prefix: &str,
    file_options: FileOptions,
    dir_options: FileOptions,
) -> Result<(), DomainError> {
    for entry in read_directory_sorted(current)? {
        let path = entry.path();
        let relative_path = path
            .strip_prefix(root)
            .map_err(|error| internal_error("Failed to resolve archive path", error))?;
        let zip_relative = normalize_zip_path(relative_path);
        let zip_path = format!("{}/{}", prefix, zip_relative);
        let file_type = entry
            .file_type()
            .map_err(|error| internal_error("Failed to read file type", error))?;

        if file_type.is_dir() {
            writer
                .add_directory(format!("{}/", zip_path), dir_options)
                .map_err(|error| internal_error("Failed to add directory to archive", error))?;
            append_directory_to_zip(writer, root, &path, prefix, file_options, dir_options)?;
            continue;
        }

        if !file_type.is_file() {
            continue;
        }

        writer
            .start_file(&zip_path, file_options)
            .map_err(|error| internal_error("Failed to add file to archive", error))?;
        let mut source_file = File::open(&path)
            .map_err(|error| internal_error("Failed to open source file for archive", error))?;
        std::io::copy(&mut source_file, writer)
            .map_err(|error| internal_error("Failed to write file into archive", error))?;
    }

    Ok(())
}

fn extract_zip_archive(archive_path: &Path, output_root: &Path) -> Result<(), DomainError> {
    let archive_file = File::open(archive_path)
        .map_err(|error| internal_error("Failed to open archive file", error))?;
    let mut archive = ZipArchive::new(archive_file)
        .map_err(|error| internal_error("Failed to read archive file", error))?;

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| internal_error("Failed to read archive entry", error))?;
        let relative_path = sanitize_zip_entry_path(entry.name())?;
        if relative_path.as_os_str().is_empty() {
            continue;
        }

        let output_path = output_root.join(&relative_path);
        if entry.is_dir() || entry.name().ends_with('/') {
            fs::create_dir_all(&output_path)
                .map_err(|error| internal_error("Failed to create extracted directory", error))?;
            continue;
        }

        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                internal_error("Failed to create extracted parent directory", error)
            })?;
        }

        let mut output_file = File::create(&output_path)
            .map_err(|error| internal_error("Failed to create extracted file", error))?;
        std::io::copy(&mut entry, &mut output_file)
            .map_err(|error| internal_error("Failed to write extracted file", error))?;
    }

    Ok(())
}

fn sanitize_zip_entry_path(raw_path: &str) -> Result<PathBuf, DomainError> {
    let mut sanitized = PathBuf::new();
    for component in Path::new(raw_path).components() {
        match component {
            Component::Normal(segment) => sanitized.push(segment),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(DomainError::InvalidData(format!(
                    "Invalid archive entry path: {}",
                    raw_path
                )))
            }
        }
    }

    Ok(sanitized)
}

fn resolve_source_data_root(extract_root: &Path) -> Result<PathBuf, DomainError> {
    let direct_data_root = extract_root.join("data");
    if direct_data_root.is_dir() {
        return Ok(direct_data_root);
    }

    if looks_like_data_root(extract_root) {
        return Ok(extract_root.to_path_buf());
    }

    let child_dirs = read_directory_sorted(extract_root)?
        .into_iter()
        .filter_map(|entry| {
            entry
                .file_type()
                .ok()
                .filter(|file_type| file_type.is_dir())
                .map(|_| entry.path())
        })
        .collect::<Vec<_>>();

    if child_dirs.len() == 1 {
        let child = child_dirs[0].clone();
        let nested_data_root = child.join("data");
        if nested_data_root.is_dir() {
            return Ok(nested_data_root);
        }

        if looks_like_data_root(&child) {
            return Ok(child);
        }
    }

    Err(DomainError::InvalidData(
        "Archive does not contain a valid data directory".to_string(),
    ))
}

fn discover_source_users(source_data_root: &Path) -> Result<Vec<SourceUser>, DomainError> {
    let mut users = Vec::new();

    for entry in read_directory_sorted(source_data_root)? {
        let file_type = entry
            .file_type()
            .map_err(|error| internal_error("Failed to detect user directory type", error))?;
        if !file_type.is_dir() {
            continue;
        }

        let user_path = entry.path();
        if is_likely_user_directory(&user_path) {
            users.push(SourceUser {
                name: entry.file_name().to_string_lossy().to_string(),
                path: user_path,
            });
        }
    }

    if users.is_empty() && is_likely_user_directory(source_data_root) {
        let source_name = source_data_root
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("archive-root")
            .to_string();
        users.push(SourceUser {
            name: source_name,
            path: source_data_root.to_path_buf(),
        });
    }

    users.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(users)
}

fn build_normalized_data_root(
    source_data_root: &Path,
    normalized_root: &Path,
    source_users: &[SourceUser],
) -> Result<(), DomainError> {
    let root_is_single_user = source_users.len() == 1 && source_users[0].path == source_data_root;
    let target_default_user = normalized_root.join(DEFAULT_USER_HANDLE);

    if root_is_single_user {
        copy_directory_contents(source_data_root, &target_default_user)?;
        return Ok(());
    }

    for entry in read_directory_sorted(source_data_root)? {
        let source_path = entry.path();
        let name = entry.file_name();
        let entry_name = name.to_string_lossy();
        let is_user_dir = source_users.iter().any(|user| user.name == entry_name);
        if is_user_dir {
            continue;
        }

        copy_path(&source_path, &normalized_root.join(name))?;
    }

    for user in source_users {
        copy_directory_contents(&user.path, &target_default_user)?;
    }

    Ok(())
}

fn copy_path(source: &Path, target: &Path) -> Result<(), DomainError> {
    let metadata = fs::metadata(source)
        .map_err(|error| internal_error("Failed to read source metadata", error))?;

    if metadata.is_dir() {
        copy_directory_contents(source, target)?;
        return Ok(());
    }

    if !metadata.is_file() {
        return Ok(());
    }

    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| internal_error("Failed to create target parent directory", error))?;
    }

    fs::copy(source, target).map_err(|error| internal_error("Failed to copy file", error))?;
    Ok(())
}

fn copy_directory_contents(source: &Path, target: &Path) -> Result<(), DomainError> {
    fs::create_dir_all(target)
        .map_err(|error| internal_error("Failed to create target directory", error))?;

    for entry in read_directory_sorted(source)? {
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        copy_path(&source_path, &target_path)?;
    }

    Ok(())
}

async fn replace_data_root(data_root: &Path, normalized_root: &Path) -> Result<(), DomainError> {
    let parent = data_root.parent().ok_or_else(|| {
        DomainError::InternalError("Data root does not have a valid parent directory".to_string())
    })?;
    let backup_root = parent.join(format!(".data-backup-{}", Uuid::new_v4().simple()));
    let had_existing_data = data_root.exists();

    if had_existing_data {
        fs::rename(data_root, &backup_root)
            .map_err(|error| internal_error("Failed to backup existing data directory", error))?;
    }

    if let Err(error) = fs::rename(normalized_root, data_root) {
        if had_existing_data {
            let _ = fs::rename(&backup_root, data_root);
        }
        return Err(internal_error("Failed to replace data directory", error));
    }

    if had_existing_data {
        cleanup_directory(&backup_root).await;
    }

    Ok(())
}

fn looks_like_data_root(path: &Path) -> bool {
    if !path.is_dir() {
        return false;
    }

    if path.join("extensions").is_dir() {
        return true;
    }

    if is_likely_user_directory(path) {
        return true;
    }

    fs::read_dir(path)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .any(|entry_path| entry_path.is_dir() && is_likely_user_directory(&entry_path))
}

fn is_likely_user_directory(path: &Path) -> bool {
    let has_characters = path.join("characters").is_dir();
    let has_chats = path.join("chats").is_dir();
    if has_characters && has_chats {
        return true;
    }

    USER_DIR_MARKERS
        .iter()
        .filter(|marker| path.join(marker).is_dir())
        .count()
        >= 2
}

fn read_directory_sorted(path: &Path) -> Result<Vec<fs::DirEntry>, DomainError> {
    let mut entries = fs::read_dir(path)
        .map_err(|error| internal_error("Failed to read directory", error))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| internal_error("Failed to read directory entry", error))?;

    entries.sort_by(|left, right| {
        left.file_name()
            .to_string_lossy()
            .cmp(&right.file_name().to_string_lossy())
    });
    Ok(entries)
}

fn normalize_zip_path(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(segment) => Some(segment.to_string_lossy().to_string()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

async fn cleanup_directory(path: &Path) {
    if let Err(error) = tokio_fs::remove_dir_all(path).await {
        if error.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(
                "Failed to clean up migration directory {}: {}",
                path.display(),
                error
            );
        }
    }
}

fn internal_error(context: &str, error: impl std::fmt::Display) -> DomainError {
    DomainError::InternalError(format!("{}: {}", context, error))
}
