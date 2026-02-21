use std::error::Error;
#[cfg(target_os = "android")]
use std::io;
#[cfg(target_os = "android")]
use std::path::Path;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

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
