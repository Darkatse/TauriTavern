use std::error::Error;
use std::io;
use std::path::{Path, PathBuf};

#[cfg(not(any(target_os = "android", target_os = "ios")))]
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

#[cfg(not(any(target_os = "android", target_os = "ios")))]
const RUNTIME_MODE_ENV: &str = "TAURITAVERN_RUNTIME_MODE";
#[cfg(not(any(target_os = "android", target_os = "ios")))]
const PORTABLE_MARKER_FILE: &str = "portable.flag";
#[cfg(not(any(target_os = "android", target_os = "ios")))]
const RUNTIME_CONFIG_FILE: &str = "tauritavern-runtime.json";
const DATA_ARCHIVE_ROOT_DIR: &str = ".data-archive";
const DATA_ARCHIVE_IMPORTS_DIR: &str = "imports";
const DATA_ARCHIVE_EXPORTS_DIR: &str = "exports";
#[cfg(not(any(target_os = "android", target_os = "ios")))]
const DEFAULT_USER_DIR_NAME: &str = "default-user";
#[cfg(not(any(target_os = "android", target_os = "ios")))]
const EFFECTIVELY_EMPTY_DIRECTORY_ENTRIES: &[&str] = &[
    ".ds_store",
    ".localized",
    "desktop.ini",
    "thumbs.db",
    "icon\r",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeMode {
    Standard,
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    Portable,
}

#[derive(Debug, Clone)]
pub struct RuntimePaths {
    pub mode: RuntimeMode,
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub app_root: PathBuf,
    pub data_root: PathBuf,
    pub log_root: PathBuf,
    pub archive_imports_root: PathBuf,
    pub archive_exports_root: PathBuf,
}

impl RuntimePaths {
    fn new(mode: RuntimeMode, app_root: PathBuf) -> Self {
        let data_root = app_root.join("data");
        let log_root = app_root.join("logs");
        let archive_root = app_root.join(DATA_ARCHIVE_ROOT_DIR);
        let archive_imports_root = archive_root.join(DATA_ARCHIVE_IMPORTS_DIR);
        let archive_exports_root = archive_root.join(DATA_ARCHIVE_EXPORTS_DIR);

        Self {
            mode,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            app_root,
            data_root,
            log_root,
            archive_imports_root,
            archive_exports_root,
        }
    }
}

pub fn resolve_runtime_paths(app_handle: &AppHandle) -> Result<RuntimePaths, Box<dyn Error>> {
    let paths = resolve_runtime_paths_inner(app_handle)?;
    ensure_startup_paths(&paths)?;
    tracing::info!(
        "Runtime mode: {:?}, data_root: {:?}, log_root: {:?}",
        paths.mode,
        paths.data_root,
        paths.log_root
    );
    Ok(paths)
}

#[cfg(any(target_os = "android", target_os = "ios"))]
fn resolve_runtime_paths_inner(app_handle: &AppHandle) -> Result<RuntimePaths, Box<dyn Error>> {
    resolve_standard_runtime_paths(app_handle)
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn resolve_runtime_paths_inner(app_handle: &AppHandle) -> Result<RuntimePaths, Box<dyn Error>> {
    let mode = detect_desktop_runtime_mode();
    let paths = match mode {
        RuntimeMode::Portable => resolve_portable_runtime_paths(),
        RuntimeMode::Standard => resolve_standard_runtime_paths(app_handle),
    }?;

    apply_runtime_config(paths)
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn apply_runtime_config(mut paths: RuntimePaths) -> Result<RuntimePaths, Box<dyn Error>> {
    let Some(mut config) = load_runtime_config(&paths.app_root)? else {
        return Ok(paths);
    };

    if let Some(migration) = config.migration.clone() {
        paths.data_root =
            resolve_pending_data_root_migration(&paths.app_root, &mut config, migration)?;
        return Ok(paths);
    }

    paths.data_root = config.data_root;
    Ok(paths)
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn resolve_pending_data_root_migration(
    app_root: &Path,
    config: &mut TauriTavernRuntimeConfig,
    migration: DataRootMigration,
) -> Result<PathBuf, Box<dyn Error>> {
    let config_path = runtime_config_path(app_root);

    if !migration.from.exists() {
        if is_initialized_data_root(&config.data_root) {
            config.migration = None;
            config.migration_error = None;
            persist_runtime_config_best_effort(
                &config_path,
                config,
                "clear runtime migration marker",
            );
            return Ok(config.data_root.clone());
        }

        let error_text = format!(
            "{} -> {}: migration source is missing and target is not an initialized data root",
            migration.from.display(),
            config.data_root.display()
        );
        config.migration_error = Some(error_text.clone());
        persist_runtime_config_best_effort(
            &config_path,
            config,
            "persist runtime migration error for missing source",
        );
        tracing::error!("Data directory migration failed: {}", error_text);
        return Err(Box::new(io::Error::new(
            io::ErrorKind::NotFound,
            error_text,
        )));
    }

    match migrate_data_root(&migration.from, &config.data_root) {
        Ok(()) => {
            config.migration = None;
            config.migration_error = None;
            persist_runtime_config_best_effort(
                &config_path,
                config,
                "update runtime config after migration",
            );
            Ok(config.data_root.clone())
        }
        Err(error) => {
            let error_text = format!(
                "{} -> {}: {}",
                migration.from.display(),
                config.data_root.display(),
                error
            );
            config.migration_error = Some(error_text.clone());
            persist_runtime_config_best_effort(
                &config_path,
                config,
                "persist runtime migration error",
            );
            tracing::error!("Data directory migration failed: {}", error_text);
            Ok(migration.from)
        }
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn persist_runtime_config_best_effort(
    config_path: &Path,
    config: &TauriTavernRuntimeConfig,
    action: &str,
) {
    if let Err(error) = write_runtime_config_sync(config_path, config) {
        tracing::warn!("Failed to {} in {:?}: {}", action, config_path, error);
    }
}

fn ensure_startup_paths(paths: &RuntimePaths) -> Result<(), Box<dyn Error>> {
    std::fs::create_dir_all(&paths.data_root)?;
    std::fs::create_dir_all(&paths.log_root)?;
    Ok(())
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn detect_desktop_runtime_mode() -> RuntimeMode {
    if cfg!(feature = "portable") {
        tracing::info!("Portable mode forced by cargo feature 'portable'");
        return RuntimeMode::Portable;
    }

    if let Some(mode) = parse_runtime_mode_env() {
        return mode;
    }

    let exe_dir = match resolve_executable_directory() {
        Ok(path) => path,
        Err(error) => {
            tracing::warn!(
                "Failed to resolve executable directory, fallback to standard mode: {}",
                error
            );
            return RuntimeMode::Standard;
        }
    };

    let marker_path = exe_dir.join(PORTABLE_MARKER_FILE);
    if marker_path.is_file() {
        tracing::info!(
            "Portable mode detected by marker file: {}",
            marker_path.display()
        );
        return RuntimeMode::Portable;
    }

    RuntimeMode::Standard
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn parse_runtime_mode_env() -> Option<RuntimeMode> {
    if let Ok(raw) = std::env::var(RUNTIME_MODE_ENV) {
        let normalized = raw.trim().to_ascii_lowercase();
        match normalized.as_str() {
            "portable" => {
                tracing::info!(
                    "Portable mode forced by environment variable {}={}",
                    RUNTIME_MODE_ENV,
                    raw
                );
                return Some(RuntimeMode::Portable);
            }
            "standard" => {
                tracing::info!(
                    "Standard mode forced by environment variable {}={}",
                    RUNTIME_MODE_ENV,
                    raw
                );
                return Some(RuntimeMode::Standard);
            }
            _ => {
                tracing::warn!(
                    "Ignoring invalid {} value '{}', expected 'portable' or 'standard'",
                    RUNTIME_MODE_ENV,
                    raw
                );
            }
        }
    }

    None
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn resolve_portable_runtime_paths() -> Result<RuntimePaths, Box<dyn Error>> {
    let exe_dir = resolve_executable_directory()?;
    Ok(RuntimePaths::new(RuntimeMode::Portable, exe_dir))
}

fn resolve_standard_runtime_paths(app_handle: &AppHandle) -> Result<RuntimePaths, Box<dyn Error>> {
    let app_root = resolve_app_data_dir(app_handle)?;
    Ok(RuntimePaths::new(RuntimeMode::Standard, app_root))
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn resolve_executable_directory() -> Result<PathBuf, Box<dyn Error>> {
    let executable_path = std::env::current_exe()?;
    let exe_dir = executable_path.parent().ok_or_else(|| {
        Box::new(io::Error::new(
            io::ErrorKind::NotFound,
            "Failed to resolve executable directory",
        )) as Box<dyn Error>
    })?;
    Ok(exe_dir.to_path_buf())
}

pub fn resolve_app_data_dir(app_handle: &AppHandle) -> Result<PathBuf, Box<dyn Error>> {
    #[cfg(target_os = "android")]
    {
        return resolve_android_app_data_dir(app_handle);
    }

    #[cfg(not(target_os = "android"))]
    {
        Ok(app_handle.path().app_data_dir()?)
    }
}

#[cfg(target_os = "android")]
fn resolve_android_app_data_dir(app_handle: &AppHandle) -> Result<PathBuf, Box<dyn Error>> {
    let reported_app_data_dir = app_handle.path().app_data_dir().ok();

    if let Some(path) = reported_app_data_dir.as_ref() {
        if is_android_external_app_data_dir(path) {
            tracing::debug!(
                "Using Android app_data_dir from Tauri path resolver: {:?}",
                path
            );
            return Ok(path.clone());
        }

        if !is_android_internal_app_data_dir(path) {
            tracing::debug!(
                "Using Android app_data_dir from Tauri path resolver (non-internal path): {:?}",
                path
            );
            return Ok(path.clone());
        }
    }

    if let Ok(document_dir) = app_handle.path().document_dir() {
        if let Some(derived_external_dir) = derive_android_external_app_data_dir(&document_dir) {
            tracing::debug!(
                "Using Android external app data directory derived from document_dir: {:?}",
                derived_external_dir
            );
            return Ok(derived_external_dir);
        }
    }

    if let Some(path) = reported_app_data_dir {
        tracing::warn!(
            "Falling back to Android app_data_dir reported by Tauri path resolver: {:?}",
            path
        );
        return Ok(path);
    }

    Err(Box::new(io::Error::new(
        io::ErrorKind::NotFound,
        "Unable to resolve Android app data directory",
    )))
}

#[cfg(target_os = "android")]
fn derive_android_external_app_data_dir(document_dir: &Path) -> Option<PathBuf> {
    let leaf = document_dir.file_name()?.to_str()?;

    let candidate = if leaf.eq_ignore_ascii_case("documents") {
        let parent = document_dir.parent()?;
        let parent_leaf = parent.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if parent_leaf.eq_ignore_ascii_case("files") {
            parent.parent()?.to_path_buf()
        } else {
            parent.to_path_buf()
        }
    } else if leaf.eq_ignore_ascii_case("files") {
        document_dir.parent()?.to_path_buf()
    } else {
        return None;
    };

    if is_android_external_app_data_dir(&candidate) {
        Some(candidate)
    } else {
        None
    }
}

#[cfg(target_os = "android")]
fn is_android_external_app_data_dir(path: &Path) -> bool {
    let normalized = normalize_android_path(path);
    normalized.contains("/android/data/")
}

#[cfg(target_os = "android")]
fn is_android_internal_app_data_dir(path: &Path) -> bool {
    let normalized = normalize_android_path(path);
    normalized.starts_with("/data/user/") || normalized.starts_with("/data/data/")
}

#[cfg(target_os = "android")]
fn normalize_android_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/").to_lowercase()
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn is_ignorable_effectively_empty_entry(entry: &std::fs::DirEntry) -> Result<bool, io::Error> {
    if !entry.file_type()?.is_file() {
        return Ok(false);
    }

    let normalized = entry
        .file_name()
        .to_string_lossy()
        .trim()
        .to_ascii_lowercase();
    Ok(EFFECTIVELY_EMPTY_DIRECTORY_ENTRIES.contains(&normalized.as_str()))
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn collect_ignorable_effectively_empty_entries(
    path: &Path,
) -> Result<Option<Vec<PathBuf>>, io::Error> {
    let mut ignorable_entries = Vec::new();

    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        if is_ignorable_effectively_empty_entry(&entry)? {
            ignorable_entries.push(entry.path());
            continue;
        }

        return Ok(None);
    }

    Ok(Some(ignorable_entries))
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub(crate) fn is_effectively_empty_directory(path: &Path) -> Result<bool, io::Error> {
    Ok(collect_ignorable_effectively_empty_entries(path)?.is_some())
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn non_empty_directory_error(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!(
            "Migration target directory is not empty: {}",
            path.display()
        ),
    )
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn prepare_effectively_empty_directory(path: &Path) -> Result<(), Box<dyn Error>> {
    std::fs::create_dir_all(path)?;
    if !path.is_dir() {
        return Err(Box::new(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Migration target is not a directory: {}", path.display()),
        )));
    }

    let Some(ignorable_entries) = collect_ignorable_effectively_empty_entries(path)? else {
        return Err(Box::new(non_empty_directory_error(path)));
    };

    for entry_path in ignorable_entries {
        std::fs::remove_file(&entry_path)?;
    }

    Ok(())
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn is_initialized_data_root(path: &Path) -> bool {
    path.join(DEFAULT_USER_DIR_NAME).is_dir()
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TauriTavernRuntimeConfig {
    #[serde(default = "runtime_config_version")]
    pub version: u32,
    pub data_root: PathBuf,
    #[serde(default)]
    pub migration: Option<DataRootMigration>,
    #[serde(default)]
    pub migration_error: Option<String>,
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct DataRootMigration {
    pub from: PathBuf,
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn runtime_config_version() -> u32 {
    TAURITAVERN_RUNTIME_CONFIG_VERSION
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub const TAURITAVERN_RUNTIME_CONFIG_VERSION: u32 = 1;

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn runtime_config_path(app_root: impl AsRef<std::path::Path>) -> PathBuf {
    app_root.as_ref().join(RUNTIME_CONFIG_FILE)
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub(crate) fn load_runtime_config(
    app_root: &std::path::Path,
) -> Result<Option<TauriTavernRuntimeConfig>, Box<dyn Error>> {
    let path = runtime_config_path(app_root);
    if !path.is_file() {
        return Ok(None);
    }

    let raw = std::fs::read_to_string(&path)?;
    let mut config: TauriTavernRuntimeConfig = serde_json::from_str(&raw)?;
    config.data_root = dunce::simplified(&config.data_root).to_path_buf();
    if let Some(migration) = config.migration.as_mut() {
        migration.from = dunce::simplified(&migration.from).to_path_buf();
    }

    if config.version != runtime_config_version() {
        return Err(Box::new(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "Unsupported runtime config version {}, expected {}",
                config.version,
                runtime_config_version()
            ),
        )));
    }

    if !config.data_root.is_absolute() {
        return Err(Box::new(io::Error::new(
            io::ErrorKind::InvalidData,
            "Runtime config data_root must be an absolute path",
        )));
    }

    if let Some(migration) = config.migration.as_ref() {
        if !migration.from.is_absolute() {
            return Err(Box::new(io::Error::new(
                io::ErrorKind::InvalidData,
                "Runtime config migration.from must be an absolute path",
            )));
        }
    }

    Ok(Some(config))
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn write_runtime_config_sync(
    path: &std::path::Path,
    config: &TauriTavernRuntimeConfig,
) -> Result<(), Box<dyn Error>> {
    use crate::infrastructure::persistence::file_system::{
        replace_file_with_fallback_sync, unique_temp_path,
    };

    let bytes = serde_json::to_vec_pretty(config)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let temp_path = unique_temp_path(path, "tauritavern-runtime.json");
    std::fs::write(&temp_path, &bytes)?;
    replace_file_with_fallback_sync(&temp_path, path)
        .map_err(|error| Box::new(io::Error::other(error.to_string())) as Box<dyn Error>)?;
    Ok(())
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn migrate_data_root(from: &std::path::Path, to: &std::path::Path) -> Result<(), Box<dyn Error>> {
    if from == to {
        return Ok(());
    }

    if !from.exists() {
        return Ok(());
    }

    if !from.is_dir() {
        return Err(Box::new(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Migration source is not a directory: {}", from.display()),
        )));
    }

    prepare_effectively_empty_directory(to)?;

    let canonical_from = dunce::canonicalize(from)?;
    let canonical_to = dunce::canonicalize(to)?;
    if canonical_to.starts_with(&canonical_from) {
        return Err(Box::new(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Migration target cannot be inside the source directory",
        )));
    }

    // Prefer an atomic move when possible (same-volume).
    if std::fs::remove_dir(to).is_ok() {
        match std::fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(error) => {
                tracing::warn!(
                    "Failed to move data_root using rename (fallback to copy): {}",
                    error
                );
                std::fs::create_dir_all(to)?;
            }
        }
    }

    if let Err(error) = copy_dir_recursive(from, to) {
        let _ = std::fs::remove_dir_all(to);
        let _ = std::fs::create_dir_all(to);
        return Err(error);
    }

    if let Err(error) = std::fs::remove_dir_all(from) {
        tracing::warn!(
            "Failed to remove migration source directory {}: {}",
            from.display(),
            error
        );
    }
    Ok(())
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn copy_dir_recursive(from: &std::path::Path, to: &std::path::Path) -> Result<(), Box<dyn Error>> {
    std::fs::create_dir_all(to)?;

    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let name = entry.file_name();
        let source = entry.path();
        let target = to.join(name);

        if file_type.is_symlink() {
            copy_symlink(&source, &target)?;
            continue;
        }

        if file_type.is_dir() {
            copy_dir_recursive(&source, &target)?;
            continue;
        }

        if file_type.is_file() {
            std::fs::copy(&source, &target)?;
            continue;
        }

        return Err(Box::new(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "Unsupported file type during migration copy: {}",
                source.display()
            ),
        )));
    }

    Ok(())
}

#[cfg(all(not(any(target_os = "android", target_os = "ios")), unix))]
fn copy_symlink(from: &std::path::Path, to: &std::path::Path) -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    let target = std::fs::read_link(from)?;
    symlink(&target, to)?;
    Ok(())
}

#[cfg(all(not(any(target_os = "android", target_os = "ios")), windows))]
fn copy_symlink(from: &std::path::Path, to: &std::path::Path) -> Result<(), Box<dyn Error>> {
    use std::os::windows::fs::{symlink_dir, symlink_file};

    let target = std::fs::read_link(from)?;

    match std::fs::metadata(from) {
        Ok(metadata) if metadata.is_dir() => symlink_dir(&target, to)?,
        Ok(_) => symlink_file(&target, to)?,
        Err(_) => {
            if symlink_file(&target, to).is_err() {
                symlink_dir(&target, to)?;
            }
        }
    }

    Ok(())
}

#[cfg(all(test, not(any(target_os = "android", target_os = "ios"))))]
mod tests {
    use super::*;
    use std::fs;
    use uuid::Uuid;

    struct TempDirGuard {
        root: PathBuf,
    }

    impl TempDirGuard {
        fn new(prefix: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "tauritavern-runtime-paths-{}-{}",
                prefix,
                Uuid::new_v4()
            ));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).expect("create temp root");
            Self { root }
        }
    }

    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn runtime_paths_with_app_root(app_root: &Path) -> RuntimePaths {
        RuntimePaths::new(RuntimeMode::Standard, app_root.to_path_buf())
    }

    #[test]
    fn effectively_empty_directory_ignores_known_metadata_files() {
        let temp = TempDirGuard::new("effectively-empty");

        fs::write(temp.root.join("desktop.ini"), "").expect("write desktop.ini");
        fs::write(temp.root.join(".DS_Store"), "").expect("write .DS_Store");

        assert!(
            is_effectively_empty_directory(&temp.root).expect("inspect temp root"),
            "expected metadata-only directory to be treated as empty"
        );

        fs::write(temp.root.join("actual.txt"), "content").expect("write actual file");
        assert!(
            !is_effectively_empty_directory(&temp.root).expect("inspect non-empty directory"),
            "expected non-metadata file to make directory non-empty"
        );
    }

    #[test]
    fn migrate_data_root_accepts_metadata_only_target_directory() {
        let temp = TempDirGuard::new("metadata-target");
        let from = temp.root.join("from");
        let to = temp.root.join("to");

        fs::create_dir_all(from.join(DEFAULT_USER_DIR_NAME).join("chats"))
            .expect("create source chats");
        fs::write(
            from.join(DEFAULT_USER_DIR_NAME)
                .join("chats")
                .join("chat.jsonl"),
            "demo",
        )
        .expect("write source file");
        fs::create_dir_all(&to).expect("create target dir");
        fs::write(to.join("desktop.ini"), "").expect("write target metadata");

        migrate_data_root(&from, &to).expect("migrate data root");

        assert!(
            to.join(DEFAULT_USER_DIR_NAME)
                .join("chats")
                .join("chat.jsonl")
                .is_file(),
            "expected migrated file to exist at target"
        );
        assert!(
            !from.exists(),
            "expected source root to be removed after migration"
        );
        assert!(
            !to.join("desktop.ini").exists(),
            "expected ignorable metadata file to be removed before migration"
        );
    }

    #[test]
    fn apply_runtime_config_finalizes_missing_source_when_target_is_initialized() {
        let temp = TempDirGuard::new("missing-source-finalize");
        let target = temp.root.join("migrated-data");
        fs::create_dir_all(target.join(DEFAULT_USER_DIR_NAME)).expect("create initialized target");
        fs::write(target.join(DEFAULT_USER_DIR_NAME).join("marker.txt"), "ok")
            .expect("write target marker");

        let config = TauriTavernRuntimeConfig {
            version: TAURITAVERN_RUNTIME_CONFIG_VERSION,
            data_root: target.clone(),
            migration: Some(DataRootMigration {
                from: temp.root.join("missing-source"),
            }),
            migration_error: None,
        };
        write_runtime_config_sync(&runtime_config_path(&temp.root), &config)
            .expect("write runtime config");

        let resolved = apply_runtime_config(runtime_paths_with_app_root(&temp.root))
            .expect("apply runtime config");

        assert_eq!(resolved.data_root, target);

        let persisted = load_runtime_config(&temp.root)
            .expect("load runtime config")
            .expect("runtime config should exist");
        assert!(
            persisted.migration.is_none(),
            "migration marker should be cleared"
        );
        assert!(
            persisted.migration_error.is_none(),
            "migration error should be cleared after successful recovery"
        );
    }

    #[test]
    fn apply_runtime_config_fails_when_missing_source_target_is_not_initialized() {
        let temp = TempDirGuard::new("missing-source-invalid-target");
        let target = temp.root.join("empty-target");
        fs::create_dir_all(&target).expect("create empty target");

        let config = TauriTavernRuntimeConfig {
            version: TAURITAVERN_RUNTIME_CONFIG_VERSION,
            data_root: target.clone(),
            migration: Some(DataRootMigration {
                from: temp.root.join("missing-source"),
            }),
            migration_error: None,
        };
        write_runtime_config_sync(&runtime_config_path(&temp.root), &config)
            .expect("write runtime config");

        let error = apply_runtime_config(runtime_paths_with_app_root(&temp.root))
            .expect_err("expected missing-source invalid-target branch to fail");
        let error_text = error.to_string();
        assert!(
            error_text.contains("migration source is missing"),
            "unexpected error text: {error_text}"
        );

        let persisted = load_runtime_config(&temp.root)
            .expect("load runtime config")
            .expect("runtime config should exist");
        assert!(
            persisted.migration.is_some(),
            "migration marker should be kept"
        );
        assert!(
            persisted
                .migration_error
                .as_deref()
                .is_some_and(|value| value.contains("migration source is missing")),
            "expected persisted migration error"
        );
    }

    #[test]
    fn apply_runtime_config_recovers_to_existing_source_on_migration_failure() {
        let temp = TempDirGuard::new("migration-recovery");
        let source = temp.root.join("data");
        let target = temp.root.join("occupied-target");

        fs::create_dir_all(source.join(DEFAULT_USER_DIR_NAME)).expect("create source root");
        fs::write(
            source.join(DEFAULT_USER_DIR_NAME).join("source.txt"),
            "source",
        )
        .expect("write source file");
        fs::create_dir_all(&target).expect("create target root");
        fs::write(target.join("keep.txt"), "occupied").expect("write conflicting target file");

        let config = TauriTavernRuntimeConfig {
            version: TAURITAVERN_RUNTIME_CONFIG_VERSION,
            data_root: target.clone(),
            migration: Some(DataRootMigration {
                from: source.clone(),
            }),
            migration_error: None,
        };
        write_runtime_config_sync(&runtime_config_path(&temp.root), &config)
            .expect("write runtime config");

        let resolved = apply_runtime_config(runtime_paths_with_app_root(&temp.root))
            .expect("apply runtime config");

        assert_eq!(
            resolved.data_root, source,
            "expected runtime to keep using the existing source root after migration failure"
        );

        let persisted = load_runtime_config(&temp.root)
            .expect("load runtime config")
            .expect("runtime config should exist");
        assert!(
            persisted.migration.is_some(),
            "migration marker should remain pending"
        );
        assert!(
            persisted
                .migration_error
                .as_deref()
                .is_some_and(|value| value.contains("Migration target directory is not empty")),
            "expected persisted migration error to mention the conflicting target"
        );
    }

    #[test]
    fn copy_dir_recursive_preserves_symlinks() {
        let temp = TempDirGuard::new("symlink-copy");
        let from = temp.root.join("from");
        let to = temp.root.join("to");

        fs::create_dir_all(&from).expect("create source dir");
        fs::write(from.join("real.txt"), "payload").expect("write source file");

        let symlink_result = create_test_symlink(Path::new("real.txt"), &from.join("link.txt"));
        if let Err(error) = symlink_result {
            if matches!(
                error.kind(),
                io::ErrorKind::PermissionDenied | io::ErrorKind::Unsupported
            ) {
                eprintln!("Skipping symlink copy test: {error}");
                return;
            }
            panic!("failed to create test symlink: {error}");
        }

        copy_dir_recursive(&from, &to).expect("copy directory recursively");

        let target_link = to.join("link.txt");
        assert!(
            target_link.exists(),
            "expected copied symlink to exist at target"
        );
        assert_eq!(
            fs::read_link(&target_link).expect("read target symlink"),
            PathBuf::from("real.txt")
        );
        assert_eq!(
            fs::read_to_string(&target_link).expect("read via copied symlink"),
            "payload"
        );
    }

    #[cfg(unix)]
    fn create_test_symlink(target: &Path, link: &Path) -> Result<(), io::Error> {
        std::os::unix::fs::symlink(target, link)
    }

    #[cfg(windows)]
    fn create_test_symlink(target: &Path, link: &Path) -> Result<(), io::Error> {
        std::os::windows::fs::symlink_file(target, link)
    }
}
