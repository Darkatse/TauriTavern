//! Host setup sequencing.
//!
//! This is the readable startup script for the native shell. It intentionally
//! stays explicit instead of using a DI container: startup order is part of the
//! Tauri/frontend contract and should be easy to audit in one pass.
//!
//! Setup never fails because of user data. Settings and policy derived from it
//! load in backend initialization, whose failures reach the page through
//! `BackendReadiness` instead of aborting the shell before a window exists.

use std::sync::Arc;

use crate::app::{BackendReadiness, StartupProfile, spawn_initialization};
use crate::infrastructure::agent_extension_tools::AgentExtensionTools;
use crate::infrastructure::logging::llm_api_logs::LlmApiLogStore;
use tauri::{AppHandle, Manager};
use tt_adapter_http::HttpClientPool;
use tt_adapter_storage_core::commit_stage::PageGeneration;
use tt_application::services::host_resource_service::HostResourceService;
use tt_domain::errors::DomainError;
use tt_domain::ios_policy::IosPolicyScope;

pub(super) fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let app_handle = app.handle().clone();

    // 1. Publish runtime paths first. Every later host/app subsystem must derive
    // paths from this snapshot rather than resolving independently.
    let runtime_paths = super::runtime_paths::install(app, &app_handle)?;
    super::observability::purge_old_logs(&runtime_paths);

    // 2. Publish lightweight host services that do not depend on user settings.
    // AppState construction later reuses the same HTTP pool via managed state.
    app.manage(Arc::new(AgentExtensionTools::default()));
    app.manage(Arc::new(PageGeneration::default()));
    app.manage(Arc::new(HttpClientPool::new(crate::product::USER_AGENT)));
    super::resources::install_bundled_templates(app, &app_handle);

    #[cfg(desktop)]
    // Desktop window-state needs the resolved data root and must be installed
    // before the main window exists. The main window restores state manually.
    super::window::install_window_state_plugin(&app_handle, &runtime_paths.data_root)?;

    // 3. Bring up observability before emitting user-visible startup diagnostics.
    super::observability::install(app, &app_handle, &runtime_paths)?;

    #[cfg(desktop)]
    super::observability::emit_pending_runtime_migration_error(&runtime_paths);

    tracing::debug!("Starting TauriTavern application");

    let backend_readiness = Arc::new(BackendReadiness::new());
    if !app.manage(backend_readiness.clone()) {
        return Err(Box::new(DomainError::InternalError(
            "BackendReadiness state is already managed".to_string(),
        )));
    }

    // 4. Resource state must be managed before the first webview can request
    // browser-visible assets.
    let host_resource_service =
        super::resources::install_runtime_resources(app, &app_handle, &runtime_paths)?;
    let _main_window = super::window::create_main_window(app, host_resource_service)?;

    #[cfg(all(desktop, not(windows)))]
    super::shutdown::install_window_close_handler(&_main_window);

    #[cfg(target_os = "windows")]
    // Windows tray owns close-to-tray window behavior and keeps its managed state
    // alive for runtime settings updates.
    super::window::install_windows_tray(&app_handle, &_main_window)?;

    // 5. Backend initialization loads user settings and builds AppState off the
    // shell critical path.
    spawn_initialization(app_handle, runtime_paths, backend_readiness);
    Ok(())
}

/// Applies the startup snapshot to host state managed before the window existed.
/// Settings commands apply later changes.
pub(crate) fn apply_startup_profile(app_handle: &AppHandle, startup_profile: &StartupProfile) {
    let tauritavern_settings = &startup_profile.tauritavern_settings;
    let ios_policy = &startup_profile.ios_policy;
    let http_client_pool = app_handle.state::<Arc<HttpClientPool>>();

    if ios_policy.scope == IosPolicyScope::Ios
        && tauritavern_settings.request_proxy.enabled
        && !ios_policy.capabilities.network.request_proxy
    {
        http_client_pool.block_requests_for_invalid_proxy();
        tracing::error!(
            "iOS policy disables the configured request proxy; outbound HTTP is blocked until the proxy is disabled in TauriTavern Settings"
        );
    } else if let Err(error) =
        http_client_pool.apply_persisted_request_proxy_settings(&tauritavern_settings.request_proxy)
    {
        tracing::error!(
            error = %error,
            "Request proxy settings are invalid; outbound HTTP is blocked until corrected"
        );
    }

    app_handle
        .state::<Arc<LlmApiLogStore>>()
        .apply_settings(tauritavern_settings.dev.effective_llm_api_keep());
    app_handle
        .state::<Arc<HostResourceService>>()
        .set_avatar_persona_original_images_enabled(
            tauritavern_settings.avatar_persona_original_images_enabled,
        );
    #[cfg(target_os = "windows")]
    app_handle
        .state::<Arc<crate::presentation::windows_tray::WindowsTrayState>>()
        .set_close_to_tray_on_close(tauritavern_settings.close_to_tray_on_close);
}
