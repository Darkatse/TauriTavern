use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::Value;
use tokio::fs;

use crate::domain::errors::DomainError;
use crate::infrastructure::persistence::file_system::list_files_with_extension;

const INVALID_FILE_CHARS: [char; 9] = ['/', '\\', ':', '*', '?', '"', '<', '>', '|'];
const WINDOWS_RESERVED_NAMES: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

#[derive(Debug, Clone)]
pub(crate) struct NamedPresetFile {
    #[cfg(test)]
    pub(crate) path: PathBuf,
    pub(crate) name: String,
    pub(crate) raw_content: String,
    pub(crate) is_canonical: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct PresetFilePaths {
    logical_name: String,
    canonical: PathBuf,
    legacy: PathBuf,
}

impl PresetFilePaths {
    pub(crate) fn new(
        logical_name: &str,
        directory: &Path,
        extension: &str,
    ) -> Result<Self, DomainError> {
        let canonical_stem = canonical_preset_file_stem(logical_name)?;
        #[allow(deprecated)]
        let legacy_stem = legacy_preset_file_stem(logical_name);

        Ok(Self {
            logical_name: logical_name.to_string(),
            canonical: directory.join(format!("{canonical_stem}{extension}")),
            legacy: directory.join(format!("{legacy_stem}{extension}")),
        })
    }

    #[cfg(test)]
    pub(crate) fn canonical_path(&self) -> &Path {
        &self.canonical
    }

    #[cfg(test)]
    #[deprecated(
        note = "Legacy preset filename compatibility path. Remove after migration window."
    )]
    pub(crate) fn legacy_path(&self) -> &Path {
        &self.legacy
    }

    pub(crate) fn resolve_existing(&self) -> Result<Option<PathBuf>, DomainError> {
        let canonical_exists = self.canonical.exists();
        let legacy_exists = self.legacy != self.canonical && self.legacy.exists();

        match (canonical_exists, legacy_exists) {
            (true, true) => Err(DomainError::InvalidData(format!(
                "Conflicting preset files exist for '{}': '{}' and '{}'",
                self.logical_name,
                self.canonical.display(),
                self.legacy.display()
            ))),
            (true, false) => Ok(Some(self.canonical.clone())),
            (false, true) => Ok(Some(self.legacy.clone())),
            (false, false) => Ok(None),
        }
    }

    pub(crate) async fn prepare_for_save(&self) -> Result<PathBuf, DomainError> {
        let canonical_exists = self.canonical.exists();
        let legacy_exists = self.legacy != self.canonical && self.legacy.exists();

        match (canonical_exists, legacy_exists) {
            (true, true) => Err(DomainError::InvalidData(format!(
                "Conflicting preset files exist for '{}': '{}' and '{}'",
                self.logical_name,
                self.canonical.display(),
                self.legacy.display()
            ))),
            (true, false) | (false, false) => Ok(self.canonical.clone()),
            (false, true) => {
                fs::rename(&self.legacy, &self.canonical)
                    .await
                    .map_err(|error| {
                        DomainError::InternalError(format!(
                            "Failed to migrate deprecated preset path '{}' -> '{}': {}",
                            self.legacy.display(),
                            self.canonical.display(),
                            error
                        ))
                    })?;
                Ok(self.canonical.clone())
            }
        }
    }
}

pub(crate) fn canonical_preset_file_stem(logical_name: &str) -> Result<String, DomainError> {
    let sanitized = logical_name
        .chars()
        .filter(|character| !INVALID_FILE_CHARS.contains(character) && !character.is_control())
        .collect::<String>();
    let sanitized = sanitized.trim_end_matches(['.', ' ']).to_string();

    if sanitized.is_empty() || is_windows_reserved_name(&sanitized) {
        return Err(DomainError::InvalidData(format!(
            "Preset name is invalid for filesystem storage: '{}'",
            logical_name
        )));
    }

    Ok(sanitized)
}

#[deprecated(note = "Legacy preset filename compatibility path. Remove after migration window.")]
pub(crate) fn legacy_preset_file_stem(logical_name: &str) -> String {
    logical_name
        .chars()
        .map(|character| match character {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            ch if ch.is_alphanumeric() || ch == '-' || ch == '_' || ch == '.' || ch == ' ' => ch,
            _ => '_',
        })
        .collect::<String>()
        .trim()
        .to_string()
}

pub(crate) async fn load_named_preset_files(
    dir: &Path,
) -> Result<Vec<NamedPresetFile>, DomainError> {
    let mut files = list_files_with_extension(dir, "json").await?;
    files.sort();

    let mut entries: Vec<NamedPresetFile> = Vec::new();
    let mut indices = HashMap::<String, usize>::new();

    for path in files {
        let file_stem = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .ok_or_else(|| {
                DomainError::InvalidData(format!(
                    "Preset file name is not valid UTF-8: {}",
                    path.display()
                ))
            })?;
        let raw_content = fs::read_to_string(&path).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read preset file '{}': {}",
                path.display(),
                error
            ))
        })?;
        let value: Value = serde_json::from_str(&raw_content).map_err(|error| {
            DomainError::InvalidData(format!(
                "Invalid preset JSON in '{}': {}",
                path.display(),
                error
            ))
        })?;
        let name = preset_name_from_value(file_stem, &value);
        let is_canonical = canonical_preset_file_stem(&name)? == file_stem;

        let entry = NamedPresetFile {
            #[cfg(test)]
            path,
            name: name.clone(),
            raw_content,
            is_canonical,
        };

        if let Some(existing_index) = indices.get(&name).copied() {
            let existing = &entries[existing_index];
            match (existing.is_canonical, entry.is_canonical) {
                (true, false) => continue,
                (false, true) => entries[existing_index] = entry,
                _ => {
                    return Err(DomainError::InvalidData(format!(
                        "Duplicate preset name '{}' found in '{}'",
                        name,
                        dir.display()
                    )));
                }
            }
        } else {
            indices.insert(name, entries.len());
            entries.push(entry);
        }
    }

    Ok(entries)
}

fn preset_name_from_value(file_stem: &str, value: &Value) -> String {
    value
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or(file_stem)
        .to_string()
}

fn is_windows_reserved_name(file_stem: &str) -> bool {
    let base_name = file_stem.split('.').next().unwrap_or(file_stem);
    WINDOWS_RESERVED_NAMES
        .iter()
        .any(|reserved| reserved.eq_ignore_ascii_case(base_name))
}

#[cfg(test)]
#[allow(deprecated)]
mod tests {
    use super::{
        PresetFilePaths, canonical_preset_file_stem, legacy_preset_file_stem,
        load_named_preset_files,
    };
    use crate::domain::errors::DomainError;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new() -> Self {
            let suffix = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time should be after unix epoch")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "tauritavern-preset-file-naming-test-{}-{}",
                std::process::id(),
                suffix
            ));
            std::fs::create_dir_all(&path).expect("create temp dir");
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn canonical_stem_matches_upstream_unicode_behavior() {
        assert_eq!(
            canonical_preset_file_stem("【明月青秋】").expect("valid stem"),
            "【明月青秋】"
        );
        assert_eq!(
            canonical_preset_file_stem("name/with\\unsafe:chars").expect("valid stem"),
            "namewithunsafechars"
        );
        assert_eq!(
            canonical_preset_file_stem("name*with?more\"unsafe<chars>").expect("valid stem"),
            "namewithmoreunsafechars"
        );
        assert_eq!(
            canonical_preset_file_stem(" test:/name?.png. ").expect("valid stem"),
            " testname.png"
        );
        assert_eq!(
            canonical_preset_file_stem("emoji😀【名】").expect("valid stem"),
            "emoji😀【名】"
        );
    }

    #[test]
    fn canonical_stem_rejects_reserved_windows_names() {
        assert!(canonical_preset_file_stem("CON").is_err());
        assert!(canonical_preset_file_stem("NUL.txt").is_err());
        assert!(canonical_preset_file_stem("  .  ").is_err());
    }

    #[test]
    fn legacy_stem_keeps_buggy_underscore_behavior() {
        #[allow(deprecated)]
        let legacy = legacy_preset_file_stem("【明月青秋】");
        assert_eq!(legacy, "_明月青秋_");
    }

    #[tokio::test]
    async fn prepare_for_save_migrates_deprecated_legacy_path() {
        let dir = TestDir::new();
        let paths = PresetFilePaths::new("【明月青秋】", dir.path(), ".json").expect("paths");
        #[allow(deprecated)]
        let legacy = paths.legacy_path().to_path_buf();
        tokio::fs::write(&legacy, r#"{"name":"【明月青秋】"}"#)
            .await
            .expect("write legacy file");

        let canonical = paths.prepare_for_save().await.expect("prepare for save");

        assert_eq!(canonical, paths.canonical_path());
        assert!(canonical.exists());
        assert!(!legacy.exists());
    }

    #[tokio::test]
    async fn prepare_for_save_rejects_conflicting_canonical_and_deprecated_legacy_paths() {
        let dir = TestDir::new();
        let paths = PresetFilePaths::new("【明月青秋】", dir.path(), ".json").expect("paths");
        #[allow(deprecated)]
        let legacy = paths.legacy_path().to_path_buf();
        tokio::fs::write(paths.canonical_path(), r#"{"name":"【明月青秋】"}"#)
            .await
            .expect("write canonical file");
        tokio::fs::write(&legacy, r#"{"name":"【明月青秋】"}"#)
            .await
            .expect("write legacy file");

        let error = paths
            .prepare_for_save()
            .await
            .expect_err("conflicting files should fail");

        assert!(matches!(error, DomainError::InvalidData(_)));
    }

    #[tokio::test]
    async fn load_named_preset_files_prefers_canonical_over_deprecated_legacy() {
        let dir = TestDir::new();
        tokio::fs::write(
            dir.path().join("_明月青秋_.json"),
            r#"{"name":"【明月青秋】","temperature":0.1}"#,
        )
        .await
        .expect("write legacy preset");
        tokio::fs::write(
            dir.path().join("【明月青秋】.json"),
            r#"{"name":"【明月青秋】","temperature":0.9}"#,
        )
        .await
        .expect("write canonical preset");

        let entries = load_named_preset_files(dir.path())
            .await
            .expect("load preset files");

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "【明月青秋】");
        assert!(entries[0].is_canonical);
        assert_eq!(entries[0].path, dir.path().join("【明月青秋】.json"));
    }

    #[tokio::test]
    async fn load_named_preset_files_falls_back_to_file_stem_without_embedded_name() {
        let dir = TestDir::new();
        tokio::fs::write(
            dir.path().join("Plain Preset.json"),
            r#"{"temperature":0.5}"#,
        )
        .await
        .expect("write preset");

        let entries = load_named_preset_files(dir.path())
            .await
            .expect("load preset files");

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "Plain Preset");
        assert!(entries[0].is_canonical);
    }
}
