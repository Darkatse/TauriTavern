use std::fmt::Display;
use std::fs::File;
use std::io::{self, BufReader, Read};
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;
use tar::{Archive as TarArchive, EntryType};
use zip::ZipArchive;

use crate::domain::errors::DomainError;
use crate::infrastructure::persistence::data_archive::shared::{
    COPY_BUFFER_BYTES, FILE_IO_BUFFER_BYTES, MAX_ARCHIVE_ENTRIES, ensure_not_cancelled,
    internal_error, validate_archive_compression_ratio, validate_archive_entry_limits,
};
use crate::infrastructure::zipkit;

const CANCELLED_READ_MESSAGE: &str = "Job cancelled";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveFormat {
    Zip,
    Tar,
    TarGz,
}

impl ArchiveFormat {
    fn label(self) -> &'static str {
        match self {
            Self::Zip => "zip",
            Self::Tar => "tar",
            Self::TarGz => "tar.gz",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScannedArchive {
    pub format: ArchiveFormat,
    pub scanned_entries: usize,
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

pub fn scan_archive(
    archive_path: &Path,
    is_cancelled: &dyn Fn() -> bool,
    visit: &mut dyn FnMut(&Path) -> Result<(), DomainError>,
) -> Result<ScannedArchive, DomainError> {
    let format = detect_archive_format(archive_path)?;
    match format {
        ArchiveFormat::Zip => scan_zip_archive(archive_path, is_cancelled, visit),
        ArchiveFormat::Tar => {
            scan_tar_archive(archive_path, ArchiveFormat::Tar, is_cancelled, visit)
        }
        ArchiveFormat::TarGz => {
            scan_tar_archive(archive_path, ArchiveFormat::TarGz, is_cancelled, visit)
        }
    }
}

pub fn read_archive_entries(
    archive_path: &Path,
    format: ArchiveFormat,
    is_cancelled: &dyn Fn() -> bool,
    visit: &mut dyn FnMut(ArchiveReadEntry<'_>) -> Result<(), DomainError>,
) -> Result<(), DomainError> {
    match format {
        ArchiveFormat::Zip => read_zip_entries(archive_path, is_cancelled, visit),
        ArchiveFormat::Tar => {
            read_tar_entries(archive_path, ArchiveFormat::Tar, is_cancelled, visit)
        }
        ArchiveFormat::TarGz => {
            read_tar_entries(archive_path, ArchiveFormat::TarGz, is_cancelled, visit)
        }
    }
}

fn detect_archive_format(archive_path: &Path) -> Result<ArchiveFormat, DomainError> {
    let mut file = File::open(archive_path)
        .map_err(|error| internal_error("Failed to open archive file", error))?;
    let mut magic = [0u8; 4];
    let bytes_read = file
        .read(&mut magic)
        .map_err(|error| internal_error("Failed to read archive header", error))?;

    if bytes_read >= 2 && magic[..2] == [0x1f, 0x8b] {
        return Ok(ArchiveFormat::TarGz);
    }

    let zip_probe = probe_zip_archive(archive_path);
    if zip_probe.is_ok() {
        return Ok(ArchiveFormat::Zip);
    }

    if bytes_read >= 2 && magic[..2] == [b'P', b'K'] {
        zip_probe?;
    }

    Ok(ArchiveFormat::Tar)
}

fn probe_zip_archive(archive_path: &Path) -> Result<(), DomainError> {
    let archive_file = File::open(archive_path)
        .map_err(|error| internal_error("Failed to open archive file", error))?;
    let archive_reader = BufReader::with_capacity(FILE_IO_BUFFER_BYTES, archive_file);
    ZipArchive::new(archive_reader)
        .map(|_| ())
        .map_err(|error| invalid_archive_error("Failed to parse zip archive", error))
}

fn scan_zip_archive(
    archive_path: &Path,
    is_cancelled: &dyn Fn() -> bool,
    visit: &mut dyn FnMut(&Path) -> Result<(), DomainError>,
) -> Result<ScannedArchive, DomainError> {
    let archive_file = File::open(archive_path)
        .map_err(|error| internal_error("Failed to open archive file", error))?;
    let archive_reader = BufReader::with_capacity(FILE_IO_BUFFER_BYTES, archive_file);
    let mut archive = ZipArchive::new(archive_reader)
        .map_err(|error| invalid_archive_error("Failed to parse zip archive", error))?;

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
        format: ArchiveFormat::Zip,
        scanned_entries,
    })
}

fn scan_tar_archive(
    archive_path: &Path,
    format: ArchiveFormat,
    is_cancelled: &dyn Fn() -> bool,
    visit: &mut dyn FnMut(&Path) -> Result<(), DomainError>,
) -> Result<ScannedArchive, DomainError> {
    let compressed_size = archive_path
        .metadata()
        .map_err(|error| internal_error("Failed to stat archive file", error))?
        .len();
    let archive_file = File::open(archive_path)
        .map_err(|error| internal_error("Failed to open archive file", error))?;
    let archive_reader = BufReader::with_capacity(FILE_IO_BUFFER_BYTES, archive_file);

    match format {
        ArchiveFormat::Tar => scan_tar_reader(
            archive_reader,
            format,
            Some(compressed_size),
            is_cancelled,
            visit,
        ),
        ArchiveFormat::TarGz => {
            let decoder = GzDecoder::new(archive_reader);
            scan_tar_reader(decoder, format, Some(compressed_size), is_cancelled, visit)
        }
        ArchiveFormat::Zip => unreachable!("zip archives are scanned by scan_zip_archive"),
    }
}

fn scan_tar_reader<R: Read>(
    reader: R,
    format: ArchiveFormat,
    compressed_size: Option<u64>,
    is_cancelled: &dyn Fn() -> bool,
    visit: &mut dyn FnMut(&Path) -> Result<(), DomainError>,
) -> Result<ScannedArchive, DomainError> {
    let mut archive = TarArchive::new(CancellableReader::new(reader, is_cancelled));
    let mut scanned_entries = 0usize;
    let mut total_uncompressed_bytes = 0u64;
    let mut skip_buffer = vec![0u8; COPY_BUFFER_BYTES];

    for entry in archive
        .entries()
        .map_err(|error| archive_io_error("Failed to read tar archive entries", error))?
    {
        ensure_not_cancelled(is_cancelled)?;

        let mut entry =
            entry.map_err(|error| archive_io_error("Failed to read tar archive entry", error))?;
        let display_name = tar_entry_display_name(&entry)?;
        let sanitized_path = zipkit::enclosed_archive_entry_path(&display_name)?;
        if sanitized_path.as_os_str().is_empty() {
            continue;
        }

        let entry_type = entry.header().entry_type();
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

        if entry_type.is_file() {
            drain_entry_data_with_cancel(&mut entry, &mut skip_buffer, is_cancelled)?;
        }
    }

    Ok(ScannedArchive {
        format,
        scanned_entries,
    })
}

fn read_zip_entries(
    archive_path: &Path,
    is_cancelled: &dyn Fn() -> bool,
    visit: &mut dyn FnMut(ArchiveReadEntry<'_>) -> Result<(), DomainError>,
) -> Result<(), DomainError> {
    let archive_file = File::open(archive_path)
        .map_err(|error| internal_error("Failed to open archive file", error))?;
    let archive_reader = BufReader::with_capacity(FILE_IO_BUFFER_BYTES, archive_file);
    let mut archive = ZipArchive::new(archive_reader)
        .map_err(|error| invalid_archive_error("Failed to parse zip archive", error))?;

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

fn read_tar_entries(
    archive_path: &Path,
    format: ArchiveFormat,
    is_cancelled: &dyn Fn() -> bool,
    visit: &mut dyn FnMut(ArchiveReadEntry<'_>) -> Result<(), DomainError>,
) -> Result<(), DomainError> {
    let archive_file = File::open(archive_path)
        .map_err(|error| internal_error("Failed to open archive file", error))?;
    let archive_reader = BufReader::with_capacity(FILE_IO_BUFFER_BYTES, archive_file);

    match format {
        ArchiveFormat::Tar => read_tar_reader(archive_reader, is_cancelled, visit),
        ArchiveFormat::TarGz => {
            read_tar_reader(GzDecoder::new(archive_reader), is_cancelled, visit)
        }
        ArchiveFormat::Zip => unreachable!("zip archives are read by read_zip_entries"),
    }
}

fn read_tar_reader<R: Read>(
    reader: R,
    is_cancelled: &dyn Fn() -> bool,
    visit: &mut dyn FnMut(ArchiveReadEntry<'_>) -> Result<(), DomainError>,
) -> Result<(), DomainError> {
    let mut archive = TarArchive::new(CancellableReader::new(reader, is_cancelled));

    for entry in archive
        .entries()
        .map_err(|error| archive_io_error("Failed to read tar archive entries", error))?
    {
        ensure_not_cancelled(is_cancelled)?;

        let mut entry =
            entry.map_err(|error| archive_io_error("Failed to read tar archive entry", error))?;
        let display_name = tar_entry_display_name(&entry)?;
        let sanitized_path = zipkit::enclosed_archive_entry_path(&display_name)?;
        if sanitized_path.as_os_str().is_empty() {
            continue;
        }

        let entry_type = entry.header().entry_type();
        ensure_supported_tar_entry_type(entry_type, &display_name)?;

        if entry_type.is_dir() {
            visit(ArchiveReadEntry::Directory {
                path: sanitized_path,
            })?;
            continue;
        }

        visit(ArchiveReadEntry::File {
            path: sanitized_path,
            reader: &mut entry,
        })?;
    }

    Ok(())
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
