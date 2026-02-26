use chrono::Utc;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Seek, Write};
use std::path::{Component, Path, PathBuf};
use uuid::Uuid;
use zip::write::FileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::domain::errors::DomainError;
use crate::infrastructure::persistence::file_system::DataDirectory;

const DEFAULT_USER_HANDLE: &str = "default-user";
const USER_DIR_MARKERS: &[&str] = &["characters", "chats", "user", "worlds", "groups"];

const MAX_ARCHIVE_ENTRIES: usize = 500_000;
const MAX_TOTAL_UNCOMPRESSED_BYTES: u64 = 64 * 1024 * 1024 * 1024;
const MAX_ENTRY_UNCOMPRESSED_BYTES: u64 = 16 * 1024 * 1024 * 1024;
const MAX_COMPRESSION_RATIO: u64 = 500;
const COMPRESSION_RATIO_MIN_BYTES: u64 = 1024 * 1024;
const COPY_BUFFER_BYTES: usize = 4 * 1024 * 1024;
const FILE_IO_BUFFER_BYTES: usize = 4 * 1024 * 1024;
const PROGRESS_REPORT_MIN_DELTA: f32 = 0.5;

pub const CANCELLED_ERROR_MARKER: &str = "__data_archive_job_cancelled__";

#[derive(Debug, Clone)]
pub struct DataArchiveImportResult {
    pub source_users: Vec<String>,
    pub target_user: String,
}

#[derive(Debug, Clone)]
pub struct DataArchiveExportResult {
    pub file_name: String,
    pub archive_path: PathBuf,
}

#[derive(Debug, Clone)]
struct SourceUsersMeta {
    users: Vec<String>,
    root_is_single_user: bool,
}

#[derive(Debug, Clone)]
struct ArchiveLayoutMeta {
    source_prefix: PathBuf,
    scanned_entries: usize,
    source_users_meta: SourceUsersMeta,
}

#[derive(Debug, Clone)]
struct ExportProgress {
    processed_steps: u64,
    total_steps: u64,
    last_reported_percent: f32,
}

pub fn is_cancelled_error(error: &DomainError) -> bool {
    match error {
        DomainError::InternalError(message) => message == CANCELLED_ERROR_MARKER,
        _ => false,
    }
}

pub fn run_export_data_archive(
    data_root: &Path,
    output_path: &Path,
    report_progress: &mut dyn FnMut(&str, f32, &str),
    is_cancelled: &dyn Fn() -> bool,
) -> Result<DataArchiveExportResult, DomainError> {
    report_progress("preparing", 0.0, "Preparing export");
    ensure_not_cancelled(is_cancelled)?;

    if !data_root.is_dir() {
        return Err(DomainError::NotFound(format!(
            "Data directory not found: {}",
            data_root.display()
        )));
    }

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| internal_error("Failed to create export output directory", error))?;
    }

    let total_steps = count_export_entries(data_root, is_cancelled)?.saturating_add(1);
    let mut progress = ExportProgress {
        processed_steps: 1,
        total_steps,
        last_reported_percent: 0.0,
    };

    let file_options = FileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o644);
    let dir_options = FileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o755);

    let output_file = File::create(output_path)
        .map_err(|error| internal_error("Failed to create export archive file", error))?;
    let buffered_output = BufWriter::with_capacity(FILE_IO_BUFFER_BYTES, output_file);
    let mut writer = ZipWriter::new(buffered_output);

    writer
        .add_directory("data/", dir_options)
        .map_err(|error| internal_error("Failed to add archive root directory", error))?;
    report_export_progress(&mut progress, report_progress);

    let mut copy_buffer = vec![0u8; COPY_BUFFER_BYTES];
    write_export_entries(
        &mut writer,
        data_root,
        data_root,
        "data",
        file_options,
        dir_options,
        &mut progress,
        &mut copy_buffer,
        report_progress,
        is_cancelled,
    )?;

    let mut buffered_output = writer
        .finish()
        .map_err(|error| internal_error("Failed to finalize export archive", error))?;
    buffered_output
        .flush()
        .map_err(|error| internal_error("Failed to flush export archive", error))?;

    ensure_not_cancelled(is_cancelled)?;
    report_progress("finalizing", 100.0, "Export completed");

    Ok(DataArchiveExportResult {
        file_name: output_path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("tauritavern-data.zip")
            .to_string(),
        archive_path: output_path.to_path_buf(),
    })
}

pub fn run_import_data_archive(
    data_root: &Path,
    archive_path: &Path,
    workspace_root: &Path,
    report_progress: &mut dyn FnMut(&str, f32, &str),
    is_cancelled: &dyn Fn() -> bool,
) -> Result<DataArchiveImportResult, DomainError> {
    report_progress("preparing", 0.0, "Preparing import");
    ensure_not_cancelled(is_cancelled)?;

    if !archive_path.is_file() {
        return Err(DomainError::InvalidData(format!(
            "Archive file does not exist: {}",
            archive_path.display()
        )));
    }

    let normalized_root = workspace_root.join("normalized");

    if normalized_root.exists() {
        cleanup_directory_sync(&normalized_root);
    }

    fs::create_dir_all(&normalized_root)
        .map_err(|error| internal_error("Failed to create normalized workspace", error))?;

    let layout_meta = scan_archive_layout(archive_path)?;
    report_progress("scanning", 8.0, "Archive metadata scanned");
    ensure_not_cancelled(is_cancelled)?;
    report_progress("scanning", 15.0, "Source users detected");
    ensure_not_cancelled(is_cancelled)?;

    extract_to_normalized_root_streaming(
        archive_path,
        &layout_meta.source_prefix,
        &layout_meta.source_users_meta,
        &normalized_root,
        layout_meta.scanned_entries,
        report_progress,
        is_cancelled,
    )?;

    report_progress("swapping", 95.0, "Replacing data directory");
    replace_data_root(data_root, &normalized_root)?;

    // Ensure required default directories exist after migration.
    tauri::async_runtime::block_on(DataDirectory::new(data_root.to_path_buf()).initialize())?;

    report_progress("completed", 100.0, "Import completed");

    Ok(DataArchiveImportResult {
        source_users: layout_meta.source_users_meta.users,
        target_user: DEFAULT_USER_HANDLE.to_string(),
    })
}

fn count_export_entries(
    current: &Path,
    is_cancelled: &dyn Fn() -> bool,
) -> Result<u64, DomainError> {
    let mut count = 0u64;

    for entry in read_directory_sorted(current)? {
        ensure_not_cancelled(is_cancelled)?;

        let file_type = entry
            .file_type()
            .map_err(|error| internal_error("Failed to read export entry type", error))?;
        let path = entry.path();

        if file_type.is_dir() {
            count = count.saturating_add(1);
            count = count.saturating_add(count_export_entries(&path, is_cancelled)?);
            continue;
        }

        if file_type.is_file() {
            count = count.saturating_add(1);
        }
    }

    Ok(count)
}

#[allow(clippy::too_many_arguments)]
fn write_export_entries(
    writer: &mut ZipWriter<impl Write + Seek>,
    root: &Path,
    current: &Path,
    zip_prefix: &str,
    file_options: FileOptions,
    dir_options: FileOptions,
    progress: &mut ExportProgress,
    copy_buffer: &mut [u8],
    report_progress: &mut dyn FnMut(&str, f32, &str),
    is_cancelled: &dyn Fn() -> bool,
) -> Result<(), DomainError> {
    for entry in read_directory_sorted(current)? {
        ensure_not_cancelled(is_cancelled)?;

        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| internal_error("Failed to read export entry type", error))?;
        let relative_path = path
            .strip_prefix(root)
            .map_err(|error| internal_error("Failed to resolve export relative path", error))?;
        let zip_relative = normalize_zip_path(relative_path);
        let zip_path = format!("{}/{}", zip_prefix, zip_relative);

        if file_type.is_dir() {
            writer
                .add_directory(format!("{}/", zip_path), dir_options)
                .map_err(|error| internal_error("Failed to add directory to archive", error))?;
            progress.processed_steps = progress.processed_steps.saturating_add(1);
            report_export_progress(progress, report_progress);

            write_export_entries(
                writer,
                root,
                &path,
                zip_prefix,
                file_options,
                dir_options,
                progress,
                copy_buffer,
                report_progress,
                is_cancelled,
            )?;
            continue;
        }

        if !file_type.is_file() {
            continue;
        }

        writer
            .start_file(&zip_path, file_options)
            .map_err(|error| internal_error("Failed to add file to archive", error))?;

        let mut source_file = File::open(&path)
            .map_err(|error| internal_error("Failed to open export source file", error))?;
        copy_stream_with_cancel(
            &mut source_file,
            writer,
            copy_buffer,
            is_cancelled,
            "Failed to read export source file",
            "Failed to write file to archive",
        )?;

        progress.processed_steps = progress.processed_steps.saturating_add(1);
        report_export_progress(progress, report_progress);
    }

    Ok(())
}

fn report_export_progress(
    progress: &mut ExportProgress,
    report_progress: &mut dyn FnMut(&str, f32, &str),
) {
    let percent = progress_percent(progress.processed_steps, progress.total_steps, 3.0, 96.0);
    let should_report = progress.processed_steps >= progress.total_steps
        || percent - progress.last_reported_percent >= PROGRESS_REPORT_MIN_DELTA;
    if !should_report {
        return;
    }

    progress.last_reported_percent = percent;
    report_progress("zipping", percent, "Writing archive entries");
}

fn scan_archive_layout(archive_path: &Path) -> Result<ArchiveLayoutMeta, DomainError> {
    let archive_file = File::open(archive_path)
        .map_err(|error| internal_error("Failed to open archive file", error))?;
    let archive_reader = BufReader::with_capacity(FILE_IO_BUFFER_BYTES, archive_file);
    let mut archive = ZipArchive::new(archive_reader)
        .map_err(|error| internal_error("Failed to parse archive file", error))?;

    let mut top_level_dirs = BTreeSet::new();
    let mut root_children = BTreeSet::new();
    let mut root_user_markers: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    let mut data_children = BTreeSet::new();
    let mut data_user_markers: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    let mut top_children: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut top_user_markers: BTreeMap<String, BTreeMap<String, BTreeSet<String>>> =
        BTreeMap::new();
    let mut top_data_children: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut top_data_user_markers: BTreeMap<String, BTreeMap<String, BTreeSet<String>>> =
        BTreeMap::new();
    let mut top_has_data_subtree = BTreeSet::new();
    let mut has_root_data_prefix = false;

    let mut scanned_entries = 0usize;
    let mut total_uncompressed_bytes = 0u64;

    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|error| internal_error("Failed to read archive entry", error))?;
        let sanitized_path = sanitize_zip_entry_path(entry.name())?;
        if sanitized_path.as_os_str().is_empty() {
            continue;
        }

        validate_zip_entry_limits(
            entry.name(),
            entry.size(),
            entry.compressed_size(),
            &mut total_uncompressed_bytes,
        )?;

        scanned_entries = scanned_entries.saturating_add(1);
        if scanned_entries > MAX_ARCHIVE_ENTRIES {
            return Err(DomainError::InvalidData(format!(
                "Archive contains too many entries (>{})",
                MAX_ARCHIVE_ENTRIES
            )));
        }

        let components = path_components(&sanitized_path);
        if components.is_empty() {
            continue;
        }

        let first = components[0].clone();
        top_level_dirs.insert(first.clone());
        root_children.insert(first.clone());

        if first == "data" {
            has_root_data_prefix = true;

            if components.len() >= 2 {
                let second = components[1].clone();
                data_children.insert(second.clone());

                if components.len() >= 3 {
                    data_user_markers
                        .entry(second)
                        .or_default()
                        .insert(components[2].clone());
                }
            }
        }

        if components.len() >= 2 {
            let second = components[1].clone();
            root_user_markers
                .entry(first.clone())
                .or_default()
                .insert(second.clone());

            top_children
                .entry(first.clone())
                .or_default()
                .insert(second.clone());

            if components.len() >= 3 {
                top_user_markers
                    .entry(first.clone())
                    .or_default()
                    .entry(second.clone())
                    .or_default()
                    .insert(components[2].clone());
            }

            if second == "data" {
                top_has_data_subtree.insert(first.clone());

                if components.len() >= 3 {
                    let third = components[2].clone();
                    top_data_children
                        .entry(first.clone())
                        .or_default()
                        .insert(third.clone());

                    if components.len() >= 4 {
                        top_data_user_markers
                            .entry(first)
                            .or_default()
                            .entry(third)
                            .or_default()
                            .insert(components[3].clone());
                    }
                }
            }
        }
    }

    if scanned_entries == 0 {
        return Err(DomainError::InvalidData("Archive is empty".to_string()));
    }

    let source_prefix = detect_source_data_prefix_from_layout(
        &top_level_dirs,
        &root_children,
        &top_children,
        &top_has_data_subtree,
        has_root_data_prefix,
    )?;

    let prefix_components = path_components(&source_prefix);
    let empty_set = BTreeSet::new();
    let empty_markers = BTreeMap::new();

    let (prefix_root_markers, prefix_child_markers): (
        &BTreeSet<String>,
        &BTreeMap<String, BTreeSet<String>>,
    ) = match prefix_components.as_slice() {
        [] => (&root_children, &root_user_markers),
        [single] if single == "data" => (&data_children, &data_user_markers),
        [top] => (
            top_children.get(top).unwrap_or(&empty_set),
            top_user_markers.get(top).unwrap_or(&empty_markers),
        ),
        [top, second] if second == "data" => (
            top_data_children.get(top).unwrap_or(&empty_set),
            top_data_user_markers.get(top).unwrap_or(&empty_markers),
        ),
        _ => {
            return Err(DomainError::InvalidData(format!(
                "Unsupported source prefix layout: {}",
                source_prefix.display()
            )));
        }
    };

    let source_users_meta = discover_source_users_from_markers(
        &source_prefix,
        prefix_root_markers,
        prefix_child_markers,
    )?;

    Ok(ArchiveLayoutMeta {
        source_prefix,
        scanned_entries,
        source_users_meta,
    })
}

fn detect_source_data_prefix_from_layout(
    top_level_dirs: &BTreeSet<String>,
    root_children: &BTreeSet<String>,
    top_children: &BTreeMap<String, BTreeSet<String>>,
    top_has_data_subtree: &BTreeSet<String>,
    has_root_data_prefix: bool,
) -> Result<PathBuf, DomainError> {
    if has_root_data_prefix {
        return Ok(PathBuf::from("data"));
    }

    if top_level_dirs.len() == 1 {
        let top = top_level_dirs
            .first()
            .expect("top level directory exists for single-entry set");
        if top_has_data_subtree.contains(top) {
            return Ok(PathBuf::from(top).join("data"));
        }
    }

    if looks_like_data_root_from_children(root_children) {
        return Ok(PathBuf::new());
    }

    if top_level_dirs.len() == 1 {
        let top = top_level_dirs
            .first()
            .expect("top level directory exists for single-entry set");
        if let Some(children) = top_children.get(top) {
            if looks_like_data_root_from_children(children) {
                return Ok(PathBuf::from(top));
            }
        }
    }

    Err(DomainError::InvalidData(
        "Archive does not contain a recognizable data directory".to_string(),
    ))
}

fn looks_like_data_root_from_children(children: &BTreeSet<String>) -> bool {
    if children.contains("extensions") {
        return true;
    }

    has_user_directory_markers(children)
}

fn discover_source_users_from_markers(
    source_prefix: &Path,
    root_markers: &BTreeSet<String>,
    child_markers: &BTreeMap<String, BTreeSet<String>>,
) -> Result<SourceUsersMeta, DomainError> {
    let mut users = child_markers
        .iter()
        .filter_map(|(candidate, markers)| {
            if has_user_directory_markers(markers) {
                Some(candidate.clone())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    users.sort();

    if !users.is_empty() {
        return Ok(SourceUsersMeta {
            users,
            root_is_single_user: false,
        });
    }

    if has_user_directory_markers(root_markers) {
        let source_name = source_prefix
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("archive-root")
            .to_string();

        return Ok(SourceUsersMeta {
            users: vec![source_name],
            root_is_single_user: true,
        });
    }

    Err(DomainError::InvalidData(
        "No user data directory found in archive".to_string(),
    ))
}

#[allow(clippy::too_many_arguments)]
fn extract_to_normalized_root_streaming(
    archive_path: &Path,
    source_prefix: &Path,
    source_users_meta: &SourceUsersMeta,
    normalized_root: &Path,
    scanned_entries: usize,
    report_progress: &mut dyn FnMut(&str, f32, &str),
    is_cancelled: &dyn Fn() -> bool,
) -> Result<(), DomainError> {
    let archive_file = File::open(archive_path)
        .map_err(|error| internal_error("Failed to open archive file", error))?;
    let archive_reader = BufReader::with_capacity(FILE_IO_BUFFER_BYTES, archive_file);
    let mut archive = ZipArchive::new(archive_reader)
        .map_err(|error| internal_error("Failed to parse archive file", error))?;

    let source_users_lookup = source_users_meta
        .users
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();

    let total_entries = scanned_entries.max(1) as u64;
    let mut processed_entries = 0u64;
    let mut last_reported_percent = 0.0f32;
    let mut copy_buffer = vec![0u8; COPY_BUFFER_BYTES];
    let mut last_ensured_parent: Option<PathBuf> = None;

    for index in 0..archive.len() {
        ensure_not_cancelled(is_cancelled)?;

        let mut archive_entry = archive
            .by_index(index)
            .map_err(|error| internal_error("Failed to read archive entry", error))?;
        let sanitized_path = sanitize_zip_entry_path(archive_entry.name())?;
        if sanitized_path.as_os_str().is_empty() {
            continue;
        }

        processed_entries = processed_entries.saturating_add(1);

        let Some(rel_components) = components_after_prefix(&sanitized_path, source_prefix) else {
            maybe_report_extraction_progress(
                processed_entries,
                total_entries,
                &mut last_reported_percent,
                report_progress,
            );
            continue;
        };
        if rel_components.is_empty() {
            maybe_report_extraction_progress(
                processed_entries,
                total_entries,
                &mut last_reported_percent,
                report_progress,
            );
            continue;
        }

        let target_relative_path = map_to_normalized_path(
            &rel_components,
            &source_users_lookup,
            source_users_meta.root_is_single_user,
        );
        let output_path = normalized_root.join(target_relative_path);

        if archive_entry.is_dir() || archive_entry.name().ends_with('/') {
            ensure_output_directory(&output_path)?;
            maybe_report_extraction_progress(
                processed_entries,
                total_entries,
                &mut last_reported_percent,
                report_progress,
            );
            continue;
        }

        if let Some(parent) = output_path.parent() {
            let should_create_parent = last_ensured_parent
                .as_ref()
                .map(|last| last != parent)
                .unwrap_or(true);
            if should_create_parent {
                fs::create_dir_all(parent).map_err(|error| {
                    internal_error("Failed to create normalized parent directory", error)
                })?;
                last_ensured_parent = Some(parent.to_path_buf());
            }
        }

        let mut output_file = create_output_file_replacing_directory(&output_path)?;

        copy_stream_with_cancel(
            &mut archive_entry,
            &mut output_file,
            &mut copy_buffer,
            is_cancelled,
            "Failed to read archive entry data",
            "Failed to write normalized output file",
        )?;

        maybe_report_extraction_progress(
            processed_entries,
            total_entries,
            &mut last_reported_percent,
            report_progress,
        );
    }

    Ok(())
}

fn maybe_report_extraction_progress(
    processed_entries: u64,
    total_entries: u64,
    last_reported_percent: &mut f32,
    report_progress: &mut dyn FnMut(&str, f32, &str),
) {
    let percent = progress_percent(processed_entries, total_entries, 15.0, 90.0);
    let should_report = processed_entries >= total_entries
        || percent - *last_reported_percent >= PROGRESS_REPORT_MIN_DELTA;
    if !should_report {
        return;
    }

    *last_reported_percent = percent;
    report_progress("extracting", percent, "Extracting and normalizing archive");
}

fn ensure_output_directory(path: &Path) -> Result<(), DomainError> {
    if path.is_file() {
        fs::remove_file(path).map_err(|error| {
            internal_error(
                "Failed to replace file with directory in normalized output",
                error,
            )
        })?;
    }

    fs::create_dir_all(path)
        .map_err(|error| internal_error("Failed to create normalized directory", error))
}

fn create_output_file_replacing_directory(path: &Path) -> Result<File, DomainError> {
    match File::create(path) {
        Ok(file) => Ok(file),
        Err(error) if error.kind() == io::ErrorKind::IsADirectory => {
            fs::remove_dir_all(path).map_err(|remove_error| {
                internal_error(
                    "Failed to replace directory with file in normalized output",
                    remove_error,
                )
            })?;

            File::create(path).map_err(|create_error| {
                internal_error("Failed to create normalized output file", create_error)
            })
        }
        Err(error) => Err(internal_error(
            "Failed to create normalized output file",
            error,
        )),
    }
}

fn map_to_normalized_path(
    relative_components: &[String],
    source_users: &BTreeSet<String>,
    root_is_single_user: bool,
) -> PathBuf {
    if root_is_single_user {
        let mut target = PathBuf::from(DEFAULT_USER_HANDLE);
        for component in relative_components {
            target.push(component);
        }
        return target;
    }

    let first_component = relative_components.first().cloned().unwrap_or_default();
    if source_users.contains(&first_component) {
        let mut target = PathBuf::from(DEFAULT_USER_HANDLE);
        for component in relative_components.iter().skip(1) {
            target.push(component);
        }
        return target;
    }

    let mut target = PathBuf::new();
    for component in relative_components {
        target.push(component);
    }
    target
}

fn replace_data_root(data_root: &Path, normalized_root: &Path) -> Result<(), DomainError> {
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
        cleanup_directory_sync(&backup_root);
    }

    Ok(())
}

fn validate_zip_entry_limits(
    entry_name: &str,
    uncompressed_size: u64,
    compressed_size: u64,
    total_uncompressed_bytes: &mut u64,
) -> Result<(), DomainError> {
    if uncompressed_size > MAX_ENTRY_UNCOMPRESSED_BYTES {
        return Err(DomainError::InvalidData(format!(
            "Archive entry is too large (>{} bytes): {}",
            MAX_ENTRY_UNCOMPRESSED_BYTES, entry_name
        )));
    }

    if compressed_size > 0
        && uncompressed_size > COMPRESSION_RATIO_MIN_BYTES
        && uncompressed_size / compressed_size > MAX_COMPRESSION_RATIO
    {
        return Err(DomainError::InvalidData(format!(
            "Archive entry compression ratio is suspicious: {}",
            entry_name
        )));
    }

    *total_uncompressed_bytes = total_uncompressed_bytes.saturating_add(uncompressed_size);
    if *total_uncompressed_bytes > MAX_TOTAL_UNCOMPRESSED_BYTES {
        return Err(DomainError::InvalidData(format!(
            "Archive uncompressed size exceeds limit (>{} bytes)",
            MAX_TOTAL_UNCOMPRESSED_BYTES
        )));
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
                )));
            }
        }
    }

    Ok(sanitized)
}

fn path_components(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(segment) => Some(segment.to_string_lossy().to_string()),
            _ => None,
        })
        .collect()
}

fn components_after_prefix(path: &Path, prefix: &Path) -> Option<Vec<String>> {
    let relative_path = if prefix.as_os_str().is_empty() {
        path
    } else {
        path.strip_prefix(prefix).ok()?
    };

    let components = relative_path
        .components()
        .filter_map(|component| match component {
            Component::Normal(segment) => Some(segment.to_string_lossy().to_string()),
            _ => None,
        })
        .collect::<Vec<_>>();

    Some(components)
}

fn has_user_directory_markers(markers: &BTreeSet<String>) -> bool {
    let has_characters = markers.contains("characters");
    let has_chats = markers.contains("chats");
    if has_characters && has_chats {
        return true;
    }

    USER_DIR_MARKERS
        .iter()
        .filter(|marker| markers.contains(**marker))
        .count()
        >= 2
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

fn copy_stream_with_cancel<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    copy_buffer: &mut [u8],
    is_cancelled: &dyn Fn() -> bool,
    read_error_context: &str,
    write_error_context: &str,
) -> Result<(), DomainError> {
    loop {
        ensure_not_cancelled(is_cancelled)?;

        let bytes_read = reader
            .read(copy_buffer)
            .map_err(|error| internal_error(read_error_context, error))?;
        if bytes_read == 0 {
            break;
        }

        writer
            .write_all(&copy_buffer[..bytes_read])
            .map_err(|error| internal_error(write_error_context, error))?;
    }

    Ok(())
}

fn cleanup_directory_sync(path: &Path) {
    if let Err(error) = fs::remove_dir_all(path) {
        if error.kind() != io::ErrorKind::NotFound {
            tracing::warn!("Failed to clean up directory {}: {}", path.display(), error);
        }
    }
}

fn ensure_not_cancelled(is_cancelled: &dyn Fn() -> bool) -> Result<(), DomainError> {
    if is_cancelled() {
        return Err(DomainError::InternalError(
            CANCELLED_ERROR_MARKER.to_string(),
        ));
    }

    Ok(())
}

fn progress_percent(processed: u64, total: u64, min: f32, max: f32) -> f32 {
    if total == 0 {
        return max;
    }

    let ratio = (processed as f64 / total as f64).clamp(0.0, 1.0) as f32;
    min + (max - min) * ratio
}

fn internal_error(context: &str, error: impl std::fmt::Display) -> DomainError {
    DomainError::InternalError(format!("{}: {}", context, error))
}

pub fn default_export_file_name() -> String {
    format!(
        "tauritavern-data-{}.zip",
        Utc::now().format("%Y%m%d-%H%M%S")
    )
}
