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
pub(crate) use startup_profile::StartupProfile;
pub(crate) use state::AppServices;
pub use state::AppState;

pub fn spawn_initialization(
    app_handle: AppHandle,
    runtime_paths: RuntimePaths,
    startup_profile: StartupProfile,
    backend_readiness: Arc<BackendReadiness>,
) {
    tauri::async_runtime::spawn(async move {
        match AppState::new(app_handle.clone(), runtime_paths, startup_profile).await {
            Ok(state) => {
                let state = Arc::new(state);
                if !app_handle.manage(state.clone()) {
                    let message =
                        "Failed to initialize application state: AppState is already managed"
                            .to_string();
                    backend_readiness.mark_failed(message.clone());
                    tracing::error!(
                        target: crate::observability_targets::USER_VISIBLE_ERROR,
                        "{message}",
                    );
                    match app_handle.emit("app-error", message) {
                        Ok(_) => {}
                        Err(emit_error) => {
                            tracing::error!("Failed to emit app-error event: {}", emit_error);
                        }
                    }
                    return;
                }

                backend_readiness.mark_ready();

                match state
                    .services
                    .content_service
                    .initialize_default_content("default-user")
                    .await
                {
                    Ok(_) => tracing::debug!("Successfully initialized default content"),
                    Err(error) => tracing::warn!("Failed to initialize default content: {}", error),
                }

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

                match app_handle.emit("app-ready", ()) {
                    Ok(_) => tracing::debug!("Application is ready"),
                    Err(error) => tracing::error!("Failed to emit app-ready event: {}", error),
                }
            }
            Err(error) => {
                let message = format!("Failed to initialize application state: {}", error);
                backend_readiness.mark_failed(message.clone());
                tracing::error!(
                    target: crate::observability_targets::USER_VISIBLE_ERROR,
                    "{message}",
                );

                match app_handle.emit("app-error", error.to_string()) {
                    Ok(_) => {}
                    Err(emit_error) => {
                        tracing::error!("Failed to emit app-error event: {}", emit_error);
                    }
                }
            }
        }
    });
}
