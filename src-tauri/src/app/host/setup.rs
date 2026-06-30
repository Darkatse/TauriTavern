//! Host setup sequencing.
//!
//! This is the readable startup script for the native shell. It intentionally
//! stays explicit instead of using a DI container: startup order is part of the
//! Tauri/frontend contract and should be easy to audit in one pass.

use std::sync::Arc;

use crate::app::{StartupProfile, spawn_initialization};
use crate::domain::errors::DomainError;
use crate::domain::ios_policy::IosPolicyScope;
use crate::infrastructure::http_client_pool::HttpClientPool;
use crate::infrastructure::logging::llm_api_logs::LlmApiLogStore;
use tauri::Manager;

pub(super) fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let app_handle = app.handle().clone();

    // 1. Publish runtime paths first. Every later host/app subsystem must derive
    // paths from this snapshot rather than resolving independently.
    let runtime_paths = super::runtime_paths::install(app, &app_handle)?;
    super::observability::purge_old_logs(&runtime_paths);

    // 2. Publish lightweight host services that do not depend on user settings.
    // AppState construction later reuses the same HTTP pool via managed state.
    let http_client_pool = Arc::new(HttpClientPool::new());
    app.manage(http_client_pool.clone());
    super::resources::install_bundled_templates(app, &app_handle);

    #[cfg(any(target_os = "macos", windows, target_os = "linux"))]
    // Desktop window-state needs the resolved data root and must be installed
    // before the main window exists. The main window restores state manually.
    super::window::install_window_state_plugin(&app_handle, &runtime_paths.data_root)?;

    // 3. Bring up observability before emitting user-visible startup diagnostics.
    let observability = super::observability::install(app, &app_handle, &runtime_paths)?;

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    super::observability::emit_pending_runtime_migration_error(&runtime_paths);

    tracing::debug!("Starting TauriTavern application");

    // 4. Load the one startup settings/policy snapshot. Runtime settings changes
    // still go through SettingsService; startup-only effects use this snapshot.
    let startup_profile = StartupProfile::load(&runtime_paths.data_root)?;
    apply_startup_profile(
        &startup_profile,
        &http_client_pool,
        &observability.llm_api_logs,
    )?;

    // 5. Resource state depends on StartupProfile settings, and must be managed
    // before the first webview can request browser-visible assets.
    let host_resource_service = super::resources::install_runtime_resources(
        app,
        &app_handle,
        &runtime_paths,
        &startup_profile,
    );
    let _main_window = super::window::create_main_window(app, host_resource_service)?;

    #[cfg(target_os = "windows")]
    // Windows tray owns close-to-tray window behavior and keeps its managed state
    // alive for runtime settings updates.
    super::window::install_windows_tray(
        &app_handle,
        &_main_window,
        startup_profile.tauritavern_settings.close_to_tray_on_close,
    )?;

    // 6. Heavy AppState construction stays off the shell critical path. The
    // initializer manages AppState and emits app-ready/app-error as before.
    spawn_initialization(app_handle, runtime_paths, startup_profile);
    Ok(())
}

fn apply_startup_profile(
    startup_profile: &StartupProfile,
    http_client_pool: &HttpClientPool,
    llm_api_log_store: &LlmApiLogStore,
) -> Result<(), Box<dyn std::error::Error>> {
    let tauritavern_settings = &startup_profile.tauritavern_settings;
    let ios_policy = &startup_profile.ios_policy;

    // Fail fast when startup settings request a host capability that the active
    // iOS policy forbids. Do not silently disable proxy settings: users should
    // see an explicit startup failure for an invalid distribution profile.
    if ios_policy.scope == IosPolicyScope::Ios
        && tauritavern_settings.request_proxy.enabled
        && !ios_policy.capabilities.network.request_proxy
    {
        return Err(Box::new(DomainError::InvalidData(
            "iOS policy disabled capability: network.request_proxy".to_string(),
        )));
    }

    // Apply startup-only runtime effects from the same settings snapshot that is
    // passed into AppState, avoiding the old double-read drift.
    http_client_pool.apply_request_proxy_settings(&tauritavern_settings.request_proxy)?;
    llm_api_log_store.apply_settings(tauritavern_settings.dev.effective_llm_api_keep());
    Ok(())
}
