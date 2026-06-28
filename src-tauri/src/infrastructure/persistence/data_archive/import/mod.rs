mod apply;
mod archive;
mod extract;
mod layout;

use std::fs;
use std::path::Path;

use crate::domain::errors::DomainError;
use crate::domain::models::data_archive::DataArchiveImportFailure;

use super::DataArchiveImportResult;
use super::shared::{
    IMPORT_TARGET_USER_HANDLE, cleanup_directory_sync, ensure_not_cancelled, internal_error,
};

pub fn run_import_data_archive(
    data_root: &Path,
    archive_path: &Path,
    workspace_root: &Path,
    report_progress: &mut dyn FnMut(&str, f32, &str),
    is_cancelled: &dyn Fn() -> bool,
) -> Result<DataArchiveImportResult, DataArchiveImportFailure> {
    report_progress("preparing", 0.0, "Preparing import");
    ensure_not_cancelled(is_cancelled)?;

    if !archive_path.is_file() {
        return Err(DomainError::InvalidData(format!(
            "Archive file does not exist: {}",
            archive_path.display()
        ))
        .into());
    }

    let normalized_root = workspace_root.join("normalized");
    if normalized_root.exists() {
        cleanup_directory_sync(&normalized_root);
    }
    fs::create_dir_all(&normalized_root)
        .map_err(|error| internal_error("Failed to create normalized workspace", error))?;

    let layout = layout::detect_archive_layout(archive_path, is_cancelled)?;
    report_progress("scanning", 10.0, "Archive layout detected");
    ensure_not_cancelled(is_cancelled)?;

    extract::extract_to_normalized_root_streaming(
        archive_path,
        &layout,
        &normalized_root,
        report_progress,
        is_cancelled,
    )?;

    report_progress("applying", 92.0, "Merging data directory");
    ensure_not_cancelled(is_cancelled)?;
    let local_applied =
        apply::apply_overlay(&normalized_root, data_root, report_progress, is_cancelled)?;

    report_progress("completed", 100.0, "Import completed");

    Ok(DataArchiveImportResult {
        source_users: layout.source_user_handles_for_import_result(),
        target_user: IMPORT_TARGET_USER_HANDLE.to_string(),
        local_applied,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use base64::Engine;
    use flate2::Compression as GzipCompression;
    use flate2::write::GzEncoder;
    use std::fs;
    use std::io::Cursor;
    use std::io::Write;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tar::{Builder as TarBuilder, EntryType, Header};
    use zip::CompressionMethod;
    use zip::ZipWriter;
    use zip::write::SimpleFileOptions as FileOptions;

    const UNICODE_PATH_FIXTURE_BASE64: &str = "UEsDBBQAAAAAAAAAAACBC0z9EgAAABIAAAAmADEAZGF0YS9kZWZhdWx0LXVzZXIvY2hhcmFjdGVycy/W0M7ELmpzb251cC0AAcO1/b1kYXRhL2RlZmF1bHQtdXNlci9jaGFyYWN0ZXJzL+S4reaWhy5qc29ueyJuYW1lIjoi5Lit5paHIn0KUEsDBBQAAAAAAAAAAACC6jpGEQAAABEAAAAjAAAAZGF0YS9kZWZhdWx0LXVzZXIvY2hhdHMvaGVsbG8uanNvbmx7ImNoYXQiOiJoZWxsbyJ9ClBLAQIUABQAAAAAAAAAAACBC0z9EgAAABIAAAAmADEAAAAAAAAAAAAAAAAAAABkYXRhL2RlZmF1bHQtdXNlci9jaGFyYWN0ZXJzL9bQzsQuanNvbnVwLQABw7X9vWRhdGEvZGVmYXVsdC11c2VyL2NoYXJhY3RlcnMv5Lit5paHLmpzb25QSwECFAAUAAAAAAAAAAAAguo6RhEAAAARAAAAIwAAAAAAAAAAAAAAAACHAAAAZGF0YS9kZWZhdWx0LXVzZXIvY2hhdHMvaGVsbG8uanNvbmxQSwUGAAAAAAIAAgDWAAAA2QAAAAAA";

    fn decode_fixture() -> Vec<u8> {
        base64::engine::general_purpose::STANDARD
            .decode(UNICODE_PATH_FIXTURE_BASE64)
            .expect("decode base64 fixture")
    }

    fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let file = fs::File::create(path).expect("create zip");
        let mut writer = ZipWriter::new(file);
        for (name, bytes) in entries {
            writer
                .start_file(*name, FileOptions::default())
                .expect("start file");
            writer.write_all(bytes).expect("write bytes");
        }
        writer.finish().expect("finish zip");
    }

    fn append_tar_file<W: Write>(builder: &mut TarBuilder<W>, name: &str, bytes: &[u8]) {
        let mut header = Header::new_gnu();
        header.set_entry_type(EntryType::Regular);
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, name, Cursor::new(bytes))
            .expect("append tar file");
    }

    fn append_tar_symlink<W: Write>(builder: &mut TarBuilder<W>, name: &str, target: &str) {
        let mut header = Header::new_gnu();
        header.set_entry_type(EntryType::Symlink);
        header.set_size(0);
        header.set_mode(0o777);
        header.set_link_name(target).expect("set link target");
        header.set_cksum();
        builder
            .append_data(&mut header, name, Cursor::new(Vec::<u8>::new()))
            .expect("append tar symlink");
    }

    fn write_tar(path: &Path, entries: &[(&str, &[u8])]) {
        let file = fs::File::create(path).expect("create tar");
        let mut builder = TarBuilder::new(file);
        for (name, bytes) in entries {
            append_tar_file(&mut builder, name, bytes);
        }
        builder.finish().expect("finish tar");
    }

    fn write_tar_gz(path: &Path, entries: &[(&str, &[u8])]) {
        let file = fs::File::create(path).expect("create tar.gz");
        let encoder = GzEncoder::new(file, GzipCompression::default());
        let mut builder = TarBuilder::new(encoder);
        for (name, bytes) in entries {
            append_tar_file(&mut builder, name, bytes);
        }
        let encoder = builder.into_inner().expect("finish tar stream");
        encoder.finish().expect("finish gzip stream");
    }

    fn write_tar_gz_symlink(path: &Path, name: &str, target: &str) {
        let file = fs::File::create(path).expect("create tar.gz");
        let encoder = GzEncoder::new(file, GzipCompression::default());
        let mut builder = TarBuilder::new(encoder);
        append_tar_symlink(&mut builder, name, target);
        let encoder = builder.into_inner().expect("finish tar stream");
        encoder.finish().expect("finish gzip stream");
    }

    fn write_raw_tar_file(path: &Path, name: &str, bytes: &[u8]) {
        let mut header = [0u8; 512];
        let name_bytes = name.as_bytes();
        assert!(
            name_bytes.len() <= 100,
            "raw tar helper only supports short names"
        );
        header[..name_bytes.len()].copy_from_slice(name_bytes);
        write_tar_octal(&mut header[100..108], 0o644);
        write_tar_octal(&mut header[108..116], 0);
        write_tar_octal(&mut header[116..124], 0);
        write_tar_octal(&mut header[124..136], bytes.len() as u64);
        write_tar_octal(&mut header[136..148], 0);
        header[148..156].fill(b' ');
        header[156] = b'0';
        header[257..263].copy_from_slice(b"ustar\0");
        header[263..265].copy_from_slice(b"00");

        let checksum = header.iter().map(|byte| u32::from(*byte)).sum::<u32>();
        let checksum_text = format!("{:06o}\0 ", checksum);
        header[148..156].copy_from_slice(checksum_text.as_bytes());

        let mut file = fs::File::create(path).expect("create raw tar");
        file.write_all(&header).expect("write raw tar header");
        file.write_all(bytes).expect("write raw tar payload");

        let padding = (512 - (bytes.len() % 512)) % 512;
        if padding > 0 {
            file.write_all(&vec![0u8; padding])
                .expect("write raw tar padding");
        }
        file.write_all(&[0u8; 1024])
            .expect("write raw tar terminator");
    }

    fn write_tar_octal(field: &mut [u8], value: u64) {
        let text = format!("{:0width$o}\0", value, width = field.len() - 1);
        field.copy_from_slice(text.as_bytes());
    }

    fn write_zip_bytes(entries: &[(&str, &[u8])], options: FileOptions) -> Vec<u8> {
        let cursor = Cursor::new(Vec::<u8>::new());
        let mut writer = ZipWriter::new(cursor);

        for (name, bytes) in entries {
            writer.start_file(*name, options).expect("start file");
            writer.write_all(bytes).expect("write bytes");
        }

        writer.finish().expect("finish zip").into_inner()
    }

    fn clear_zip_utf8_flag(bytes: &mut [u8]) -> usize {
        const UTF8_FLAG: u16 = 1u16 << 11;
        let mut patched = 0usize;

        let mut index = 0usize;
        while index + 4 <= bytes.len() {
            if bytes[index..].starts_with(b"PK\x03\x04") {
                if index + 8 <= bytes.len() {
                    let offset = index + 6;
                    let flags = u16::from_le_bytes([bytes[offset], bytes[offset + 1]]);
                    let flags = flags & !UTF8_FLAG;
                    bytes[offset..offset + 2].copy_from_slice(&flags.to_le_bytes());
                    patched += 1;
                }
                index += 4;
                continue;
            }

            if bytes[index..].starts_with(b"PK\x01\x02") {
                if index + 10 <= bytes.len() {
                    let offset = index + 8;
                    let flags = u16::from_le_bytes([bytes[offset], bytes[offset + 1]]);
                    let flags = flags & !UTF8_FLAG;
                    bytes[offset..offset + 2].copy_from_slice(&flags.to_le_bytes());
                    patched += 1;
                }
                index += 4;
                continue;
            }

            index += 1;
        }

        patched
    }

    #[test]
    fn zip_unicode_extra_field_overrides_non_utf8_filename() {
        let bytes = decode_fixture();
        let reader = std::io::Cursor::new(bytes);

        let mut archive = zip::ZipArchive::new(reader).expect("parse fixture zip");
        let mut names = (0..archive.len())
            .map(|index| {
                archive
                    .by_index(index)
                    .expect("read entry")
                    .name()
                    .to_string()
            })
            .collect::<Vec<_>>();
        names.sort();

        assert!(
            names
                .iter()
                .any(|name| name.ends_with("data/default-user/characters/中文.json"))
        );
    }

    #[test]
    fn import_preserves_unicode_filenames() {
        let root = std::env::temp_dir().join(format!(
            "tauritavern-data-archive-unicode-{}",
            rand::random::<u64>()
        ));
        let data_root = root.join("data");
        let workspace_root = root.join("workspace");
        let archive_path = root.join("fixture.zip");

        fs::create_dir_all(&root).expect("create temp root");
        fs::create_dir_all(&workspace_root).expect("create temp workspace");
        fs::write(&archive_path, decode_fixture()).expect("write fixture zip");

        let mut report_progress = |_stage: &str, _percent: f32, _message: &str| {};
        let is_cancelled = || false;

        run_import_data_archive(
            &data_root,
            &archive_path,
            &workspace_root,
            &mut report_progress,
            &is_cancelled,
        )
        .expect("import archive");

        let imported = data_root
            .join("default-user")
            .join("characters")
            .join("中文.json");
        assert!(imported.is_file(), "imported file should exist");

        let text = fs::read_to_string(&imported).expect("read imported file");
        assert!(text.contains("中文"), "imported content should match");

        cleanup_directory_sync(&root);
    }

    #[test]
    fn import_is_incremental_overlay() {
        let root = std::env::temp_dir().join(format!(
            "tauritavern-data-archive-overlay-{}",
            rand::random::<u64>()
        ));
        let data_root = root.join("data");
        let workspace_root = root.join("workspace");
        let archive_path = root.join("fixture.zip");

        fs::create_dir_all(data_root.join("default-user").join("chats")).expect("create chats");
        fs::write(
            data_root
                .join("default-user")
                .join("chats")
                .join("keep.jsonl"),
            "keep",
        )
        .expect("write keep file");

        fs::create_dir_all(&workspace_root).expect("create workspace");
        write_zip(
            &archive_path,
            &[("default-user/characters/new.json", br#"{ "new": true }"#)],
        );

        let mut report_progress = |_stage: &str, _percent: f32, _message: &str| {};
        let is_cancelled = || false;

        run_import_data_archive(
            &data_root,
            &archive_path,
            &workspace_root,
            &mut report_progress,
            &is_cancelled,
        )
        .expect("import archive");

        assert!(
            data_root
                .join("default-user")
                .join("chats")
                .join("keep.jsonl")
                .is_file(),
            "existing file should remain"
        );
        assert_eq!(
            fs::read_to_string(
                data_root
                    .join("default-user")
                    .join("chats")
                    .join("keep.jsonl")
            )
            .expect("read keep file"),
            "keep"
        );
        assert!(
            data_root
                .join("default-user")
                .join("characters")
                .join("new.json")
                .is_file(),
            "new file should be imported"
        );

        cleanup_directory_sync(&root);
    }

    #[test]
    fn import_overwrites_same_path_files() {
        let root = std::env::temp_dir().join(format!(
            "tauritavern-data-archive-overwrite-{}",
            rand::random::<u64>()
        ));
        let data_root = root.join("data");
        let workspace_root = root.join("workspace");
        let archive_path = root.join("fixture.zip");

        fs::create_dir_all(data_root.join("default-user").join("characters"))
            .expect("create characters");
        fs::write(
            data_root
                .join("default-user")
                .join("characters")
                .join("a.json"),
            "old",
        )
        .expect("write old file");

        fs::create_dir_all(&workspace_root).expect("create workspace");
        write_zip(&archive_path, &[("default-user/characters/a.json", b"new")]);

        let mut report_progress = |_stage: &str, _percent: f32, _message: &str| {};
        let is_cancelled = || false;

        run_import_data_archive(
            &data_root,
            &archive_path,
            &workspace_root,
            &mut report_progress,
            &is_cancelled,
        )
        .expect("import archive");

        assert_eq!(
            fs::read_to_string(
                data_root
                    .join("default-user")
                    .join("characters")
                    .join("a.json")
            )
            .expect("read overwritten file"),
            "new"
        );

        cleanup_directory_sync(&root);
    }

    #[test]
    fn import_supports_tar_archives() {
        let root = std::env::temp_dir().join(format!(
            "tauritavern-data-archive-tar-{}",
            rand::random::<u64>()
        ));
        let data_root = root.join("data");
        let workspace_root = root.join("workspace");
        let archive_path = root.join("fixture.tar");

        fs::create_dir_all(&root).expect("create temp root");
        fs::create_dir_all(&workspace_root).expect("create temp workspace");
        write_tar(
            &archive_path,
            &[(
                "data/default-user/characters/tar.json",
                br#"{ "tar": true }"#,
            )],
        );

        let mut report_progress = |_stage: &str, _percent: f32, _message: &str| {};
        let is_cancelled = || false;

        run_import_data_archive(
            &data_root,
            &archive_path,
            &workspace_root,
            &mut report_progress,
            &is_cancelled,
        )
        .expect("import tar archive");

        assert!(
            data_root
                .join("default-user")
                .join("characters")
                .join("tar.json")
                .is_file(),
            "tar file should be imported"
        );

        cleanup_directory_sync(&root);
    }

    #[test]
    fn import_supports_tar_gz_archives() {
        let root = std::env::temp_dir().join(format!(
            "tauritavern-data-archive-targz-{}",
            rand::random::<u64>()
        ));
        let data_root = root.join("data");
        let workspace_root = root.join("workspace");
        let archive_path = root.join("fixture.tar.gz");

        fs::create_dir_all(&root).expect("create temp root");
        fs::create_dir_all(&workspace_root).expect("create temp workspace");
        write_tar_gz(
            &archive_path,
            &[(
                "BackupRoot/data/default-user/chats/targz.jsonl",
                br#"{ "tar_gz": true }"#,
            )],
        );

        let mut report_progress = |_stage: &str, _percent: f32, _message: &str| {};
        let is_cancelled = || false;

        run_import_data_archive(
            &data_root,
            &archive_path,
            &workspace_root,
            &mut report_progress,
            &is_cancelled,
        )
        .expect("import tar.gz archive");

        assert!(
            data_root
                .join("default-user")
                .join("chats")
                .join("targz.jsonl")
                .is_file(),
            "tar.gz file should be imported"
        );

        cleanup_directory_sync(&root);
    }

    #[test]
    fn import_detects_tar_gz_by_content_not_extension() {
        let root = std::env::temp_dir().join(format!(
            "tauritavern-data-archive-tgz-magic-{}",
            rand::random::<u64>()
        ));
        let data_root = root.join("data");
        let workspace_root = root.join("workspace");
        let archive_path = root.join("fixture.zip");

        fs::create_dir_all(&root).expect("create temp root");
        fs::create_dir_all(&workspace_root).expect("create temp workspace");
        write_tar_gz(
            &archive_path,
            &[(
                "default-user/worlds/content-detected.json",
                br#"{ "ok": true }"#,
            )],
        );

        let mut report_progress = |_stage: &str, _percent: f32, _message: &str| {};
        let is_cancelled = || false;

        run_import_data_archive(
            &data_root,
            &archive_path,
            &workspace_root,
            &mut report_progress,
            &is_cancelled,
        )
        .expect("import content-detected tar.gz archive");

        assert!(
            data_root
                .join("default-user")
                .join("worlds")
                .join("content-detected.json")
                .is_file(),
            "tar.gz content should import even when staging name is not reliable"
        );

        cleanup_directory_sync(&root);
    }

    #[test]
    fn tar_import_rejects_path_escape() {
        let root = std::env::temp_dir().join(format!(
            "tauritavern-data-archive-tar-escape-{}",
            rand::random::<u64>()
        ));
        let data_root = root.join("data");
        let workspace_root = root.join("workspace");
        let archive_path = root.join("fixture.tar");

        fs::create_dir_all(&root).expect("create temp root");
        fs::create_dir_all(&workspace_root).expect("create temp workspace");
        write_raw_tar_file(&archive_path, "../escape.json", b"bad");

        let mut report_progress = |_stage: &str, _percent: f32, _message: &str| {};
        let is_cancelled = || false;

        let error = run_import_data_archive(
            &data_root,
            &archive_path,
            &workspace_root,
            &mut report_progress,
            &is_cancelled,
        )
        .expect_err("path escape should be rejected");
        assert!(matches!(error.error, DomainError::InvalidData(_)));

        cleanup_directory_sync(&root);
    }

    #[test]
    fn import_rejects_malformed_archive_as_invalid_data() {
        let root = std::env::temp_dir().join(format!(
            "tauritavern-data-archive-malformed-{}",
            rand::random::<u64>()
        ));
        let data_root = root.join("data");
        let workspace_root = root.join("workspace");
        let archive_path = root.join("fixture.archive");

        fs::create_dir_all(&root).expect("create temp root");
        fs::create_dir_all(&workspace_root).expect("create temp workspace");
        fs::write(&archive_path, b"not a zip, tar, or gzip archive").expect("write archive");

        let mut report_progress = |_stage: &str, _percent: f32, _message: &str| {};
        let is_cancelled = || false;

        let error = run_import_data_archive(
            &data_root,
            &archive_path,
            &workspace_root,
            &mut report_progress,
            &is_cancelled,
        )
        .expect_err("malformed archive should be rejected");
        assert!(
            matches!(error.error, DomainError::InvalidData(_)),
            "malformed archive should be invalid data, got: {}",
            error.error
        );

        cleanup_directory_sync(&root);
    }

    #[test]
    fn tar_scan_reports_cancelled_errors_as_cancelled() {
        let root = std::env::temp_dir().join(format!(
            "tauritavern-data-archive-tar-cancel-{}",
            rand::random::<u64>()
        ));
        let archive_path = root.join("fixture.tar");
        let large_payload = vec![0u8; 2 * 1024 * 1024];

        fs::create_dir_all(&root).expect("create temp root");
        write_tar(
            &archive_path,
            &[("data/default-user/chats/large.jsonl", &large_payload)],
        );

        let checks = AtomicUsize::new(0);
        let is_cancelled = || checks.fetch_add(1, Ordering::SeqCst) >= 2;

        let error = layout::detect_archive_layout(&archive_path, &is_cancelled)
            .expect_err("cancelled scan should fail");
        assert!(
            matches!(error, DomainError::Cancelled(_)),
            "cancelled scan should stay cancelled, got: {}",
            error
        );

        cleanup_directory_sync(&root);
    }

    #[test]
    fn tar_gz_import_rejects_symlinks() {
        let root = std::env::temp_dir().join(format!(
            "tauritavern-data-archive-targz-symlink-{}",
            rand::random::<u64>()
        ));
        let data_root = root.join("data");
        let workspace_root = root.join("workspace");
        let archive_path = root.join("fixture.tgz");

        fs::create_dir_all(&root).expect("create temp root");
        fs::create_dir_all(&workspace_root).expect("create temp workspace");
        write_tar_gz_symlink(
            &archive_path,
            "data/default-user/characters/link.json",
            "target.json",
        );

        let mut report_progress = |_stage: &str, _percent: f32, _message: &str| {};
        let is_cancelled = || false;

        let error = run_import_data_archive(
            &data_root,
            &archive_path,
            &workspace_root,
            &mut report_progress,
            &is_cancelled,
        )
        .expect_err("symlink should be rejected");
        assert!(matches!(error.error, DomainError::InvalidData(_)));

        cleanup_directory_sync(&root);
    }

    #[test]
    fn import_supports_sillytavern_user_root_layout() {
        let root = std::env::temp_dir().join(format!(
            "tauritavern-data-archive-sillytavern-user-root-{}",
            rand::random::<u64>()
        ));
        let data_root = root.join("data");
        let workspace_root = root.join("workspace");
        let archive_path = root.join("fixture.zip");

        fs::create_dir_all(&workspace_root).expect("create workspace");
        write_zip(&archive_path, &[("characters/root.json", b"{}")]);

        let mut report_progress = |_stage: &str, _percent: f32, _message: &str| {};
        let is_cancelled = || false;

        run_import_data_archive(
            &data_root,
            &archive_path,
            &workspace_root,
            &mut report_progress,
            &is_cancelled,
        )
        .expect("import archive");

        assert!(
            data_root
                .join("default-user")
                .join("characters")
                .join("root.json")
                .is_file(),
            "SillyTavern user-root archive should map into default-user"
        );

        cleanup_directory_sync(&root);
    }

    #[test]
    fn import_supports_sillytavern_native_user_backup_layout() {
        let root = std::env::temp_dir().join(format!(
            "tauritavern-data-archive-native-user-backup-{}",
            rand::random::<u64>()
        ));
        let data_root = root.join("data");
        let workspace_root = root.join("workspace");
        let archive_path = root.join("fixture.zip");

        fs::create_dir_all(&workspace_root).expect("create workspace");
        write_zip(
            &archive_path,
            &[
                ("settings.json", br#"{ "setting": true }"#),
                ("characters/Alice.json", br#"{ "name": "Alice" }"#),
                ("chats/characters/session.jsonl", b"chat"),
                ("groups/group.json", br#"{ "id": "group" }"#),
                ("group chats/group-session.jsonl", b"group chat"),
                ("assets/worlds/cover.png", b"image"),
            ],
        );

        let mut report_progress = |_stage: &str, _percent: f32, _message: &str| {};
        let is_cancelled = || false;

        run_import_data_archive(
            &data_root,
            &archive_path,
            &workspace_root,
            &mut report_progress,
            &is_cancelled,
        )
        .expect("import archive");

        assert!(
            data_root
                .join("default-user")
                .join("settings.json")
                .is_file(),
            "settings.json should map into default-user"
        );
        assert!(
            data_root
                .join("default-user")
                .join("chats")
                .join("characters")
                .join("session.jsonl")
                .is_file(),
            "marker-like chat paths should remain user-root content"
        );
        assert!(
            data_root
                .join("default-user")
                .join("assets")
                .join("worlds")
                .join("cover.png")
                .is_file(),
            "marker-like asset paths should remain user-root content"
        );

        cleanup_directory_sync(&root);
    }

    #[test]
    fn import_supports_settings_single_file() {
        let root = std::env::temp_dir().join(format!(
            "tauritavern-data-archive-settings-{}",
            rand::random::<u64>()
        ));
        let data_root = root.join("data");
        let workspace_root = root.join("workspace");
        let archive_path = root.join("fixture.zip");

        fs::create_dir_all(&workspace_root).expect("create workspace");
        write_zip(&archive_path, &[("settings.json", br#"{ "ok": true }"#)]);

        let mut report_progress = |_stage: &str, _percent: f32, _message: &str| {};
        let is_cancelled = || false;

        run_import_data_archive(
            &data_root,
            &archive_path,
            &workspace_root,
            &mut report_progress,
            &is_cancelled,
        )
        .expect("import archive");

        assert!(
            data_root
                .join("default-user")
                .join("settings.json")
                .is_file(),
            "settings.json should map into default-user"
        );

        cleanup_directory_sync(&root);
    }

    #[test]
    fn import_preserves_unicode_filenames_when_utf8_flag_missing() {
        let root = std::env::temp_dir().join(format!(
            "tauritavern-data-archive-non-utf8-flag-{}",
            rand::random::<u64>()
        ));
        let data_root = root.join("data");
        let workspace_root = root.join("workspace");
        let archive_path = root.join("fixture.zip");

        fs::create_dir_all(&root).expect("create temp root");
        fs::create_dir_all(&workspace_root).expect("create temp workspace");

        let file_name = "data/default-user/worlds/夏瑾 Pro - Beta 天狼星.json";
        let mut bytes =
            write_zip_bytes(&[(file_name, br#"{ "ok": true }"#)], FileOptions::default());
        let patched = clear_zip_utf8_flag(&mut bytes);
        assert!(patched > 0, "should patch zip headers");
        fs::write(&archive_path, bytes).expect("write fixture zip");

        let mut report_progress = |_stage: &str, _percent: f32, _message: &str| {};
        let is_cancelled = || false;

        run_import_data_archive(
            &data_root,
            &archive_path,
            &workspace_root,
            &mut report_progress,
            &is_cancelled,
        )
        .expect("import archive");

        let imported = data_root
            .join("default-user")
            .join("worlds")
            .join("夏瑾 Pro - Beta 天狼星.json");
        assert!(imported.is_file(), "imported file should exist");

        let text = fs::read_to_string(&imported).expect("read imported file");
        assert!(
            text.contains("\"ok\": true"),
            "imported content should match"
        );

        cleanup_directory_sync(&root);
    }

    #[test]
    fn layout_validation_errors_use_utf8_entry_names() {
        let root = std::env::temp_dir().join(format!(
            "tauritavern-data-archive-layout-error-name-{}",
            rand::random::<u64>()
        ));
        let archive_path = root.join("fixture.zip");

        fs::create_dir_all(&root).expect("create temp root");

        let entry_name = "data/default-user/chats/夏瑾 Pro - Beta 天狼星.json";
        let large_payload = vec![0u8; 2 * 1024 * 1024];
        let options = FileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .compression_level(Some(9));
        let mut bytes = write_zip_bytes(&[(entry_name, &large_payload)], options);
        let patched = clear_zip_utf8_flag(&mut bytes);
        assert!(patched > 0, "should patch zip headers");
        fs::write(&archive_path, bytes).expect("write fixture zip");

        let error =
            layout::detect_archive_layout(&archive_path, &|| false).expect_err("scan should fail");
        assert!(
            error.to_string().contains(entry_name),
            "error should reference utf-8 entry name, got: {}",
            error
        );

        cleanup_directory_sync(&root);
    }
}
