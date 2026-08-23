//! Native Tauri plugin registration.
//!
//! This is the only place that should know which native capabilities are part of
//! the app shell. Downstream code should consume those capabilities through
//! commands, bridges, or managed state instead of installing plugins itself.

#[cfg(any(target_os = "macos", windows, target_os = "linux"))]
use crate::presentation::main_window_presenter::present_main_window_from_app;
#[cfg(any(dev, debug_assertions))]
use crate::presentation::web_resources::dev_protocol_endpoint::{
    dev_protocol_task_error_response, handle_dev_protocol_request,
};

pub(super) fn install<R: tauri::Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    #[cfg(any(target_os = "macos", windows, target_os = "linux"))]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
        if let Err(error) = present_main_window_from_app(app) {
            tracing::warn!("Failed to present main window for secondary instance: {error}");
        }
    }));

    // Keep the remaining cross-platform plugins together and cfg-gated plugins local to this file.
    // Moving desktop/mobile plugins into setup would make capability availability
    // depend on runtime initialization order instead of Builder construction.
    let builder = builder
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init());

    #[cfg(target_os = "android")]
    let builder = builder.plugin(crate::platform::generation_background::plugin());

    #[cfg(all(
        feature = "devtools-pilot",
        any(target_os = "macos", windows, target_os = "linux")
    ))]
    let builder = builder.plugin(tauri_plugin_pilot::init());

    #[cfg(mobile)]
    let builder = builder.plugin(tauri_plugin_barcode_scanner::init());

    #[cfg(any(dev, debug_assertions))]
    // Dev-only static-resource protocol for extension assets served outside the
    // production custom protocol path. It depends on HostResourceService being
    // managed by setup before the frontend can issue requests.
    let builder = builder.register_asynchronous_uri_scheme_protocol(
        "tt-ext",
        move |ctx, request, responder| {
            let app_handle = ctx.app_handle().clone();
            let _task = tauri::async_runtime::spawn(async move {
                let response = tauri::async_runtime::spawn_blocking(move || {
                    handle_dev_protocol_request(&app_handle, request)
                })
                .await
                .unwrap_or_else(|error| {
                    tracing::error!("Development resource task failed: {error}");
                    dev_protocol_task_error_response()
                });

                responder.respond(response);
            });
        },
    );

    builder
}
