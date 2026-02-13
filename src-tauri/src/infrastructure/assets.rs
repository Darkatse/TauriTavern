use serde::de::DeserializeOwned;
use std::path::Path;
use std::sync::OnceLock;
#[cfg(not(target_os = "android"))]
use tauri::path::BaseDirectory;
use tauri::AppHandle;
#[cfg(not(target_os = "android"))]
use tauri::Manager;
#[cfg(not(target_os = "android"))]
use tauri_plugin_fs::FsExt;
use tokio::fs;

use crate::domain::errors::DomainError;
use crate::infrastructure::logging::logger;

static DEFAULT_CONTENT_MANIFEST: OnceLock<Vec<String>> = OnceLock::new();

#[cfg(target_os = "android")]
mod embedded_resources {
    include!(concat!(env!("OUT_DIR"), "/embedded_resources.rs"));
}

pub fn read_resource_text(app_handle: &AppHandle, relative_path: &str) -> Result<String, DomainError> {
    let bytes = read_resource_bytes(app_handle, relative_path)?;
    String::from_utf8(bytes).map_err(|e| {
        logger::error(&format!(
            "Failed to decode resource text {:?}: {}",
            relative_path, e
        ));
        DomainError::InvalidData(format!(
            "Resource '{}' is not valid UTF-8: {}",
            relative_path, e
        ))
    })
}

pub fn read_resource_bytes(app_handle: &AppHandle, relative_path: &str) -> Result<Vec<u8>, DomainError> {
    let normalized = normalize_resource_relative_path(relative_path)?;

    #[cfg(target_os = "android")]
    {
        let _ = app_handle;
        if let Some(bytes) = embedded_resources::get_embedded_resource(&normalized) {
            return Ok(bytes.to_vec());
        }

        return Err(DomainError::NotFound(format!(
            "Embedded resource not found: {}",
            normalized
        )));
    }

    #[cfg(not(target_os = "android"))]
    {
        let path = resolve_resource_path(app_handle, &normalized)?;
        app_handle.fs().read(&path).map_err(|e| {
            logger::error(&format!(
                "Failed to read resource bytes {:?} (resolved to {:?}): {}",
                normalized, path, e
            ));
            map_resource_error(&normalized, e)
        })
    }
}

pub fn read_resource_json<T: DeserializeOwned>(
    app_handle: &AppHandle,
    relative_path: &str,
) -> Result<T, DomainError> {
    let text = read_resource_text(app_handle, relative_path)?;
    serde_json::from_str(&text).map_err(|e| {
        logger::error(&format!(
            "Failed to parse JSON resource {:?}: {}",
            relative_path, e
        ));
        DomainError::InvalidData(format!(
            "Invalid JSON resource '{}': {}",
            relative_path, e
        ))
    })
}

pub async fn copy_resource_to_file(
    app_handle: &AppHandle,
    resource_relative_path: &str,
    destination: &Path,
) -> Result<(), DomainError> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).await.map_err(|e| {
            logger::error(&format!(
                "Failed to create destination directory {:?}: {}",
                parent, e
            ));
            DomainError::InternalError(format!("Failed to create directory: {}", e))
        })?;
    }

    let bytes = read_resource_bytes(app_handle, resource_relative_path)?;
    fs::write(destination, bytes).await.map_err(|e| {
        logger::error(&format!(
            "Failed to write copied resource {:?} to {:?}: {}",
            resource_relative_path, destination, e
        ));
        DomainError::InternalError(format!("Failed to write resource file: {}", e))
    })
}

pub fn list_default_content_files_under(prefix: &str) -> Vec<String> {
    let normalized_prefix = prefix.trim_matches('/').replace('\\', "/");
    let query = if normalized_prefix.is_empty() {
        String::new()
    } else {
        format!("{}/", normalized_prefix)
    };

    let files = DEFAULT_CONTENT_MANIFEST.get_or_init(load_default_content_manifest);
    if query.is_empty() {
        return files.to_vec();
    }

    files
        .iter()
        .filter(|path| path.starts_with(&query))
        .cloned()
        .collect()
}

fn load_default_content_manifest() -> Vec<String> {
    let raw = include_str!(concat!(env!("OUT_DIR"), "/default_content_manifest.json"));
    match serde_json::from_str::<Vec<String>>(raw) {
        Ok(mut entries) => {
            entries.sort();
            entries
        }
        Err(error) => {
            logger::error(&format!(
                "Failed to load generated default content manifest: {}",
                error
            ));
            Vec::new()
        }
    }
}

#[cfg(not(target_os = "android"))]
fn resolve_resource_path(app_handle: &AppHandle, relative_path: &str) -> Result<std::path::PathBuf, DomainError> {
    app_handle
        .path()
        .resolve(relative_path, BaseDirectory::Resource)
        .map_err(|e| {
            logger::error(&format!(
                "Failed to resolve resource path {:?}: {}",
                relative_path, e
            ));
            DomainError::InternalError(format!("Failed to resolve resource path: {}", e))
        })
}

fn normalize_resource_relative_path(relative_path: &str) -> Result<String, DomainError> {
    let normalized = relative_path.trim().replace('\\', "/");
    let normalized = normalized.trim_start_matches('/').to_string();

    if normalized.is_empty() || normalized.contains("..") {
        return Err(DomainError::InvalidData(format!(
            "Invalid resource path: {}",
            relative_path
        )));
    }

    Ok(normalized)
}

fn map_resource_error(relative_path: &str, error: std::io::Error) -> DomainError {
    if error.kind() == std::io::ErrorKind::NotFound {
        DomainError::NotFound(format!("Resource not found: {}", relative_path))
    } else {
        DomainError::InternalError(format!("Failed to read resource '{}': {}", relative_path, error))
    }
}
