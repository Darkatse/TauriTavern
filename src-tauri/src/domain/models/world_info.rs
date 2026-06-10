use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::ffi::OsStr;
use std::path::Path;

use crate::domain::models::filename::sanitize_filename;

pub const WORLD_INFO_EXTENSION: &str = "json";

/// World Info (Lorebook) document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldInfo {
    /// Logical lorebook name (filename stem).
    pub name: String,
    /// Raw lorebook payload.
    pub data: Value,
}

impl WorldInfo {
    pub fn new(name: String, data: Value) -> Self {
        Self { name, data }
    }

    pub fn file_stem(&self) -> String {
        sanitize_world_info_name(&self.name)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.file_stem().is_empty() {
            return Err("World file must have a name".to_string());
        }

        validate_world_info_data(&self.data)
    }
}

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

/// Validate lorebook payload.
pub fn validate_world_info_data(data: &Value) -> Result<(), String> {
    if !data.is_object() {
        return Err("Is not a valid world info file".to_string());
    }

    if data.get("entries").is_none() {
        return Err("World info must contain an entries list".to_string());
    }

    Ok(())
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
