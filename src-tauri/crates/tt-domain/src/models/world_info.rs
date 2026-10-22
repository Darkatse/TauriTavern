use std::ffi::OsStr;
use std::path::Path;

use crate::models::filename::sanitize_filename;

pub const WORLD_INFO_EXTENSION: &str = "json";

/// Sanitize a world info logical name as SillyTavern does: sanitize the full
/// filename (`name.json`), then expose the filename stem as the logical name.
pub fn sanitize_world_info_name(name: &str) -> String {
    let filename = sanitize_world_info_file_name(name);
    if Path::new(&filename)
        .extension()
        .and_then(OsStr::to_str)
        .is_none_or(|ext| !ext.eq_ignore_ascii_case(WORLD_INFO_EXTENSION))
    {
        return String::new();
    }

    Path::new(&filename)
        .file_stem()
        .and_then(OsStr::to_str)
        .unwrap_or_default()
        .to_string()
}

/// Sanitize a world info logical name into the on-disk JSON filename.
pub fn sanitize_world_info_file_name(name: &str) -> String {
    sanitize_filename(&format!("{name}.{WORLD_INFO_EXTENSION}"))
}

/// Sanitize an imported world info original filename into the committed logical name.
pub fn sanitize_world_info_import_name(original_filename: &str) -> String {
    let sanitized_filename = sanitize_filename(original_filename);
    let file_stem = Path::new(&sanitized_filename)
        .file_stem()
        .and_then(OsStr::to_str)
        .unwrap_or_default();

    sanitize_world_info_name(file_stem)
}

#[cfg(test)]
mod tests {
    use super::{
        sanitize_world_info_file_name, sanitize_world_info_import_name, sanitize_world_info_name,
    };

    #[test]
    fn sanitize_world_info_name_preserves_upstream_significant_whitespace_and_dots() {
        assert_eq!(sanitize_world_info_name(" Lore"), " Lore");
        assert_eq!(sanitize_world_info_name("Lore "), "Lore ");
        assert_eq!(sanitize_world_info_name(".Lore"), ".Lore");
        assert_eq!(sanitize_world_info_name("Lore."), "Lore.");
        assert_eq!(sanitize_world_info_name("  "), "  ");
    }

    #[test]
    fn sanitize_world_info_name_matches_upstream_full_filename_sanitization() {
        assert_eq!(sanitize_world_info_file_name("a:b*c?"), "abc.json");
        assert_eq!(sanitize_world_info_name("a:b*c?"), "abc");
        assert_eq!(sanitize_world_info_name("CON"), "");
        assert_eq!(sanitize_world_info_name("CON "), "CON ");
    }

    #[test]
    fn sanitize_world_info_import_name_uses_original_filename_contract() {
        assert_eq!(sanitize_world_info_import_name(" Lore.json"), " Lore");
        assert_eq!(sanitize_world_info_import_name("Lore .json"), "Lore ");
        assert_eq!(sanitize_world_info_import_name("a:b*c?.json"), "abc");
        assert_eq!(
            sanitize_world_info_import_name("file.name.with.dots.json"),
            "file.name.with.dots"
        );
    }
}
