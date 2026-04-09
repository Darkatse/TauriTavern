use chrono::Utc;
use std::fs::{self, File};
use std::io::{BufWriter, Read, Seek, Write};
use std::path::{Path, PathBuf};
use tauri::Manager;
use zip::write::SimpleFileOptions as FileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::domain::errors::DomainError;
use crate::infrastructure::paths::{IOS_EXPORT_STAGING_ROOT_NAME, RuntimePaths};
use crate::infrastructure::zipkit::export_file_options;

const BUNDLE_ROOT_DIR: &str = "tauritavern-dev-bundle";

const COPY_BUFFER_BYTES: usize = 1024 * 1024;
const FILE_IO_BUFFER_BYTES: usize = 1024 * 1024;

pub struct DevLogBundleInput {
    pub meta_json: String,
    pub readme_text: String,
    pub frontend_logs_jsonl: String,
    pub backend_logs_tail_text: String,
}

pub fn export_dev_log_bundle(
    app_handle: &tauri::AppHandle,
    runtime_paths: &RuntimePaths,
    input: DevLogBundleInput,
) -> Result<PathBuf, DomainError> {
    let output_dir = resolve_bundle_output_dir(app_handle)?;
    fs::create_dir_all(&output_dir).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to create export output directory {}: {}",
            output_dir.display(),
            error
        ))
    })?;

    let output_path = output_dir.join(default_bundle_file_name());
    if output_path.exists() {
        return Err(DomainError::InvalidData(format!(
            "Export target already exists: {}",
            output_path.display()
        )));
    }

    let output_file = File::create(&output_path).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to create export bundle file {}: {}",
            output_path.display(),
            error
        ))
    })?;
    let buffered_output = BufWriter::with_capacity(FILE_IO_BUFFER_BYTES, output_file);
    let mut writer = ZipWriter::new(buffered_output);

    let dir_options = FileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .unix_permissions(0o755);

    writer
        .add_directory(format!("{}/", BUNDLE_ROOT_DIR), dir_options)
        .map_err(|error| DomainError::InternalError(format!("Failed to add bundle root: {error}")))?;

    add_text_file(
        &mut writer,
        &format!("{}/meta.json", BUNDLE_ROOT_DIR),
        &input.meta_json,
    )?;
    add_text_file(
        &mut writer,
        &format!("{}/README.txt", BUNDLE_ROOT_DIR),
        &input.readme_text,
    )?;

    writer
        .add_directory(format!("{}/frontend/", BUNDLE_ROOT_DIR), dir_options)
        .map_err(|error| {
            DomainError::InternalError(format!("Failed to add frontend directory: {error}"))
        })?;
    add_text_file(
        &mut writer,
        &format!("{}/frontend/logs.jsonl", BUNDLE_ROOT_DIR),
        &input.frontend_logs_jsonl,
    )?;

    writer
        .add_directory(format!("{}/backend/", BUNDLE_ROOT_DIR), dir_options)
        .map_err(|error| {
            DomainError::InternalError(format!("Failed to add backend directory: {error}"))
        })?;
    add_text_file(
        &mut writer,
        &format!("{}/backend/tail.txt", BUNDLE_ROOT_DIR),
        &input.backend_logs_tail_text,
    )?;

    writer
        .add_directory(format!("{}/settings/", BUNDLE_ROOT_DIR), dir_options)
        .map_err(|error| {
            DomainError::InternalError(format!("Failed to add settings directory: {error}"))
        })?;

    let mut copy_buffer = vec![0u8; COPY_BUFFER_BYTES];

    let settings_dir = runtime_paths.data_root.join("default-user");
    for file_name in ["tauritavern-settings.json", "settings.json"] {
        let source_path = settings_dir.join(file_name);
        if !source_path.is_file() {
            continue;
        }

        let zip_path = format!("{}/settings/{}", BUNDLE_ROOT_DIR, file_name);
        add_file_from_disk(&mut writer, &source_path, &zip_path, &mut copy_buffer)?;
    }

    for source_path in list_log_root_files(&runtime_paths.log_root, |name| {
        name.starts_with("tauritavern.log")
    })? {
        let file_name = source_path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| {
                DomainError::InvalidData(format!(
                    "Invalid backend log file name: {}",
                    source_path.display()
                ))
            })?;

        let zip_path = format!("{}/backend/{}", BUNDLE_ROOT_DIR, file_name);
        add_file_from_disk(&mut writer, &source_path, &zip_path, &mut copy_buffer)?;
    }

    writer
        .add_directory(format!("{}/llm-api/", BUNDLE_ROOT_DIR), dir_options)
        .map_err(|error| DomainError::InternalError(format!("Failed to add llm-api directory: {error}")))?;

    for source_path in list_log_root_files(&runtime_paths.log_root, |name| {
        name.starts_with("llm-api-")
    })? {
        let file_name = source_path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| {
                DomainError::InvalidData(format!(
                    "Invalid LLM API log file name: {}",
                    source_path.display()
                ))
            })?;

        let zip_path = format!("{}/llm-api/{}", BUNDLE_ROOT_DIR, file_name);
        add_file_from_disk(&mut writer, &source_path, &zip_path, &mut copy_buffer)?;
    }

    let mut buffered_output = writer
        .finish()
        .map_err(|error| DomainError::InternalError(format!("Failed to finalize bundle: {error}")))?;
    buffered_output.flush().map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to flush export bundle {}: {}",
            output_path.display(),
            error
        ))
    })?;

    Ok(output_path)
}

fn resolve_bundle_output_dir(app_handle: &tauri::AppHandle) -> Result<PathBuf, DomainError> {
    if let Ok(download_dir) = app_handle.path().download_dir() {
        return Ok(download_dir);
    }

    let cache_dir = app_handle.path().app_cache_dir().map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to resolve app cache directory for export: {}",
            error
        ))
    })?;

    Ok(cache_dir.join(IOS_EXPORT_STAGING_ROOT_NAME))
}

fn default_bundle_file_name() -> String {
    let ts = Utc::now().format("%Y%m%d-%H%M%S").to_string();
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    format!("tauritavern-dev-bundle-{}-{}.zip", ts, &suffix[..8])
}

fn add_text_file(
    writer: &mut ZipWriter<impl Write + Seek>,
    zip_path: &str,
    content: &str,
) -> Result<(), DomainError> {
    writer
        .start_file(zip_path, export_file_options(zip_path))
        .map_err(|error| DomainError::InternalError(format!("Failed to add {zip_path}: {error}")))?;

    writer.write_all(content.as_bytes()).map_err(|error| {
        DomainError::InternalError(format!("Failed to write archive entry {zip_path}: {error}"))
    })?;

    Ok(())
}

fn add_file_from_disk(
    writer: &mut ZipWriter<impl Write + Seek>,
    source_path: &Path,
    zip_path: &str,
    copy_buffer: &mut [u8],
) -> Result<(), DomainError> {
    let file_options = export_file_options(source_path);

    writer.start_file(zip_path, file_options).map_err(|error| {
        DomainError::InternalError(format!("Failed to add {zip_path} to archive: {error}"))
    })?;

    let mut source_file = File::open(source_path).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to open source file {}: {}",
            source_path.display(),
            error
        ))
    })?;

    loop {
        let bytes_read = source_file.read(copy_buffer).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read source file {}: {}",
                source_path.display(),
                error
            ))
        })?;
        if bytes_read == 0 {
            break;
        }

        writer.write_all(&copy_buffer[..bytes_read]).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to write archive entry {zip_path}: {error}"
            ))
        })?;
    }

    Ok(())
}

fn list_log_root_files(
    log_root: &Path,
    predicate: impl Fn(&str) -> bool,
) -> Result<Vec<PathBuf>, DomainError> {
    let mut files = Vec::new();

    let entries = fs::read_dir(log_root).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to read log directory {}: {}",
            log_root.display(),
            error
        ))
    })?;

    for entry in entries {
        let entry = entry.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read log directory entry {}: {}",
                log_root.display(),
                error
            ))
        })?;

        let file_type = entry.file_type().map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read log directory entry type {}: {}",
                entry.path().display(),
                error
            ))
        })?;

        if !file_type.is_file() {
            continue;
        }

        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if !predicate(&file_name) {
            continue;
        }

        files.push(entry.path());
    }

    files.sort_by(|left, right| {
        let left_name = left
            .file_name()
            .map(|value| value.to_string_lossy())
            .unwrap_or_else(|| std::borrow::Cow::Borrowed(""));
        let right_name = right
            .file_name()
            .map(|value| value.to_string_lossy())
            .unwrap_or_else(|| std::borrow::Cow::Borrowed(""));
        left_name.cmp(&right_name)
    });
    Ok(files)
}
