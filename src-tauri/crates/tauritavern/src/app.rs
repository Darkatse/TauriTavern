use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager};

use crate::infrastructure::paths::RuntimePaths;

pub mod backend_errors;
mod backend_readiness;
mod composition;
#[cfg(test)]
mod contract_tests;
pub mod dev_observability;
pub(crate) mod host;
mod startup_profile;
mod state;

pub(crate) use backend_readiness::BackendReadiness;
pub(crate) use composition::HostEnvironment;
pub(crate) use startup_profile::StartupProfile;
pub(crate) use state::AppServices;
pub use state::AppState;

pub fn spawn_initialization(
    app_handle: AppHandle,
    runtime_paths: RuntimePaths,
    backend_readiness: Arc<BackendReadiness>,
) {
    tauri::async_runtime::spawn(async move {
        if let Err(error) = crate::infrastructure::staging::clear_staging_root(&app_handle).await {
            tracing::warn!(%error, "Failed to clear files left by the previous session");
        }
        match initialize_app_state(&app_handle, runtime_paths).await {
            Ok(state) => {
                backend_readiness.mark_ready();

                state
                    .services
                    .settings_service
                    .schedule_chat_backup_reconciliation();

                let sync_automation_service = state.services.sync_automation_service.clone();
                let sync_automation_cancel = state.lifecycle.sync_automation_cancel.clone();
                tauri::async_runtime::spawn(async move {
                    sync_automation_service.run(sync_automation_cancel).await;
                });

                let agent_run_retention_automation_service = state
                    .services
                    .agent_run_retention_automation_service
                    .clone();
                let agent_run_retention_automation_cancel = state
                    .lifecycle
                    .agent_run_retention_automation_cancel
                    .clone();
                tauri::async_runtime::spawn(async move {
                    agent_run_retention_automation_service
                        .run(agent_run_retention_automation_cancel)
                        .await;
                });

                let chat_history_coordinator = state.services.chat_history_coordinator.clone();
                let chat_history_cancel = state.lifecycle.chat_history_cancel.clone();
                tauri::async_runtime::spawn(async move {
                    chat_history_coordinator.run(chat_history_cancel).await;
                });

                match app_handle.emit("app-ready", ()) {
                    Ok(_) => tracing::debug!("Application is ready"),
                    Err(error) => tracing::error!("Failed to emit app-ready event: {}", error),
                }
            }
            Err(message) => {
                backend_readiness.mark_failed(message.clone());
                tracing::error!(
                    target: crate::observability_targets::USER_VISIBLE_ERROR,
                    "{message}",
                );
                if let Err(error) = app_handle.emit("app-error", message) {
                    tracing::error!("Failed to emit app-error event: {}", error);
                }
            }
        }
    });
}

async fn initialize_app_state(
    app_handle: &AppHandle,
    runtime_paths: RuntimePaths,
) -> Result<Arc<AppState>, String> {
    let data_root = runtime_paths.data_root.clone();
    let startup_profile = tokio::task::spawn_blocking(move || StartupProfile::load(&data_root))
        .await
        .map_err(|error| format!("Startup settings task failed: {error}"))?
        .map_err(|error| format!("Failed to load startup settings: {error}"))?;
    host::apply_startup_profile(app_handle, &startup_profile);

    let environment = host::host_environment(app_handle);
    let state = AppState::new(environment, runtime_paths, startup_profile)
        .await
        .map(Arc::new)
        .map_err(|error| format!("Failed to initialize application state: {error}"))?;
    state
        .services
        .content_service
        .initialize_default_content("default-user")
        .await
        .map_err(|error| format!("Failed to initialize default content: {error}"))?;
    tracing::debug!("Successfully initialized default content");

    if !app_handle.manage(state.clone()) {
        return Err(
            "Failed to initialize application state: AppState is already managed".to_string(),
        );
    }
    Ok(state)
}
