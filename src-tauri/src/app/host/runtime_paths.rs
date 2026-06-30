//! Runtime path resolution and publication.
//!
//! Runtime paths are the first managed state published during setup. Every later
//! host subsystem reads data/log/resource locations from this one snapshot, so
//! do not resolve paths again in sibling modules.

#[cfg(any(target_os = "macos", windows, target_os = "linux"))]
use std::sync::Arc;

#[cfg(any(target_os = "macos", windows, target_os = "linux"))]
use crate::application::services::runtime_paths_service::{
    RuntimeModeInfo, RuntimePathsService, RuntimePathsSnapshot,
};
use crate::infrastructure::paths::{RuntimePaths, resolve_runtime_paths};
#[cfg(any(target_os = "macos", windows, target_os = "linux"))]
use crate::infrastructure::runtime_paths_config_store::FilesystemRuntimePathConfigStore;
use tauri::Manager;

pub(super) fn install(
    app: &mut tauri::App,
    app_handle: &tauri::AppHandle,
) -> Result<RuntimePaths, Box<dyn std::error::Error>> {
    // Resolve once, ensure startup directories, then publish the exact clone
    // consumed by commands, resource services, and AppState initialization.
    let runtime_paths = resolve_runtime_paths(app_handle)?;
    app.manage(runtime_paths.clone());

    #[cfg(any(target_os = "macos", windows, target_os = "linux"))]
    // Desktop exposes data-root migration controls through a small managed
    // service. Mobile uses the platform sandbox path directly and has no runtime
    // data-root switcher.
    app.manage(Arc::new(RuntimePathsService::new(
        runtime_paths_snapshot(&runtime_paths),
        Arc::new(FilesystemRuntimePathConfigStore),
    )));

    Ok(runtime_paths)
}

#[cfg(any(target_os = "macos", windows, target_os = "linux"))]
fn runtime_paths_snapshot(runtime_paths: &RuntimePaths) -> RuntimePathsSnapshot {
    RuntimePathsSnapshot {
        mode: match runtime_paths.mode {
            crate::infrastructure::paths::RuntimeMode::Standard => RuntimeModeInfo::Standard,
            crate::infrastructure::paths::RuntimeMode::Portable => RuntimeModeInfo::Portable,
        },
        app_root: runtime_paths.app_root.clone(),
        data_root: runtime_paths.data_root.clone(),
    }
}
