use std::error::Error;
use std::io;
#[cfg(target_os = "android")]
use std::path::Path;
use std::path::PathBuf;

use tauri::{AppHandle, Manager};

const RUNTIME_MODE_ENV: &str = "TAURITAVERN_RUNTIME_MODE";
const PORTABLE_MARKER_FILE: &str = "portable.flag";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeMode {
    Standard,
    Portable,
}

#[derive(Debug, Clone)]
pub struct RuntimePaths {
    pub mode: RuntimeMode,
    pub data_root: PathBuf,
    pub log_root: PathBuf,
    pub archive_jobs_root: PathBuf,
    pub archive_exports_root: PathBuf,
}

impl RuntimePaths {
    fn new(mode: RuntimeMode, app_root: PathBuf) -> Self {
        let data_root = app_root.join("data");
        let log_root = app_root.join("logs");
        let archive_jobs_root = app_root.join(".data-archive-jobs");
        let archive_exports_root = app_root.join(".data-archive-exports");

        Self {
            mode,
            data_root,
            log_root,
            archive_jobs_root,
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
    match mode {
        RuntimeMode::Portable => resolve_portable_runtime_paths(),
        RuntimeMode::Standard => resolve_standard_runtime_paths(app_handle),
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
