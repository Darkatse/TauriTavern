use std::fmt::Display;
use std::fs::{self, File};
use std::io::{self, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;
use tar::{Archive as TarArchive, EntryType};
use zip::ZipArchive;

use crate::data_archive::shared::{
    COPY_BUFFER_BYTES, FILE_IO_BUFFER_BYTES, MAX_ARCHIVE_ENTRIES, PROGRESS_REPORT_MIN_DELTA,
    ensure_not_cancelled, internal_error, is_macos_resource_fork_path, progress_percent,
    validate_archive_compression_ratio, validate_archive_entry_limits,
};
use crate::zipkit;
use tt_domain::errors::DomainError;

const CANCELLED_READ_MESSAGE: &str = "Job cancelled";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveFormat {
    Tar,
    TarGz,
}

impl ArchiveFormat {
    fn label(self) -> &'static str {
        match self {
            Self::Tar => "tar",
            Self::TarGz => "tar.gz",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ScannedArchive {
    pub scanned_entries: usize,
}

pub enum PreparedArchive {
    Zip(ZipImportArchive),
    Staged(StagedArchive),
}

impl PreparedArchive {
    pub fn scanned_archive(&self) -> ScannedArchive {
        match self {
            Self::Zip(archive) => archive.scanned_archive(),
            Self::Staged(archive) => archive.scanned_archive,
        }
    }
}

pub struct ZipImportArchive {
    archive: ZipArchive<BufReader<File>>,
    scanned_entries: usize,
}

impl ZipImportArchive {
    fn scanned_archive(&self) -> ScannedArchive {
        ScannedArchive {
            scanned_entries: self.scanned_entries,
        }
    }

    pub fn read_entries(
        &mut self,
        is_cancelled: &dyn Fn() -> bool,
        visit: &mut dyn FnMut(ArchiveReadEntry<'_>) -> Result<(), DomainError>,
    ) -> Result<(), DomainError> {
        read_zip_entries(&mut self.archive, is_cancelled, visit)
    }
}

pub struct StagedArchive {
    scanned_archive: ScannedArchive,
    entries: Vec<StagedEntry>,
}

impl StagedArchive {
    pub fn entries(&self) -> &[StagedEntry] {
        &self.entries
    }
}

pub enum StagedEntry {
    Directory {
        path: PathBuf,
    },
    File {
        path: PathBuf,
        payload_path: PathBuf,
    },
}

impl StagedEntry {
    pub fn path(&self) -> &Path {
        match self {
            Self::Directory { path } | Self::File { path, .. } => path,
        }
    }
}

pub enum ArchiveReadEntry<'a> {
    Directory {
        path: PathBuf,
    },
    File {
        path: PathBuf,
        reader: &'a mut dyn Read,
    },
}

impl ArchiveReadEntry<'_> {
    pub fn path(&self) -> &Path {
        match self {
            Self::Directory { path } | Self::File { path, .. } => path,
        }
    }

    pub fn is_dir(&self) -> bool {
        matches!(self, Self::Directory { .. })
    }
}

pub fn prepare_archive_for_import(
    archive_path: &Path,
    raw_root: &Path,
    report_progress: &mut dyn FnMut(&str, f32, &str),
    is_cancelled: &dyn Fn() -> bool,
    visit: &mut dyn FnMut(&Path) -> Result<(), DomainError>,
) -> Result<PreparedArchive, DomainError> {
    let mut archive_file = File::open(archive_path)
        .map_err(|error| internal_error("Failed to open archive file", error))?;
    let mut magic = [0u8; 4];
    let bytes_read = archive_file
        .read(&mut magic)
        .map_err(|error| internal_error("Failed to read archive header", error))?;

    if bytes_read >= 2 && magic[..2] == [0x1f, 0x8b] {
        return stage_tar_archive(
            archive_path,
            ArchiveFormat::TarGz,
            raw_root,
            report_progress,
            is_cancelled,
            visit,
        )
        .map(PreparedArchive::Staged);
    }

    archive_file
        .seek(SeekFrom::Start(0))
        .map_err(|error| internal_error("Failed to seek archive file", error))?;
    let archive_reader = BufReader::with_capacity(FILE_IO_BUFFER_BYTES, archive_file);
    match ZipArchive::new(archive_reader) {
        Ok(mut archive) => {
            let scanned_archive = scan_zip_archive(&mut archive, is_cancelled, visit)?;
            Ok(PreparedArchive::Zip(ZipImportArchive {
                archive,
                scanned_entries: scanned_archive.scanned_entries,
            }))
        }
        Err(error) if bytes_read >= 2 && magic[..2] == [b'P', b'K'] => {
            Err(invalid_archive_error("Failed to parse zip archive", error))
        }
        Err(_) => stage_tar_archive(
            archive_path,
            ArchiveFormat::Tar,
            raw_root,
            report_progress,
            is_cancelled,
            visit,
        )
        .map(PreparedArchive::Staged),
    }
}

fn scan_zip_archive<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    is_cancelled: &dyn Fn() -> bool,
    visit: &mut dyn FnMut(&Path) -> Result<(), DomainError>,
) -> Result<ScannedArchive, DomainError> {
    let mut scanned_entries = 0usize;
    let mut total_uncompressed_bytes = 0u64;

    for index in 0..archive.len() {
        ensure_not_cancelled(is_cancelled)?;

        let entry = archive
            .by_index(index)
            .map_err(|error| invalid_archive_error("Failed to read zip archive entry", error))?;
        let (sanitized_path, entry_name) = zipkit::enclosed_zip_entry_path_with_name(&entry)?;
        if sanitized_path.as_os_str().is_empty() {
            continue;
        }

        validate_archive_entry_limits(
            entry_name,
            entry.size(),
            Some(entry.compressed_size()),
            &mut total_uncompressed_bytes,
        )?;

        scanned_entries = scanned_entries.saturating_add(1);
        ensure_entry_count_limit(scanned_entries)?;

        visit(&sanitized_path)?;
    }

    Ok(ScannedArchive {
        scanned_entries,
    })
}

fn read_zip_entries<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    is_cancelled: &dyn Fn() -> bool,
    visit: &mut dyn FnMut(ArchiveReadEntry<'_>) -> Result<(), DomainError>,
) -> Result<(), DomainError> {
    for index in 0..archive.len() {
        ensure_not_cancelled(is_cancelled)?;

        let mut archive_entry = archive
            .by_index(index)
            .map_err(|error| invalid_archive_error("Failed to read zip archive entry", error))?;
        let sanitized_path = zipkit::enclosed_zip_entry_path(&archive_entry)?;
        if sanitized_path.as_os_str().is_empty() {
            continue;
        }

        if archive_entry.is_dir() {
            visit(ArchiveReadEntry::Directory {
                path: sanitized_path,
            })?;
            continue;
        }

        visit(ArchiveReadEntry::File {
            path: sanitized_path,
            reader: &mut archive_entry,
        })?;
    }

    Ok(())
}

fn stage_tar_archive(
    archive_path: &Path,
    format: ArchiveFormat,
    raw_root: &Path,
    report_progress: &mut dyn FnMut(&str, f32, &str),
    is_cancelled: &dyn Fn() -> bool,
    visit: &mut dyn FnMut(&Path) -> Result<(), DomainError>,
) -> Result<StagedArchive, DomainError> {
    fs::create_dir_all(raw_root)
        .map_err(|error| internal_error("Failed to create raw archive workspace", error))?;
    let compressed_size = archive_path
        .metadata()
        .map_err(|error| internal_error("Failed to stat archive file", error))?
        .len();
    let archive_file = File::open(archive_path)
        .map_err(|error| internal_error("Failed to open archive file", error))?;
    let archive_reader = BufReader::with_capacity(FILE_IO_BUFFER_BYTES, archive_file);

    let staged_archive = match format {
        ArchiveFormat::Tar => stage_tar_reader(
            archive_reader,
            format,
            Some(compressed_size),
            raw_root,
            report_progress,
            is_cancelled,
            visit,
        )?,
        ArchiveFormat::TarGz => stage_tar_reader(
            GzDecoder::new(archive_reader),
            format,
            Some(compressed_size),
            raw_root,
            report_progress,
            is_cancelled,
            visit,
        )?,
    };

    if staged_archive.scanned_archive.scanned_entries > 0 {
        report_progress("extracting", 90.0, "Archive extracted");
    }

    Ok(staged_archive)
}

fn stage_tar_reader<R: Read>(
    reader: R,
    format: ArchiveFormat,
    compressed_size: Option<u64>,
    raw_root: &Path,
    report_progress: &mut dyn FnMut(&str, f32, &str),
    is_cancelled: &dyn Fn() -> bool,
    visit: &mut dyn FnMut(&Path) -> Result<(), DomainError>,
) -> Result<StagedArchive, DomainError> {
    let mut archive = TarArchive::new(CancellableReader::new(reader, is_cancelled));
    let mut scanned_entries = 0usize;
    let mut total_uncompressed_bytes = 0u64;
    let mut last_reported_percent = 0.0f32;
    let mut copy_buffer = vec![0u8; COPY_BUFFER_BYTES];
    let payload_root = raw_root.join("payloads");
    let mut staged_entries = Vec::new();

    for entry in archive
        .entries()
        .map_err(|error| archive_io_error("Failed to read tar archive entries", error))?
    {
        ensure_not_cancelled(is_cancelled)?;

        let mut entry =
            entry.map_err(|error| archive_io_error("Failed to read tar archive entry", error))?;
        let display_name = tar_entry_display_name(&entry)?;
        let sanitized_path = zipkit::enclosed_archive_entry_path(&display_name)?;
        let entry_type = entry.header().entry_type();

        if sanitized_path.as_os_str().is_empty() {
            if entry_type.is_file() {
                drain_entry_data_with_cancel(&mut entry, &mut copy_buffer, is_cancelled)?;
            }
            continue;
        }

        ensure_supported_tar_entry_type(entry_type, &display_name)?;
        validate_archive_entry_limits(
            &display_name,
            entry.size(),
            None,
            &mut total_uncompressed_bytes,
        )?;

        if format == ArchiveFormat::TarGz {
            validate_archive_compression_ratio(
                format.label(),
                total_uncompressed_bytes,
                compressed_size,
            )?;
        }

        scanned_entries = scanned_entries.saturating_add(1);
        ensure_entry_count_limit(scanned_entries)?;

        visit(&sanitized_path)?;

        if is_macos_resource_fork_path(&sanitized_path) {
            if entry_type.is_file() {
                drain_entry_data_with_cancel(&mut entry, &mut copy_buffer, is_cancelled)?;
            }
            maybe_report_staging_progress(scanned_entries, &mut last_reported_percent, report_progress);
            continue;
        }

        if entry_type.is_dir() {
            staged_entries.push(StagedEntry::Directory {
                path: sanitized_path,
            });
        } else {
            let payload_path = payload_root.join(staged_entries.len().to_string());
            stage_archive_file(
                &payload_path,
                &mut entry,
                &mut copy_buffer,
                is_cancelled,
            )?;
            staged_entries.push(StagedEntry::File {
                path: sanitized_path,
                payload_path,
            });
        }

        maybe_report_staging_progress(scanned_entries, &mut last_reported_percent, report_progress);
    }

    Ok(StagedArchive {
        scanned_archive: ScannedArchive { scanned_entries },
        entries: staged_entries,
    })
}

fn stage_archive_file(
    payload_path: &Path,
    reader: &mut dyn Read,
    copy_buffer: &mut [u8],
    is_cancelled: &dyn Fn() -> bool,
) -> Result<(), DomainError> {
    if let Some(parent) = payload_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| internal_error("Failed to create raw archive parent directory", error))?;
    }

    let mut output_file = File::create(payload_path)
        .map_err(|error| internal_error("Failed to create raw archive output file", error))?;
    loop {
        ensure_not_cancelled(is_cancelled)?;

        let bytes_read = reader
            .read(copy_buffer)
            .map_err(|error| archive_io_error("Failed to read tar archive entry data", error))?;
        if bytes_read == 0 {
            break;
        }

        output_file
            .write_all(&copy_buffer[..bytes_read])
            .map_err(|error| internal_error("Failed to write raw archive output file", error))?;
    }

    Ok(())
}

fn maybe_report_staging_progress(
    processed_entries: usize,
    last_reported_percent: &mut f32,
    report_progress: &mut dyn FnMut(&str, f32, &str),
) {
    let percent = progress_percent(processed_entries as u64, MAX_ARCHIVE_ENTRIES as u64, 15.0, 89.0);
    let should_report =
        processed_entries == 1 || percent - *last_reported_percent >= PROGRESS_REPORT_MIN_DELTA;
    if !should_report {
        return;
    }

    *last_reported_percent = percent;
    report_progress("extracting", percent, "Extracting archive");
}

struct CancellableReader<'a, R> {
    inner: R,
    is_cancelled: &'a dyn Fn() -> bool,
}

impl<'a, R> CancellableReader<'a, R> {
    fn new(inner: R, is_cancelled: &'a dyn Fn() -> bool) -> Self {
        Self {
            inner,
            is_cancelled,
        }
    }
}

impl<R: Read> Read for CancellableReader<'_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if (self.is_cancelled)() {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                CANCELLED_READ_MESSAGE,
            ));
        }

        self.inner.read(buffer)
    }
}

fn ensure_supported_tar_entry_type(
    entry_type: EntryType,
    display_name: &str,
) -> Result<(), DomainError> {
    if entry_type.is_file() || entry_type.is_dir() {
        return Ok(());
    }

    Err(DomainError::InvalidData(format!(
        "Unsupported archive entry type: {}",
        display_name
    )))
}

fn tar_entry_display_name<R: Read>(entry: &tar::Entry<'_, R>) -> Result<String, DomainError> {
    let path_bytes = entry.path_bytes();
    if path_bytes.contains(&0) {
        return Err(DomainError::InvalidData(
            "Invalid archive entry path (NUL byte)".to_string(),
        ));
    }

    let name = std::str::from_utf8(&path_bytes).map_err(|error| {
        DomainError::InvalidData(format!("Invalid archive entry path encoding: {}", error))
    })?;
    Ok(name.to_string())
}

fn drain_entry_data_with_cancel<R: Read>(
    reader: &mut R,
    buffer: &mut [u8],
    is_cancelled: &dyn Fn() -> bool,
) -> Result<(), DomainError> {
    loop {
        ensure_not_cancelled(is_cancelled)?;

        let bytes_read = reader
            .read(buffer)
            .map_err(|error| archive_io_error("Failed to read tar archive entry data", error))?;
        if bytes_read == 0 {
            return Ok(());
        }
    }
}

fn archive_io_error(context: &str, error: io::Error) -> DomainError {
    if error.kind() == io::ErrorKind::Interrupted {
        return DomainError::cancelled(CANCELLED_READ_MESSAGE);
    }

    invalid_archive_error(context, error)
}

fn invalid_archive_error(context: &str, error: impl Display) -> DomainError {
    DomainError::InvalidData(format!("{}: {}", context, error))
}

fn ensure_entry_count_limit(scanned_entries: usize) -> Result<(), DomainError> {
    if scanned_entries > MAX_ARCHIVE_ENTRIES {
        return Err(DomainError::InvalidData(format!(
            "Archive contains too many entries (>{})",
            MAX_ARCHIVE_ENTRIES
        )));
    }

    Ok(())
}
