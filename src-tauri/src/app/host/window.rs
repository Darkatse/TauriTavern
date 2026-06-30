//! Main-window creation and native window policy.
//!
//! Keep browser-runtime behavior here: resource interception, popup/external-link
//! routing, window-state restore, platform WebView tuning, and Windows tray
//! integration. Feature behavior belongs in frontend/routes/commands, not in
//! native window setup.

use std::sync::Arc;

use crate::application::services::host_resource_service::HostResourceService;
use crate::presentation::web_resources::tauri_resource_adapter::handle_tauri_web_resource_request;
#[cfg(any(target_os = "macos", windows, target_os = "linux"))]
use tauri_plugin_opener::OpenerExt;

#[cfg(any(target_os = "macos", windows, target_os = "linux"))]
fn desktop_window_state_flags() -> tauri_plugin_window_state::StateFlags {
    use tauri_plugin_window_state::StateFlags;

    StateFlags::SIZE | StateFlags::POSITION | StateFlags::MAXIMIZED
}

#[cfg(any(target_os = "macos", windows, target_os = "linux"))]
pub(super) fn install_window_state_plugin(
    app_handle: &tauri::AppHandle,
    data_root: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    // Window geometry persistence is desktop shell state. Product/user settings
    // stay in the regular settings model so window policy does not leak into
    // domain logic.
    let flags = desktop_window_state_flags();
    let state_path = data_root.join("_tauritavern").join(".window-state.json");
    std::fs::create_dir_all(
        state_path
            .parent()
            .expect("Window state path must have parent directory"),
    )?;

    app_handle.plugin(
        tauri_plugin_window_state::Builder::new()
            .with_state_flags(flags)
            .with_filename(state_path.to_string_lossy())
            .skip_initial_state("main")
            .build(),
    )?;

    Ok(())
}

pub(super) fn create_main_window(
    app: &mut tauri::App,
    host_resource_service: Arc<HostResourceService>,
) -> Result<tauri::webview::WebviewWindow, Box<dyn std::error::Error>> {
    // tauri.conf.json sets `create = false`; the host creates the main window
    // manually so web-resource interception and platform policy are attached
    // before the frontend can load.
    let window_config = app
        .config()
        .app
        .windows
        .iter()
        .find(|config| config.label == "main")
        .expect("Main window config with label 'main' is missing");

    let builder = tauri::webview::WebviewWindowBuilder::from_config(app.handle(), window_config)?
        // Route browser-visible URLs to host-owned file handlers so upstream JS
        // can keep using HTTP-like paths for extensions, thumbnails, and user
        // data assets.
        .on_web_resource_request(move |request, response| {
            handle_tauri_web_resource_request(host_resource_service.as_ref(), &request, response);
        });

    #[cfg(any(target_os = "macos", windows, target_os = "linux"))]
    let builder = {
        let app_handle = app.handle().clone();

        // `window.open()` semantics belong to the host/runtime boundary. Keep
        // OAuth-style popups in-app to preserve opener/postMessage behavior, and
        // hand ordinary external links to the operating system.
        builder.on_new_window(move |url, features| {
            let is_popup = features.size().is_some() || features.position().is_some();

            if is_popup {
                // Fresh labels avoid collisions. The popup inherits only the
                // window features needed by Tauri/Wry; extension-specific window
                // behavior should not spread into frontend code.
                let label = format!("popup-{}", uuid::Uuid::new_v4());
                let title = url.host_str().unwrap_or("Authentication");

                let window = tauri::WebviewWindowBuilder::new(
                    &app_handle,
                    label,
                    tauri::WebviewUrl::External("about:blank".parse().expect("valid URL")),
                )
                .window_features(features)
                .title(title)
                .build();

                return match window {
                    Ok(window) => tauri::webview::NewWindowResponse::Create { window },
                    Err(error) => {
                        tracing::warn!("Failed to create popup window: {}", error);
                        tauri::webview::NewWindowResponse::Allow
                    }
                };
            }

            if matches!(url.scheme(), "http" | "https" | "mailto" | "tel") {
                let _ = app_handle.opener().open_url(url.as_str(), None::<String>);
                return tauri::webview::NewWindowResponse::Deny;
            }

            tauri::webview::NewWindowResponse::Allow
        })
    };

    #[cfg(any(target_os = "macos", windows, target_os = "linux"))]
    // Desktop windows start hidden so restored size/position apply before first
    // paint. Mobile platforms do not use the window-state plugin.
    let builder = builder.visible(false);

    let window = builder.build()?;

    #[cfg(target_os = "ios")]
    // iOS needs explicit WKWebView policy for safe-area/content inset behavior,
    // fullscreen media, and JS dialogs.
    crate::infrastructure::ios_webview::configure_main_wkwebview(&window)?;

    #[cfg(target_os = "macos")]
    // macOS shares the JS dialog delegate path but not the iOS inset policy.
    crate::infrastructure::macos_webview::configure_main_wkwebview(&window)?;

    #[cfg(any(target_os = "macos", windows, target_os = "linux"))]
    {
        use tauri_plugin_window_state::WindowExt;

        // Restore persisted desktop geometry only after the window exists, then
        // reveal/focus it. Reordering this causes visible first-frame jumps.
        let flags = desktop_window_state_flags();
        window.restore_state(flags)?;
        window.show()?;
        window.set_focus()?;
    }

    Ok(window)
}

#[cfg(target_os = "windows")]
pub(super) fn install_windows_tray(
    app_handle: &tauri::AppHandle,
    main_window: &tauri::webview::WebviewWindow,
    close_to_tray_on_close: bool,
) -> tauri::Result<()> {
    // Windows tray state is managed by `presentation::windows_tray` because
    // settings commands update it at runtime; host only seeds the startup value.
    let tray_state = Arc::new(crate::presentation::windows_tray::WindowsTrayState::new(
        close_to_tray_on_close,
    ));
    crate::presentation::windows_tray::install_windows_tray(app_handle, main_window, tray_state)
}
