//! Host-level observability state.
//!
//! These sinks live in the host shell because they bridge backend logs/errors to
//! frontend tooling and native files. Feature services should log through
//! tracing; they should not know about windows, events, or dev-log bundles.

use std::sync::Arc;
use std::time::Duration;

use crate::app::backend_errors::BackendErrorHub;
use crate::app::dev_observability::DevObservabilityHub;
use crate::infrastructure::logging::{devtools, llm_api_logs, tracing_runtime};
use crate::infrastructure::paths::RuntimePaths;
use tauri::Manager;

pub(super) struct ObservabilityHandles {
    pub(super) llm_api_logs: Arc<llm_api_logs::LlmApiLogStore>,
}

pub(super) fn purge_old_logs(runtime_paths: &RuntimePaths) {
    // Purge before tracing opens new appenders. Failure is intentionally
    // non-fatal: stale logs are inconvenient, but startup should continue.
    if let Err(error) = devtools::purge_old_log_files(
        &runtime_paths.log_root,
        Duration::from_secs(14 * 24 * 60 * 60),
    ) {
        eprintln!(
            "Failed to purge old log files in {:?}: {}",
            runtime_paths.log_root, error
        );
    }
}

pub(super) fn install(
    app: &mut tauri::App,
    app_handle: &tauri::AppHandle,
    runtime_paths: &RuntimePaths,
) -> Result<ObservabilityHandles, Box<dyn std::error::Error>> {
    // BackendErrorHub is managed before tracing so user-visible startup errors
    // can be queued until the frontend bridge announces readiness.
    let backend_log_store = Arc::new(devtools::BackendLogStore::new(app_handle.clone()));
    let backend_error_hub = Arc::new(BackendErrorHub::new(app_handle.clone()));
    app.manage(backend_error_hub.clone());

    // LLM API logs are both a managed command dependency and the runtime sink
    // used by chat-completion repositories built later in AppState.
    let llm_api_log_store = Arc::new(llm_api_logs::LlmApiLogStore::new(
        app_handle.clone(),
        runtime_paths.log_root.clone(),
    ));
    app.manage(llm_api_log_store.clone());
    app.manage(Arc::new(DevObservabilityHub::new(
        app_handle.clone(),
        runtime_paths.clone(),
        backend_log_store.clone(),
        llm_api_log_store.clone(),
    )));

    // The guard owns tracing writer resources; managing it keeps file/stdout
    // layers alive for the app lifetime.
    let tracing_guard =
        tracing_runtime::init_tracing(&runtime_paths.log_root, Some(backend_log_store), {
            let backend_error_hub = backend_error_hub.clone();
            Arc::new(move |message| backend_error_hub.emit_or_queue(message))
        })
        .map_err(std::io::Error::other)?;
    app.manage(tracing_guard);

    Ok(ObservabilityHandles {
        llm_api_logs: llm_api_log_store,
    })
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub(super) fn emit_pending_runtime_migration_error(runtime_paths: &RuntimePaths) {
    // Desktop migration failures should surface as user-visible backend errors.
    // Mobile has no runtime data-root migration path, so this whole check is
    // compiled out there.
    let Ok(Some(config)) =
        crate::infrastructure::paths::load_runtime_config(&runtime_paths.app_root)
    else {
        return;
    };

    let Some(error) = config.migration_error.as_deref().map(str::trim) else {
        return;
    };
    if config.migration.is_some() && !error.is_empty() {
        tracing::error!(
            target: crate::observability_targets::USER_VISIBLE_ERROR,
            "Data directory migration failed: {}",
            error
        );
    }
}
