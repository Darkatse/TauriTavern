use std::error::Error;
use std::path::PathBuf;
#[cfg(target_os = "android")]
use std::path::Path;
use tauri::{AppHandle, Manager};

// Temporary workaround for Tauri Android app-data path bug.
// When upstream is fixed, flip this to `false` (or remove Android branch).
#[cfg(target_os = "android")]
const USE_ANDROID_EXTERNAL_APP_DATA_WORKAROUND: bool = true;

pub fn resolve_app_data_dir(app_handle: &AppHandle) -> Result<PathBuf, Box<dyn Error>> {
    #[cfg(target_os = "android")]
    {
        if USE_ANDROID_EXTERNAL_APP_DATA_WORKAROUND {
            if let Ok(document_dir) = app_handle.path().document_dir() {
                if let Some(path) = derive_android_external_app_data_dir(&document_dir) {
                    tracing::debug!(
                        "Using Android external app data directory workaround: {:?}",
                        path
                    );
                    return Ok(path);
                }
            }
        }
    }

    Ok(app_handle.path().app_data_dir()?)
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

    if is_android_external_app_dir(&candidate) {
        Some(candidate)
    } else {
        None
    }
}

#[cfg(target_os = "android")]
fn is_android_external_app_dir(path: &Path) -> bool {
    let normalized = path.to_string_lossy().replace('\\', "/");
    normalized.contains("/Android/data/")
}
