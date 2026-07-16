//! Host shutdown hooks.

use std::sync::Arc;

use crate::app::AppState;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use tauri::WindowEvent;
use tauri::{Emitter, Manager};

const MAIN_WINDOW_LABEL: &str = "main";
const FRONTEND_SHUTDOWN_EVENT: &str = "tauritavern-graceful-exit-requested";

pub(crate) fn request_frontend_shutdown(
    window: &tauri::webview::WebviewWindow,
) -> tauri::Result<()> {
    window.emit(FRONTEND_SHUTDOWN_EVENT, ())
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(super) fn install_window_close_handler(window: &tauri::webview::WebviewWindow) {
    let window_for_close = window.clone();
    window.on_window_event(move |event| {
        let WindowEvent::CloseRequested { api, .. } = event else {
            return;
        };

        match request_frontend_shutdown(&window_for_close) {
            Ok(()) => api.prevent_close(),
            Err(error) => tracing::error!("Failed to request graceful shutdown: {error}"),
        }
    });
}

pub(super) fn handle_run_event(app_handle: &tauri::AppHandle, event: tauri::RunEvent) {
    if let tauri::RunEvent::ExitRequested {
        code: None, api, ..
    } = &event
        && let Some(window) = app_handle.get_webview_window(MAIN_WINDOW_LABEL)
    {
        match request_frontend_shutdown(&window) {
            Ok(()) => {
                api.prevent_exit();
                return;
            }
            Err(error) => tracing::error!("Failed to request graceful shutdown: {error}"),
        }
    }

    // AppState may not exist if startup failed or the user exits during async
    // initialization, so this must remain `try_state` rather than `state`.
    if matches!(
        event,
        tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit
    ) && let Some(state) = app_handle.try_state::<Arc<AppState>>()
    {
        // Cancellation tokens stop long-running automation loops; LAN shutdown
        // remains best-effort and asynchronous, preserving existing exit timing.
        state.lifecycle.sync_automation_cancel.cancel();
        state
            .lifecycle
            .agent_run_retention_automation_cancel
            .cancel();
        state.lifecycle.chat_history_cancel.cancel();
        let lan_sync_service = state.services.lan_sync_service.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(error) = lan_sync_service.shutdown().await {
                tracing::warn!("Failed to shut down LAN Sync cleanly: {}", error);
            }
        });
    }
}
