use std::path::Path;

use zip::CompressionMethod;
use zip::write::SimpleFileOptions as FileOptions;

pub const DEFLATE_TEXT_COMPRESSION_LEVEL: i64 = 1;
pub const DEFLATE_TEXT_EXTENSIONS: &[&str] = &[
    "json", "jsonl", "txt", "md", "csv", "html", "css", "js", "yaml", "yml", "log", "sse",
];

pub fn export_file_options(path: impl AsRef<Path>) -> FileOptions {
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
