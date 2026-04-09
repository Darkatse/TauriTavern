use std::io::Read;
use std::path::{Path, PathBuf};

use typed_path::{Utf8WindowsComponent, Utf8WindowsPath};
use zip::read::ZipFile;
use zip::write::SimpleFileOptions as FileOptions;
use zip::CompressionMethod;

use crate::domain::errors::DomainError;

pub const DEFLATE_TEXT_COMPRESSION_LEVEL: i64 = 1;
pub const DEFLATE_TEXT_EXTENSIONS: &[&str] = &[
    "json", "jsonl", "txt", "md", "csv", "html", "css", "js", "yaml", "yml", "log", "sse",
];

pub fn export_file_options(path: impl AsRef<Path>) -> FileOptions {
    let path = path.as_ref();
    let ext = path.extension().and_then(|ext| ext.to_str());
    if let Some(ext) = ext {
        if DEFLATE_TEXT_EXTENSIONS
            .iter()
            .any(|candidate| ext.eq_ignore_ascii_case(candidate))
        {
            return FileOptions::default()
                .compression_method(CompressionMethod::Deflated)
                .compression_level(Some(DEFLATE_TEXT_COMPRESSION_LEVEL))
                .unix_permissions(0o644);
        }
    }

    FileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .unix_permissions(0o644)
}

pub fn enclosed_zip_entry_path<R: Read + ?Sized>(
    entry: &ZipFile<'_, R>,
) -> Result<PathBuf, DomainError> {
    Ok(enclosed_zip_entry_path_with_name(entry)?.0)
}

pub fn enclosed_zip_entry_path_with_name<'a, 'b, R: Read + ?Sized>(
    entry: &'b ZipFile<'a, R>,
) -> Result<(PathBuf, &'b str), DomainError> {
    let name = zip_entry_display_name(entry)?;
    let path = enclosed_name_from_str(name).ok_or_else(|| {
        DomainError::InvalidData(format!("Invalid archive entry path: {}", name))
    })?;
    Ok((path, name))
}

pub fn zip_entry_display_name<'a, 'b, R: Read + ?Sized>(
    entry: &'b ZipFile<'a, R>,
) -> Result<&'b str, DomainError> {
    let raw_name = entry.name_raw();
    if raw_name.contains(&0) {
        return Err(DomainError::InvalidData(format!(
            "Invalid archive entry path (NUL byte): {}",
            entry.name()
        )));
    }

    // Some ZIP writers store UTF-8 names without setting the UTF-8 flag.
    // In that case the zip crate decodes `entry.name()` as CP437, producing mojibake
    // (e.g. Chinese becomes `σñÅ...`). Prefer strict UTF-8 when possible.
    Ok(std::str::from_utf8(raw_name).unwrap_or_else(|_| entry.name()))
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
