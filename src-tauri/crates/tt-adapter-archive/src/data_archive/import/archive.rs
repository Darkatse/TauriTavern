use std::fmt::Display;
use std::fs::{self, File};
use std::io::{self, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;

use flate2::read::GzDecoder;
use tar::{Archive as TarArchive, EntryType};
use zip::ZipArchive;

use crate::data_archive::shared::{
    ByteProgress, COPY_BUFFER_BYTES, FILE_IO_BUFFER_BYTES, MAX_ARCHIVE_ENTRIES,
    copy_stream_with_cancel, ensure_not_cancelled, internal_error, is_macos_resource_fork_path,
    validate_archive_compression_ratio, validate_archive_entry_limits,
};
use crate::zipkit;
use tt_domain::errors::DomainError;

const CANCELLED_READ_MESSAGE: &str = "Job cancelled";
const MAX_ZIP_EXTRACTION_WORKERS: usize = 4;

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
    pub total_uncompressed_bytes: u64,
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
    archives: Vec<ZipArchive<File>>,
    scanned_archive: ScannedArchive,
    entries: Vec<ZipEntryPlan>,
}

impl ZipImportArchive {
    fn scanned_archive(&self) -> ScannedArchive {
        self.scanned_archive
    }

    pub fn stage(
        self,
        raw_root: &Path,
        include: &dyn Fn(&Path) -> bool,
        report_progress: &mut dyn FnMut(&str, f32, &str),
        is_cancelled: &dyn Fn() -> bool,
    ) -> Result<StagedArchive, DomainError> {
        ensure_not_cancelled(is_cancelled)?;
        report_progress("extracting", 15.0, "Extracting archive data");

        let Self {
            mut archives,
            scanned_archive,
            entries,
        } = self;
        let payload_root = raw_root.join("payloads");
        let mut staged_entries = Vec::new();
        let mut files = Vec::new();

        for entry in entries {
            ensure_not_cancelled(is_cancelled)?;
            if !include(&entry.path) {
                continue;
            }

            if entry.is_dir {
                staged_entries.push(StagedEntry::Directory { path: entry.path });
            } else {
                let payload_path = payload_root.join(entry.index.to_string());
                files.push(ZipFileStage {
                    index: entry.index,
                    payload_path: payload_path.clone(),
                });
                staged_entries.push(StagedEntry::File {
                    path: entry.path,
                    payload_path,
                });
            }
        }

        let mut progress = ByteProgress::new(scanned_archive.total_uncompressed_bytes, 15.0, 90.0);
        if !files.is_empty() {
            fs::create_dir_all(&payload_root).map_err(|error| {
                internal_error("Failed to create raw archive payload directory", error)
            })?;
            archives.truncate(archives.len().min(files.len()));
            stage_zip_files(
                archives,
                &files,
                &mut progress,
                report_progress,
                is_cancelled,
            )?;
        }

        progress.complete("extracting", "Archive extracted", report_progress);
        Ok(StagedArchive {
            scanned_archive,
            entries: staged_entries,
        })
    }
}

struct ZipEntryPlan {
    index: usize,
    path: PathBuf,
    is_dir: bool,
}

struct ZipFileStage {
    index: usize,
    payload_path: PathBuf,
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
    match ZipArchive::new(archive_file) {
        Ok(mut archive) => {
            let (scanned_archive, entries) = scan_zip_archive(&mut archive, is_cancelled, visit)?;
            let worker_count =
                zip_worker_count(entries.iter().filter(|entry| !entry.is_dir).count());
            let metadata = archive.metadata();
            let mut archives = Vec::with_capacity(worker_count);
            archives.push(archive);
            for _ in 1..worker_count {
                let file = File::open(archive_path)
                    .map_err(|error| internal_error("Failed to open zip worker reader", error))?;
                // SAFETY: the import contract keeps archive_path bound to the same unchanged ZIP
                // while readers are opened, and keeps its bytes unchanged while they are read.
                archives
                    .push(unsafe { ZipArchive::unsafe_new_with_metadata(file, metadata.clone()) });
            }
            Ok(PreparedArchive::Zip(ZipImportArchive {
                archives,
                scanned_archive,
                entries,
            }))
        }
        Err(error) if bytes_read >= 2 && magic[..2] == *b"PK" => {
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
) -> Result<(ScannedArchive, Vec<ZipEntryPlan>), DomainError> {
    let mut scanned_entries = 0usize;
    let mut total_uncompressed_bytes = 0u64;
    let mut entries = Vec::with_capacity(archive.len());

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
        entries.push(ZipEntryPlan {
            index,
            path: sanitized_path,
            is_dir: entry.is_dir(),
        });
    }

    Ok((
        ScannedArchive {
            scanned_entries,
            total_uncompressed_bytes,
        },
        entries,
    ))
}

fn zip_worker_count(file_count: usize) -> usize {
    thread::available_parallelism()
        .map_or(1, |available| available.get())
        .min(MAX_ZIP_EXTRACTION_WORKERS)
        .min(file_count.max(1))
}

fn stage_zip_files(
    archives: Vec<ZipArchive<File>>,
    files: &[ZipFileStage],
    progress: &mut ByteProgress,
    report_progress: &mut dyn FnMut(&str, f32, &str),
    is_cancelled: &dyn Fn() -> bool,
) -> Result<(), DomainError> {
    ensure_not_cancelled(is_cancelled)?;

    let next_file = AtomicUsize::new(0);
    let stopped = AtomicBool::new(false);
    let (progress_sender, progress_receiver) = mpsc::channel();

    thread::scope(|scope| {
        let mut workers = Vec::with_capacity(archives.len());
        for archive in archives {
            let progress_sender = progress_sender.clone();
            workers.push(
                scope.spawn(|| {
                    stage_zip_worker(archive, files, &next_file, &stopped, progress_sender)
                }),
            );
        }
        drop(progress_sender);

        let mut cancelled = false;
        while let Ok(bytes) = progress_receiver.recv() {
            if !cancelled && is_cancelled() {
                stopped.store(true, Ordering::Relaxed);
                cancelled = true;
                continue;
            }

            if !cancelled {
                progress.advance(
                    bytes,
                    "extracting",
                    "Extracting archive data",
                    report_progress,
                );
            }
        }

        if !cancelled && is_cancelled() {
            stopped.store(true, Ordering::Relaxed);
            cancelled = true;
        }

        let mut worker_error = None;
        for worker in workers {
            match worker.join() {
                Ok(Err(error)) if worker_error.is_none() => worker_error = Some(error),
                Ok(_) => {}
                Err(_) if worker_error.is_none() => {
                    worker_error = Some(DomainError::InternalError(
                        "Zip extraction worker panicked".to_string(),
                    ));
                }
                Err(_) => {}
            }
        }

        if let Some(error) = worker_error {
            Err(error)
        } else if cancelled {
            Err(DomainError::cancelled(CANCELLED_READ_MESSAGE))
        } else {
            Ok(())
        }
    })
}

fn stage_zip_worker(
    mut archive: ZipArchive<File>,
    files: &[ZipFileStage],
    next_file: &AtomicUsize,
    stopped: &AtomicBool,
    progress_sender: mpsc::Sender<u64>,
) -> Result<(), DomainError> {
    let mut copy_buffer = vec![0u8; COPY_BUFFER_BYTES];

    loop {
        if stopped.load(Ordering::Relaxed) {
            return Ok(());
        }

        let file_index = next_file.fetch_add(1, Ordering::Relaxed);
        let Some(file) = files.get(file_index) else {
            return Ok(());
        };

        if let Err(error) = stage_zip_file(
            &mut archive,
            file,
            &mut copy_buffer,
            stopped,
            &progress_sender,
        ) {
            if stopped.swap(true, Ordering::Relaxed) {
                return Ok(());
            }
            return Err(error);
        }
    }
}

fn stage_zip_file(
    archive: &mut ZipArchive<File>,
    file: &ZipFileStage,
    copy_buffer: &mut [u8],
    stopped: &AtomicBool,
    progress_sender: &mpsc::Sender<u64>,
) -> Result<(), DomainError> {
    let mut entry = archive
        .by_index(file.index)
        .map_err(|error| invalid_archive_error("Failed to read zip archive entry", error))?;
    let expected_size = entry.size();

    let mut output = File::create(&file.payload_path)
        .map_err(|error| internal_error("Failed to create raw archive output file", error))?;
    let mut written = 0u64;
    let is_stopped = || stopped.load(Ordering::Relaxed);
    let mut on_bytes_copied = |bytes| {
        written = written.saturating_add(bytes);
        let _ = progress_sender.send(bytes);
    };
    copy_stream_with_cancel(
        &mut entry,
        &mut output,
        copy_buffer,
        &is_stopped,
        &mut on_bytes_copied,
        "Failed to read zip archive entry data",
        "Failed to write raw archive output file",
    )?;

    if written != expected_size {
        return Err(DomainError::InvalidData(format!(
            "Zip archive entry size mismatch at index {}: {}/{}",
            file.index, written, expected_size
        )));
    }
    if written == 0 {
        let _ = progress_sender.send(0);
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
    report_progress("extracting", 15.0, "Extracting archive data");

    let staged_archive = match format {
        ArchiveFormat::Tar => stage_tar_reader(
            ProgressReader::new(archive_reader, compressed_size, report_progress),
            format,
            Some(compressed_size),
            raw_root,
            is_cancelled,
            visit,
        )?,
        ArchiveFormat::TarGz => stage_tar_reader(
            GzDecoder::new(ProgressReader::new(
                archive_reader,
                compressed_size,
                report_progress,
            )),
            format,
            Some(compressed_size),
            raw_root,
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
    is_cancelled: &dyn Fn() -> bool,
    visit: &mut dyn FnMut(&Path) -> Result<(), DomainError>,
) -> Result<StagedArchive, DomainError> {
    let mut archive = TarArchive::new(CancellableReader::new(reader, is_cancelled));
    let mut scanned_entries = 0usize;
    let mut total_uncompressed_bytes = 0u64;
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
            continue;
        }

        if entry_type.is_dir() {
            staged_entries.push(StagedEntry::Directory {
                path: sanitized_path,
            });
        } else {
            let payload_path = payload_root.join(staged_entries.len().to_string());
            stage_archive_file(&payload_path, &mut entry, &mut copy_buffer, is_cancelled)?;
            staged_entries.push(StagedEntry::File {
                path: sanitized_path,
                payload_path,
            });
        }
    }

    Ok(StagedArchive {
        scanned_archive: ScannedArchive {
            scanned_entries,
            total_uncompressed_bytes,
        },
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
        fs::create_dir_all(parent).map_err(|error| {
            internal_error("Failed to create raw archive parent directory", error)
        })?;
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

struct CancellableReader<'a, R> {
    inner: R,
    is_cancelled: &'a dyn Fn() -> bool,
}

struct ProgressReader<'a, R> {
    inner: R,
    progress: ByteProgress,
    report_progress: &'a mut dyn FnMut(&str, f32, &str),
}

impl<'a, R> ProgressReader<'a, R> {
    fn new(
        inner: R,
        total_bytes: u64,
        report_progress: &'a mut dyn FnMut(&str, f32, &str),
    ) -> Self {
        Self {
            inner,
            // Input may reach EOF before tar staging finishes; reserve 90% for completed staging.
            progress: ByteProgress::new(total_bytes, 15.0, 89.0),
            report_progress,
        }
    }
}

impl<R: Read> Read for ProgressReader<'_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let bytes_read = self.inner.read(buffer)?;
        self.progress.advance(
            bytes_read as u64,
            "extracting",
            "Extracting archive data",
            self.report_progress,
        );
        Ok(bytes_read)
    }
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

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use zip::ZipWriter;
    use zip::write::SimpleFileOptions;

    use super::*;

    #[test]
    fn zero_byte_zip_entry_wakes_cancellation_polling() {
        let root = std::env::temp_dir().join(format!(
            "tauritavern-empty-zip-cancel-{}",
            rand::random::<u64>()
        ));
        fs::create_dir_all(&root).expect("create test root");
        let archive_path = root.join("fixture.zip");
        let mut writer = ZipWriter::new(File::create(&archive_path).expect("create zip"));
        writer
            .start_file("empty", SimpleFileOptions::default())
            .expect("start empty file");
        writer.finish().expect("finish zip");

        let archive =
            ZipArchive::new(File::open(&archive_path).expect("open zip")).expect("read zip");
        let files = [ZipFileStage {
            index: 0,
            payload_path: root.join("payload"),
        }];
        let checks = AtomicUsize::new(0);
        let is_cancelled = || checks.fetch_add(1, Ordering::SeqCst) > 0;
        let mut progress = ByteProgress::new(0, 15.0, 90.0);
        let mut report_progress = |_stage: &str, _percent: f32, _message: &str| {};

        let error = stage_zip_files(
            vec![archive],
            &files,
            &mut progress,
            &mut report_progress,
            &is_cancelled,
        )
        .expect_err("empty entry should wake cancellation polling");

        assert!(matches!(error, DomainError::Cancelled(_)));
        fs::remove_dir_all(root).expect("remove test root");
    }
}
