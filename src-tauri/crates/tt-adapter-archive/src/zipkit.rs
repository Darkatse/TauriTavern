use std::borrow::Cow;
use std::io::Read;
use std::path::{Path, PathBuf};

use encoding_rs::GB18030;
use typed_path::{Utf8WindowsComponent, Utf8WindowsPath};
use zip::CompressionMethod;
use zip::read::{HasZipMetadata, ZipFile};
use zip::write::SimpleFileOptions as FileOptions;

use tt_domain::errors::DomainError;

const DEFLATE_TEXT_COMPRESSION_LEVEL: i64 = 1;
const DEFLATE_TEXT_EXTENSIONS: &[&str] = &[
    "json", "jsonl", "txt", "md", "csv", "html", "css", "js", "yaml", "yml", "log", "sse",
];

#[derive(Clone, Copy)]
pub(crate) enum ZipEntryNameEncoding {
    Utf8OrCp437,
    Gb18030,
}

pub(crate) fn export_file_options(path: impl AsRef<Path>) -> FileOptions {
    let path = path.as_ref();
    let ext = path.extension().and_then(|ext| ext.to_str());
    if let Some(ext) = ext
        && DEFLATE_TEXT_EXTENSIONS
            .iter()
            .any(|candidate| ext.eq_ignore_ascii_case(candidate))
    {
        return FileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .compression_level(Some(DEFLATE_TEXT_COMPRESSION_LEVEL))
            .unix_permissions(0o644);
    }

    FileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .unix_permissions(0o644)
}

pub(crate) fn enclosed_zip_entry_path_with_encoding<'a, 'b, R: Read + ?Sized>(
    entry: &'b ZipFile<'a, R>,
    encoding: ZipEntryNameEncoding,
) -> Result<(PathBuf, Cow<'b, str>), DomainError> {
    let name = zip_entry_display_name(entry, encoding)?;
    let path = enclosed_archive_entry_path(&name)?;
    Ok((path, name))
}

pub(crate) fn has_cp437_box_or_block(name: &str) -> bool {
    name.chars()
        .any(|character| matches!(character, '\u{2500}'..='\u{259f}'))
}

pub(crate) fn decodes_as_legacy_gb18030_cjk<R: Read + ?Sized>(entry: &ZipFile<'_, R>) -> bool {
    if entry.get_metadata().is_utf8 || std::str::from_utf8(entry.name_raw()).is_ok() {
        return false;
    }

    GB18030
        .decode_without_bom_handling_and_without_replacement(entry.name_raw())
        .is_some_and(|name| contains_cjk(&name))
}

pub(crate) fn enclosed_archive_entry_path(name: &str) -> Result<PathBuf, DomainError> {
    enclosed_name_from_str(name)
        .ok_or_else(|| DomainError::InvalidData(format!("Invalid archive entry path: {}", name)))
}

fn zip_entry_display_name<'a, 'b, R: Read + ?Sized>(
    entry: &'b ZipFile<'a, R>,
    encoding: ZipEntryNameEncoding,
) -> Result<Cow<'b, str>, DomainError> {
    let raw_name = entry.name_raw();
    if raw_name.contains(&0) {
        return Err(DomainError::InvalidData(format!(
            "Invalid archive entry path (NUL byte): {}",
            entry.name()
        )));
    }

    if entry.get_metadata().is_utf8 {
        return Ok(Cow::Borrowed(entry.name()));
    }

    match encoding {
        ZipEntryNameEncoding::Utf8OrCp437 => Ok(std::str::from_utf8(raw_name)
            .map(Cow::Borrowed)
            .unwrap_or_else(|_| Cow::Borrowed(entry.name()))),
        ZipEntryNameEncoding::Gb18030 => GB18030
            .decode_without_bom_handling_and_without_replacement(raw_name)
            .ok_or_else(|| {
                DomainError::InvalidData(format!(
                    "Invalid GB18030 archive entry name: {}",
                    entry.name()
                ))
            }),
    }
}

fn contains_cjk(name: &str) -> bool {
    name.chars().any(|character| {
        matches!(
            character,
            '\u{3400}'..='\u{4dbf}'
                | '\u{4e00}'..='\u{9fff}'
                | '\u{f900}'..='\u{faff}'
                | '\u{20000}'..='\u{2fa1f}'
                | '\u{30000}'..='\u{323af}'
        )
    })
}

fn enclosed_name_from_str(name: &str) -> Option<PathBuf> {
    if name.contains('\0') {
        return None;
    }

    let mut depth = 0usize;
    let mut out_path = PathBuf::new();
    for component in Utf8WindowsPath::new(name).components() {
        match component {
            Utf8WindowsComponent::Prefix(_) | Utf8WindowsComponent::RootDir => {
                if depth > 0 {
                    return None;
                }
            }
            Utf8WindowsComponent::ParentDir => {
                depth = depth.checked_sub(1)?;
                out_path.pop();
            }
            Utf8WindowsComponent::Normal(segment) => {
                depth += 1;
                out_path.push(segment);
            }
            Utf8WindowsComponent::CurDir => (),
        }
    }

    Some(out_path)
}
