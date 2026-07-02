//! Native Tauri plugin registration.
//!
//! This is the only place that should know which native capabilities are part of
//! the app shell. Downstream code should consume those capabilities through
//! commands, bridges, or managed state instead of installing plugins itself.

#[cfg(any(dev, debug_assertions))]
use crate::presentation::web_resources::dev_protocol_endpoint::handle_dev_protocol_request;

pub(super) fn install<R: tauri::Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    // Keep cross-platform plugins first and cfg-gated plugins local to this file.
    // Moving desktop/mobile plugins into setup would make capability availability
    // depend on runtime initialization order instead of Builder construction.
    let builder = builder
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init());

    #[cfg(any(target_os = "macos", windows, target_os = "linux"))]
    let builder = builder.plugin(tauri_plugin_dialog::init());

    #[cfg(mobile)]
    let builder = builder.plugin(tauri_plugin_barcode_scanner::init());

    #[cfg(any(dev, debug_assertions))]
    // Dev-only static-resource protocol for extension assets served outside the
    // production custom protocol path. It depends on HostResourceService being
    // managed by setup before the frontend can issue requests.
    let builder = builder.register_uri_scheme_protocol("tt-ext", move |ctx, request| {
        handle_dev_protocol_request(ctx, request)
    });

    builder
}
