use std::borrow::Cow;
use std::collections::BTreeSet;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};

use chardetng::{EncodingDetector, Iso2022JpDetection, Utf8Detection};
use encoding_rs::{BIG5, EUC_KR, Encoding, GB18030, SHIFT_JIS};
use typed_path::{Utf8WindowsComponent, Utf8WindowsPath};
use zip::read::ZipFileEntry;
use zip::write::SimpleFileOptions as FileOptions;
use zip::{CompressionMethod, ZipArchive};

use tt_domain::errors::DomainError;

const DEFLATE_TEXT_COMPRESSION_LEVEL: i64 = 1;
const DEFLATE_TEXT_EXTENSIONS: &[&str] = &[
    "json", "jsonl", "txt", "md", "csv", "html", "css", "js", "yaml", "yml", "log", "sse",
];
const UTF8_NAME_FLAG: u16 = 1 << 11;
const MIN_DETECTION_WORDS: usize = 5;

#[derive(Clone, Copy)]
pub(crate) enum ZipEntryNameEncoding {
    Utf8OrCp437,
    Legacy(&'static Encoding),
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

pub(crate) fn detect_zip_entry_name_encoding<R: Read + Seek>(
    archive: &ZipArchive<R>,
) -> ZipEntryNameEncoding {
    let legacy_names = || {
        archive
            .file_names_raw()
            .filter(|name| std::str::from_utf8(name).is_err())
    };
    if !legacy_names()
        .flatten()
        .any(|byte| (0xB0..=0xDF).contains(byte))
    {
        return ZipEntryNameEncoding::Utf8OrCp437;
    }

    let words: BTreeSet<&[u8]> = legacy_names()
        .flat_map(|name| name.split(|byte| *byte < 0x40))
        .filter(|word| !word.is_ascii())
        .collect();
    let encoding = if words.len() < MIN_DETECTION_WORDS
        && legacy_names().all(|name| {
            GB18030
                .decode_without_bom_handling_and_without_replacement(name)
                .is_some()
        }) {
        GB18030
    } else {
        detect_cjk_encoding(&words)
    };
    tracing::info!(
        encoding = encoding.name(),
        "Detected legacy ZIP entry names"
    );
    ZipEntryNameEncoding::Legacy(encoding)
}

pub(crate) fn enclosed_zip_entry_path_with_encoding<'a>(
    entry: &'a ZipFileEntry<'_>,
    encoding: ZipEntryNameEncoding,
) -> Result<(PathBuf, Cow<'a, str>), DomainError> {
    let name = zip_entry_display_name(entry, encoding)?;
    let path = enclosed_archive_entry_path(&name)?;
    Ok((path, name))
}

pub(crate) fn enclosed_archive_entry_path(name: &str) -> Result<PathBuf, DomainError> {
    enclosed_name_from_str(name)
        .ok_or_else(|| DomainError::InvalidData(format!("Invalid archive entry path: {}", name)))
}

fn detect_cjk_encoding(words: &BTreeSet<&[u8]>) -> &'static Encoding {
    let mut detector = EncodingDetector::new(Iso2022JpDetection::Deny);
    for word in words {
        detector.feed(word, false);
        detector.feed(b"\n", false);
    }
    detector.feed(&[], true);
    let guess = detector.guess(None, Utf8Detection::Deny);
    if [BIG5, SHIFT_JIS, EUC_KR].contains(&guess) {
        guess
    } else {
        GB18030
    }
}

fn zip_entry_display_name<'a>(
    entry: &'a ZipFileEntry<'_>,
    encoding: ZipEntryNameEncoding,
) -> Result<Cow<'a, str>, DomainError> {
    match encoding {
        ZipEntryNameEncoding::Legacy(legacy) if !declares_unicode_name(entry) => legacy
            .decode_without_bom_handling_and_without_replacement(entry.name_raw())
            .ok_or_else(|| {
                DomainError::InvalidData(format!(
                    "Invalid {} archive entry name: {}",
                    legacy.name(),
                    entry.name_raw().escape_ascii()
                ))
            }),
        _ => entry.name().map_err(|error| {
            DomainError::InvalidData(format!("Invalid archive entry name: {error}"))
        }),
    }
}

// zip also sets the UTF-8 flag when it applies a Unicode Path extra field.
fn declares_unicode_name(entry: &ZipFileEntry<'_>) -> bool {
    entry.flags().as_u16() & UTF8_NAME_FLAG != 0 && std::str::from_utf8(entry.name_raw()).is_ok()
}

// Custom-decoded paths must follow the same rules as `ZipFile::enclosed_name`.
fn enclosed_name_from_str(name: &str) -> Option<PathBuf> {
    if name.contains('\0') {
        return None;
    }

    let mut depth = 0usize;
    let mut out_path = PathBuf::new();
    for component in windows_components(name) {
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
                // Inside a verbatim path `/` is not a separator, but it splits the segment on push.
                if segment.contains(['/', '\\']) {
                    return None;
                }
                #[cfg(windows)]
                if is_windows_invalid_component(segment) {
                    return None;
                }
                depth += 1;
                out_path.push(segment);
            }
            Utf8WindowsComponent::CurDir => (),
        }
    }

    Some(out_path)
}

fn windows_components(name: &str) -> impl Iterator<Item = Utf8WindowsComponent<'_>> {
    Utf8WindowsPath::new(name)
        .components()
        .flat_map(|component| match component {
            Utf8WindowsComponent::Normal(segment) => split_drive_prefix(segment),
            component => [Some(component), None],
        })
        .flatten()
}

fn split_drive_prefix(segment: &str) -> [Option<Utf8WindowsComponent<'_>>; 2] {
    let mut rest = segment;
    while let [drive, b':', ..] = rest.as_bytes()
        && drive.is_ascii_alphabetic()
    {
        rest = &rest[2..];
    }
    let prefix = Utf8WindowsPath::new(&segment[..segment.len() - rest.len()])
        .components()
        .next();
    let rest = match rest {
        "" => None,
        "." => Some(Utf8WindowsComponent::CurDir),
        ".." => Some(Utf8WindowsComponent::ParentDir),
        rest => Some(Utf8WindowsComponent::Normal(rest)),
    };
    [prefix, rest]
}

#[cfg(windows)]
fn is_windows_invalid_component(segment: &str) -> bool {
    segment.is_empty()
        || segment.encode_utf16().count() > 255
        || segment.contains(['<', '>', ':', '"', '/', '\\', '|', '?', '*'])
        || segment
            .chars()
            .any(|character| character.is_ascii_control())
        || segment.ends_with(['.', ' '])
        || is_windows_reserved_device_name(segment)
}

#[cfg(windows)]
fn is_windows_reserved_device_name(segment: &str) -> bool {
    let Some(stem) = segment.split(['.', ':']).next() else {
        return false;
    };
    let stem = stem.trim_end_matches([' ', '.']).to_ascii_uppercase();
    if matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$" | "CLOCK$" | "CONFIG$"
    ) {
        return true;
    }
    let mut chars = stem.chars();
    let prefix: String = chars.by_ref().take(3).collect();
    matches!(prefix.as_str(), "COM" | "LPT")
        && matches!(
            (chars.next(), chars.next()),
            (Some('1'..='9' | '¹' | '²' | '³' | '⁴'..='⁹'), None)
        )
}
