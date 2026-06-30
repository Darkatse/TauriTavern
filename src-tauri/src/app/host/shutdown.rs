//! Host shutdown hooks.
//!
//! Keep this non-blocking unless the app gains an explicit graceful-shutdown
//! protocol. Current behavior matches the original shell: request cancellation
//! and spawn best-effort LAN shutdown on process exit.

use std::sync::Arc;

use crate::app::AppState;
use tauri::Manager;

pub(super) fn handle_run_event(app_handle: &tauri::AppHandle, event: tauri::RunEvent) {
    // AppState may not exist if startup failed or the user exits during async
    // initialization, so this must remain `try_state` rather than `state`.
    if matches!(
        event,
        tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit
    ) && let Some(state) = app_handle.try_state::<Arc<AppState>>()
    {
        // Cancellation tokens stop long-running automation loops; LAN shutdown
        // remains best-effort and asynchronous, preserving existing exit timing.
        state.sync_automation_cancel.cancel();
        state.agent_run_retention_automation_cancel.cancel();
        let lan_sync_service = state.lan_sync_service.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(error) = lan_sync_service.shutdown().await {
                tracing::warn!("Failed to shut down LAN Sync cleanly: {}", error);
            }
        });
    }
}
