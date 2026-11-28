use std::borrow::Cow;
use std::io::Read;
use std::path::{Path, PathBuf};

use zip::CompressionMethod;
use zip::read::ZipFile;
use zip::write::SimpleFileOptions as FileOptions;

use tt_domain::errors::DomainError;

const DEFLATE_TEXT_COMPRESSION_LEVEL: i64 = 1;
const DEFLATE_TEXT_EXTENSIONS: &[&str] = &[
    "json", "jsonl", "txt", "md", "csv", "html", "css", "js", "yaml", "yml", "log", "sse",
];

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

pub(crate) fn enclosed_zip_entry_path_with_name<'a, R: Read + ?Sized>(
    entry: &'a ZipFile<'_, R>,
) -> Result<(PathBuf, Cow<'a, str>), DomainError> {
    let name = entry.name().map_err(|error| {
        DomainError::InvalidData(format!("Invalid archive entry name: {error}"))
    })?;
    let path = entry
        .enclosed_name()
        .ok_or_else(|| DomainError::InvalidData(format!("Invalid archive entry path: {name}")))?;
    Ok((path, name))
}
