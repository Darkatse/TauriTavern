use serde::de::DeserializeOwned;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::OnceLock;
use tauri::AppHandle;
#[cfg(not(embedded_resources))]
use tauri::Manager;
#[cfg(not(embedded_resources))]
use tauri::path::BaseDirectory;
#[cfg(not(embedded_resources))]
use tauri_plugin_fs::FsExt;
use tokio::fs;

use tt_domain::errors::DomainError;

static DEFAULT_FILES_MANIFEST: OnceLock<BTreeMap<String, Vec<String>>> = OnceLock::new();

#[cfg(any(embedded_resources, feature = "portable"))]
mod embedded_resources {
    include!(concat!(env!("OUT_DIR"), "/embedded_resources.rs"));
}

pub fn read_resource_text(
    app_handle: &AppHandle,
    relative_path: &str,
) -> Result<String, DomainError> {
    let bytes = read_resource_bytes(app_handle, relative_path)?;
    String::from_utf8(bytes).map_err(|e| {
        tracing::error!("Failed to decode resource text {:?}: {}", relative_path, e);
        DomainError::InvalidData(format!(
            "Resource '{}' is not valid UTF-8: {}",
            relative_path, e
        ))
    })
}

pub fn read_resource_bytes(
    app_handle: &AppHandle,
    relative_path: &str,
) -> Result<Vec<u8>, DomainError> {
    let normalized = normalize_resource_relative_path(relative_path)?;

    #[cfg(embedded_resources)]
    {
        let _ = app_handle;
        if let Some(bytes) = embedded_resources::get_embedded_resource(&normalized) {
            return Ok(bytes.to_vec());
        }

        Err(DomainError::NotFound(format!(
            "Embedded resource not found: {}",
            normalized
        )))
    }

    #[cfg(not(embedded_resources))]
    {
        let path = resolve_resource_path(app_handle, &normalized)?;
        match app_handle.fs().read(&path) {
            Ok(bytes) => Ok(bytes),
            Err(error) => {
                #[cfg(feature = "portable")]
                {
                    if error.kind() == std::io::ErrorKind::NotFound {
                        if let Some(bytes) = embedded_resources::get_embedded_resource(&normalized)
                        {
                            tracing::debug!(
                                "Resource {:?} not found on disk at {:?}, using embedded fallback",
                                normalized,
                                path
                            );
                            return Ok(bytes.to_vec());
                        }
                    }
                }

                tracing::error!(
                    "Failed to read resource bytes {:?} (resolved to {:?}): {}",
                    normalized,
                    path,
                    error
                );
                Err(map_resource_error(&normalized, error))
            }
        }
    }
}

pub fn read_resource_json<T: DeserializeOwned>(
    app_handle: &AppHandle,
    relative_path: &str,
) -> Result<T, DomainError> {
    let text = read_resource_text(app_handle, relative_path)?;
    serde_json::from_str(&text).map_err(|e| {
        tracing::error!("Failed to parse JSON resource {:?}: {}", relative_path, e);
        DomainError::InvalidData(format!("Invalid JSON resource '{}': {}", relative_path, e))
    })
}

pub async fn copy_resource_to_file(
    app_handle: &AppHandle,
    resource_relative_path: &str,
    destination: &Path,
) -> Result<(), DomainError> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).await.map_err(|e| {
            tracing::error!("Failed to create destination directory {:?}: {}", parent, e);
            DomainError::InternalError(format!("Failed to create directory: {}", e))
        })?;
    }

    let bytes = read_resource_bytes(app_handle, resource_relative_path)?;
    fs::write(destination, bytes).await.map_err(|e| {
        tracing::error!(
            "Failed to write copied resource {:?} to {:?}: {}",
            resource_relative_path,
            destination,
            e
        );
        DomainError::InternalError(format!("Failed to write resource file: {}", e))
    })
}

pub fn list_default_files_under(root: &str, prefix: &str) -> Vec<String> {
    let normalized_prefix = prefix.trim_matches('/').replace('\\', "/");
    let query = if normalized_prefix.is_empty() {
        String::new()
    } else {
        format!("{}/", normalized_prefix)
    };

    let manifest = DEFAULT_FILES_MANIFEST.get_or_init(load_default_files_manifest);
    let files = manifest.get(root).expect("known default resource root");
    if query.is_empty() {
        return files.to_vec();
    }

    files
        .iter()
        .filter(|path| path.starts_with(&query))
        .cloned()
        .collect()
}

fn load_default_files_manifest() -> BTreeMap<String, Vec<String>> {
    serde_json::from_str(include_str!(concat!(
        env!("OUT_DIR"),
        "/default_files_manifest.json"
    )))
    .expect("valid generated default resource manifest")
}

#[cfg(not(embedded_resources))]
fn resolve_resource_path(
    app_handle: &AppHandle,
    relative_path: &str,
) -> Result<std::path::PathBuf, DomainError> {
    app_handle
        .path()
        .resolve(relative_path, BaseDirectory::Resource)
        .map_err(|e| {
            tracing::error!("Failed to resolve resource path {:?}: {}", relative_path, e);
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

#[cfg(not(embedded_resources))]
fn map_resource_error(relative_path: &str, error: std::io::Error) -> DomainError {
    if error.kind() == std::io::ErrorKind::NotFound {
        DomainError::NotFound(format!("Resource not found: {}", relative_path))
    } else {
        DomainError::InternalError(format!(
            "Failed to read resource '{}': {}",
            relative_path, error
        ))
    }
}
