mod apply;
mod archive;
mod extract;
mod layout;

use std::fs;
use std::path::Path;

use tt_domain::errors::DomainError;
use tt_domain::models::data_archive::DataArchiveImportFailure;

use super::DataArchiveImportResult;
use super::shared::{
    IMPORT_TARGET_USER_HANDLE, cleanup_directory_sync, ensure_not_cancelled, internal_error,
};

pub(crate) fn run_import_data_archive(
    data_root: &Path,
    archive_path: &Path,
    workspace_root: &Path,
    report_progress: &mut dyn FnMut(&str, f32, &str),
    is_cancelled: &dyn Fn() -> bool,
    prepare_user_data: fn(&Path, &Path) -> Result<(), DomainError>,
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
    let raw_root = workspace_root.join("raw");
    if normalized_root.exists() {
        cleanup_directory_sync(&normalized_root);
    }
    if raw_root.exists() {
        cleanup_directory_sync(&raw_root);
    }
    fs::create_dir_all(&normalized_root)
        .map_err(|error| internal_error("Failed to create normalized workspace", error))?;

    let mut layout_scan = layout::ArchiveLayoutScan::new();
    let archive = archive::prepare_archive_for_import(
        archive_path,
        &raw_root,
        report_progress,
        is_cancelled,
        &mut |path| layout_scan.visit_path(path),
    )?;
    let scanned_archive = archive.scanned_archive();
    let layout = layout_scan.finish(scanned_archive)?;
    ensure_not_cancelled(is_cancelled)?;

    let staged_archive = match archive {
        archive::PreparedArchive::Zip(zip_archive) => {
            report_progress("scanning", 10.0, "Archive layout detected");
            zip_archive.stage(
                &raw_root,
                &|path| {
                    extract::target_relative_path(path, &layout, layout.detected_user_handles())
                        .is_some()
                },
                report_progress,
                is_cancelled,
            )?
        }
        archive::PreparedArchive::Staged(staged_archive) => staged_archive,
    };

    report_progress("normalizing", 90.0, "Normalizing archive layout");
    extract::normalize_staged_archive(&staged_archive, &layout, &normalized_root, is_cancelled)?;
    drop(staged_archive);
    prepare_user_data(
        &normalized_root.join(IMPORT_TARGET_USER_HANDLE),
        &data_root.join(IMPORT_TARGET_USER_HANDLE),
    )?;

    report_progress("applying", 92.0, "Merging data directory");
    ensure_not_cancelled(is_cancelled)?;
    let local_applied = apply::apply_overlay(
        &normalized_root,
        data_root,
        scanned_archive.total_uncompressed_bytes,
        report_progress,
        is_cancelled,
    )?;

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
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
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

    fn import_zip_with_raw_names(
        root: &Path,
        entries: &[(&str, &[u8], &[u8])],
    ) -> std::path::PathBuf {
        let data_root = root.join("data");
        let workspace_root = root.join("workspace");
        let archive_path = root.join("fixture.zip");

        fs::create_dir_all(&workspace_root).expect("create temp workspace");
        let source_entries = entries
            .iter()
            .map(|(placeholder, _, content)| (*placeholder, *content))
            .collect::<Vec<_>>();
        let mut bytes = write_zip_bytes(&source_entries, FileOptions::default());

        for (placeholder, raw_name, _) in entries {
            let placeholder = placeholder.as_bytes();
            assert_eq!(placeholder.len(), raw_name.len());

            let mut replaced = 0;
            let mut offset = 0;
            while let Some(relative) = bytes[offset..]
                .windows(placeholder.len())
                .position(|candidate| candidate == placeholder)
            {
                let start = offset + relative;
                bytes[start..start + raw_name.len()].copy_from_slice(raw_name);
                offset = start + raw_name.len();
                replaced += 1;
            }
            assert_eq!(replaced, 2, "replace local and central ZIP entry names");
        }

        fs::write(&archive_path, bytes).expect("write raw-name zip");
        let mut report_progress = |_stage: &str, _percent: f32, _message: &str| {};
        run_import_data_archive(
            &data_root,
            &archive_path,
            &workspace_root,
            &mut report_progress,
            &|| false,
            |_, _| Ok(()),
        )
        .expect("import raw-name zip");

        data_root
    }

    #[test]
    fn zip_extract_uses_prepared_archive_handle_after_source_is_removed() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let raw_root = temp.path().join("raw");
        let normalized_root = temp.path().join("normalized");
        let archive_path = temp.path().join("fixture.zip");

        fs::create_dir_all(&normalized_root).expect("create normalized root");
        write_zip(
            &archive_path,
            &[("data/default-user/characters/zip.json", b"zip")],
        );

        let mut layout_scan = layout::ArchiveLayoutScan::new();
        let mut report_progress = |_stage: &str, _percent: f32, _message: &str| {};
        let is_cancelled = || false;
        let prepared = archive::prepare_archive_for_import(
            &archive_path,
            &raw_root,
            &mut report_progress,
            &is_cancelled,
            &mut |path| layout_scan.visit_path(path),
        )
        .expect("prepare zip archive");
        let scanned_archive = prepared.scanned_archive();
        let layout = layout_scan.finish(scanned_archive).expect("finish layout");

        fs::remove_file(&archive_path).expect("remove source archive");
        let archive::PreparedArchive::Zip(zip_archive) = prepared else {
            panic!("zip fixture should prepare as zip");
        };
        let staged_archive = zip_archive
            .stage(
                &raw_root,
                &|path| {
                    extract::target_relative_path(path, &layout, layout.detected_user_handles())
                        .is_some()
                },
                &mut report_progress,
                &is_cancelled,
            )
            .expect("extract from prepared zip archive");
        extract::normalize_staged_archive(
            &staged_archive,
            &layout,
            &normalized_root,
            &is_cancelled,
        )
        .expect("normalize staged zip archive");

        assert_eq!(
            fs::read(normalized_root.join("default-user/characters/zip.json"))
                .expect("read normalized zip file"),
            b"zip"
        );
    }

    #[test]
    fn tar_gz_normalizes_from_staged_workspace_after_source_is_removed() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let raw_root = temp.path().join("raw");
        let normalized_root = temp.path().join("normalized");
        let archive_path = temp.path().join("fixture.tar.gz");

        fs::create_dir_all(&normalized_root).expect("create normalized root");
        write_tar_gz(
            &archive_path,
            &[("data/default-user/chats/targz.jsonl", b"targz")],
        );

        let mut layout_scan = layout::ArchiveLayoutScan::new();
        let mut report_progress = |_stage: &str, _percent: f32, _message: &str| {};
        let is_cancelled = || false;
        let prepared = archive::prepare_archive_for_import(
            &archive_path,
            &raw_root,
            &mut report_progress,
            &is_cancelled,
            &mut |path| layout_scan.visit_path(path),
        )
        .expect("prepare tar.gz archive");
        let layout = layout_scan
            .finish(prepared.scanned_archive())
            .expect("finish layout");

        fs::remove_file(&archive_path).expect("remove source archive");
        let archive::PreparedArchive::Staged(staged_archive) = &prepared else {
            panic!("tar.gz fixture should prepare as staged archive");
        };
        extract::normalize_staged_archive(staged_archive, &layout, &normalized_root, &is_cancelled)
            .expect("normalize staged tar.gz archive");

        assert_eq!(
            fs::read(normalized_root.join("default-user/chats/targz.jsonl"))
                .expect("read normalized tar.gz file"),
            b"targz"
        );
    }

    #[test]
    fn tar_gz_import_preserves_archive_order_for_target_conflicts() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let data_root = temp.path().join("data");
        let workspace_root = temp.path().join("workspace");
        let archive_path = temp.path().join("fixture.tar.gz");

        fs::create_dir_all(&workspace_root).expect("create workspace");
        write_tar_gz(
            &archive_path,
            &[
                ("alice/characters/a.json", b"first"),
                ("bob/characters/a.json", b"second"),
                ("alice/characters/a.json", b"third"),
            ],
        );

        let mut report_progress = |_stage: &str, _percent: f32, _message: &str| {};
        run_import_data_archive(
            &data_root,
            &archive_path,
            &workspace_root,
            &mut report_progress,
            &|| false,
            |_, _| Ok(()),
        )
        .expect("import tar.gz archive");

        assert_eq!(
            fs::read(data_root.join("default-user/characters/a.json"))
                .expect("read imported conflict target"),
            b"third"
        );
    }

    #[test]
    fn zip_import_preserves_archive_order_for_target_conflicts() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let data_root = temp.path().join("data");
        let workspace_root = temp.path().join("workspace");
        let archive_path = temp.path().join("fixture.zip");

        fs::create_dir_all(&workspace_root).expect("create workspace");
        write_zip(
            &archive_path,
            &[
                ("alice/characters/a.json", b"first"),
                ("bob/characters/a.json", b"second"),
                ("carol/characters/a.json", b"third"),
            ],
        );

        let mut report_progress = |_stage: &str, _percent: f32, _message: &str| {};
        run_import_data_archive(
            &data_root,
            &archive_path,
            &workspace_root,
            &mut report_progress,
            &|| false,
            |_, _| Ok(()),
        )
        .expect("import zip archive");

        assert_eq!(
            fs::read(data_root.join("default-user/characters/a.json"))
                .expect("read imported conflict target"),
            b"third"
        );
    }

    #[test]
    fn cancelling_parallel_zip_staging_does_not_apply_partial_data() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let data_root = temp.path().join("data");
        let workspace_root = temp.path().join("workspace");
        let archive_path = temp.path().join("fixture.zip");
        let payload = vec![b'x'; crate::data_archive::shared::COPY_BUFFER_BYTES * 3];

        fs::create_dir_all(&workspace_root).expect("create workspace");
        fs::write(
            &archive_path,
            write_zip_bytes(
                &[
                    ("data/default-user/characters/first.bin", payload.as_slice()),
                    (
                        "data/default-user/characters/second.bin",
                        payload.as_slice(),
                    ),
                ],
                FileOptions::default().compression_method(CompressionMethod::Stored),
            ),
        )
        .expect("write zip");

        let cancelled = AtomicBool::new(false);
        let mut report_progress = |stage: &str, percent: f32, _message: &str| {
            if stage == "extracting" && percent > 15.0 {
                cancelled.store(true, Ordering::SeqCst);
            }
        };
        let is_cancelled = || cancelled.load(Ordering::SeqCst);
        let failure = run_import_data_archive(
            &data_root,
            &archive_path,
            &workspace_root,
            &mut report_progress,
            &is_cancelled,
            |_, _| Ok(()),
        )
        .expect_err("cancelled import should fail");

        assert!(
            matches!(&failure.error, DomainError::Cancelled(_)),
            "unexpected failure: {:?}",
            failure.error
        );
        assert!(!data_root.exists());
    }

    #[test]
    fn import_preserves_unicode_filenames() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let data_root = temp.path().join("data");
        let workspace_root = temp.path().join("workspace");
        let archive_path = temp.path().join("fixture.zip");

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
            |_, _| Ok(()),
        )
        .expect("import archive");

        let imported = data_root
            .join("default-user")
            .join("characters")
            .join("中文.json");
        assert!(imported.is_file(), "imported file should exist");

        let text = fs::read_to_string(&imported).expect("read imported file");
        assert!(text.contains("中文"), "imported content should match");
    }

    #[test]
    fn import_decodes_legacy_gb18030_zip_filenames_as_one_archive() {
        let entries: &[(&str, &[u8], &[u8])] = &[
            (
                "data/default-user/characters/abcdefgh.json",
                b"data/default-user/characters/\xd6\xd0\xce\xc4\xc3\xfb\xd7\xd6.json",
                b"first",
            ),
            (
                "data/default-user/characters/ij.json",
                b"data/default-user/characters/\xc2\xa5.json",
                b"second",
            ),
        ];
        let temp = tempfile::tempdir().expect("create temp dir");
        let data_root = import_zip_with_raw_names(temp.path(), entries);

        assert_eq!(
            fs::read(
                data_root
                    .join("default-user/characters")
                    .join("中文名字.json")
            )
            .expect("read GB18030 filename"),
            b"first"
        );
        assert_eq!(
            fs::read(data_root.join("default-user/characters/楼.json"))
                .expect("read ambiguous GB18030 filename"),
            b"second"
        );
    }

    #[test]
    fn import_detects_big5_zip_filenames() {
        let names = ["繁體中文設定", "聊天紀錄", "角色卡片", "世界書", "夏瑾"];
        let raw_names = names.map(|name| {
            let path = format!("data/default-user/characters/{name}.json");
            let (bytes, _, unmappable) = encoding_rs::BIG5.encode(&path);
            assert!(!unmappable, "{name} is Big5");
            bytes.into_owned()
        });
        let placeholders = raw_names
            .iter()
            .enumerate()
            .map(|(index, raw_name)| {
                let prefix = format!("data/default-user/characters/{index}");
                format!("{prefix}{}", "x".repeat(raw_name.len() - prefix.len()))
            })
            .collect::<Vec<_>>();
        let entries = placeholders
            .iter()
            .zip(&raw_names)
            .map(|(placeholder, raw_name)| {
                (placeholder.as_str(), raw_name.as_slice(), &b"big5"[..])
            })
            .collect::<Vec<_>>();
        let temp = tempfile::tempdir().expect("create temp dir");
        let data_root = import_zip_with_raw_names(temp.path(), &entries);

        for name in names {
            assert!(
                data_root
                    .join("default-user/characters")
                    .join(format!("{name}.json"))
                    .is_file(),
                "{name}"
            );
        }
    }

    #[test]
    fn import_preserves_unflagged_utf8_zip_filenames() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let data_root = import_zip_with_raw_names(
            temp.path(),
            &[(
                "data/default-user/characters/abcdef.json",
                "data/default-user/characters/夏瑾.json".as_bytes(),
                b"utf8",
            )],
        );

        assert_eq!(
            fs::read(data_root.join("default-user/characters/夏瑾.json"))
                .expect("read UTF-8 filename"),
            b"utf8"
        );
    }

    #[test]
    fn import_preserves_cp437_zip_filenames_without_gb18030_evidence() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let data_root = import_zip_with_raw_names(
            temp.path(),
            &[(
                "data/default-user/characters/abcde.json",
                b"data/default-user/characters/\x82cole.json",
                b"cp437",
            )],
        );

        assert_eq!(
            fs::read(data_root.join("default-user/characters/école.json"))
                .expect("read CP437 filename"),
            b"cp437"
        );
    }

    #[test]
    fn import_is_incremental_overlay() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let data_root = temp.path().join("data");
        let workspace_root = temp.path().join("workspace");
        let archive_path = temp.path().join("fixture.zip");

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
            |_, _| Ok(()),
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
    }

    #[test]
    fn import_detects_tar_gz_by_content_not_extension() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let data_root = temp.path().join("data");
        let workspace_root = temp.path().join("workspace");
        let archive_path = temp.path().join("fixture.zip");

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
            |_, _| Ok(()),
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
    }

    #[test]
    fn tar_import_rejects_path_escape() {
        for escape in ["../escape.json", "data/c:/escape.json"] {
            let temp = tempfile::tempdir().expect("create temp dir");
            let data_root = temp.path().join("data");
            let workspace_root = temp.path().join("workspace");
            let archive_path = temp.path().join("fixture.tar");

            fs::create_dir_all(&workspace_root).expect("create temp workspace");
            write_raw_tar_file(&archive_path, escape, b"bad");

            let mut report_progress = |_stage: &str, _percent: f32, _message: &str| {};
            let is_cancelled = || false;

            let error = run_import_data_archive(
                &data_root,
                &archive_path,
                &workspace_root,
                &mut report_progress,
                &is_cancelled,
                |_, _| Ok(()),
            )
            .expect_err("path escape should be rejected");
            assert!(
                matches!(&error.error, DomainError::InvalidData(message)
                    if message.starts_with("Invalid archive entry path:")),
                "{escape}: {:?}",
                error.error
            );
        }
    }

    #[test]
    fn import_rejects_malformed_archive_as_invalid_data() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let data_root = temp.path().join("data");
        let workspace_root = temp.path().join("workspace");
        let archive_path = temp.path().join("fixture.archive");

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
            |_, _| Ok(()),
        )
        .expect_err("malformed archive should be rejected");
        assert!(
            matches!(error.error, DomainError::InvalidData(_)),
            "malformed archive should be invalid data, got: {}",
            error.error
        );
    }

    #[test]
    fn import_rejects_pk_prefixed_malformed_zip_without_tar_fallback() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let data_root = temp.path().join("data");
        let workspace_root = temp.path().join("workspace");
        let archive_path = temp.path().join("fixture.zip");

        fs::create_dir_all(&workspace_root).expect("create temp workspace");
        fs::write(&archive_path, b"PKnot a valid zip archive").expect("write archive");

        let mut report_progress = |_stage: &str, _percent: f32, _message: &str| {};
        let is_cancelled = || false;

        let error = run_import_data_archive(
            &data_root,
            &archive_path,
            &workspace_root,
            &mut report_progress,
            &is_cancelled,
            |_, _| Ok(()),
        )
        .expect_err("malformed zip should be rejected");
        assert!(matches!(error.error, DomainError::InvalidData(_)));
        assert!(
            error
                .error
                .to_string()
                .contains("Failed to parse zip archive"),
            "PK-prefixed malformed archive should not fall back to tar, got: {}",
            error.error
        );
    }

    #[test]
    fn tar_scan_reports_cancelled_errors_as_cancelled() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let archive_path = temp.path().join("fixture.tar");
        let large_payload = vec![0u8; 2 * 1024 * 1024];

        write_tar(
            &archive_path,
            &[("data/default-user/chats/large.jsonl", &large_payload)],
        );

        let checks = AtomicUsize::new(0);
        let is_cancelled = || checks.fetch_add(1, Ordering::SeqCst) >= 2;

        let mut layout_scan = layout::ArchiveLayoutScan::new();
        let mut report_progress = |_stage: &str, _percent: f32, _message: &str| {};
        let error = match archive::prepare_archive_for_import(
            &archive_path,
            &temp.path().join("raw"),
            &mut report_progress,
            &is_cancelled,
            &mut |path| layout_scan.visit_path(path),
        ) {
            Ok(_) => panic!("cancelled scan should fail"),
            Err(error) => error,
        };
        assert!(
            matches!(error, DomainError::Cancelled(_)),
            "cancelled scan should stay cancelled, got: {}",
            error
        );
    }

    #[test]
    fn tar_gz_import_rejects_symlinks() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let data_root = temp.path().join("data");
        let workspace_root = temp.path().join("workspace");
        let archive_path = temp.path().join("fixture.tgz");

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
            |_, _| Ok(()),
        )
        .expect_err("symlink should be rejected");
        assert!(matches!(error.error, DomainError::InvalidData(_)));
    }

    #[test]
    fn import_supports_sillytavern_user_root_layout() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let data_root = temp.path().join("data");
        let workspace_root = temp.path().join("workspace");
        let archive_path = temp.path().join("fixture.zip");

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
            |_, _| Ok(()),
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
    }

    #[test]
    fn import_supports_sillytavern_native_user_backup_layout() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let data_root = temp.path().join("data");
        let workspace_root = temp.path().join("workspace");
        let archive_path = temp.path().join("fixture.zip");

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
                ("extensions/SomeExtension/assets/icon.png", b"icon"),
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
            |_, _| Ok(()),
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
        assert!(
            data_root
                .join("default-user")
                .join("extensions")
                .join("SomeExtension")
                .join("assets")
                .join("icon.png")
                .is_file(),
            "extension content paths should remain user-root content"
        );
    }

    #[test]
    fn import_supports_settings_single_file() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let data_root = temp.path().join("data");
        let workspace_root = temp.path().join("workspace");
        let archive_path = temp.path().join("fixture.zip");

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
            |_, _| Ok(()),
        )
        .expect("import archive");

        assert!(
            data_root
                .join("default-user")
                .join("settings.json")
                .is_file(),
            "settings.json should map into default-user"
        );
    }
}
