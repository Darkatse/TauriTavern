mod app;
mod application;
mod domain;
mod infrastructure;
mod presentation;

// This module is the Tauri host composition root.
// Keep only shell wiring here: plugin registration, managed state, window policy,
// and startup sequencing. If code here starts knowing feature/business rules,
// move it down into app/application/presentation instead of growing lib.rs further.

use app::spawn_initialization;
use infrastructure::http_client_pool::HttpClientPool;
use infrastructure::logging::{devtools, llm_api_logs, logger};
use infrastructure::paths::resolve_runtime_paths;
use infrastructure::third_party_assets::ThirdPartyExtensionDirs;
use infrastructure::user_data_dirs::DefaultUserWebDirs;
use presentation::commands::registry::invoke_handler;
#[cfg(any(dev, debug_assertions))]
use presentation::web_resources::dev_protocol_endpoint::handle_dev_protocol_request;
use presentation::web_resources::third_party_endpoint::handle_third_party_asset_web_request;
use presentation::web_resources::thumbnail_endpoint::{
    ThumbnailEndpointPolicy, handle_thumbnail_web_request,
};
use presentation::web_resources::user_data_endpoint::handle_user_data_asset_web_request;
use tauri::Manager;
#[cfg(any(target_os = "macos", windows, target_os = "linux"))]
use tauri_plugin_opener::OpenerExt;

#[cfg(any(target_os = "macos", windows, target_os = "linux"))]
fn desktop_window_state_flags() -> tauri_plugin_window_state::StateFlags {
    use tauri_plugin_window_state::StateFlags;

    StateFlags::SIZE | StateFlags::POSITION | StateFlags::MAXIMIZED
}

#[cfg(any(target_os = "macos", windows, target_os = "linux"))]
fn install_window_state_plugin(
    app_handle: &tauri::AppHandle,
    data_root: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    // Window geometry persistence is a desktop shell concern.
    // Keep only host-managed state here; product/user settings still belong in the
    // regular settings model so window policy does not leak into domain logic.
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub async fn run() {
    // Register cross-platform host plugins up front.
    // This is the only place that should know which native capabilities are part of
    // the app shell; downstream layers consume them through commands/bridges.
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init());

    #[cfg(any(target_os = "macos", windows, target_os = "linux"))]
    let builder = builder.plugin(tauri_plugin_dialog::init());

    #[cfg(mobile)]
    let builder = builder.plugin(tauri_plugin_barcode_scanner::init());

    #[cfg(any(dev, debug_assertions))]
    let builder = builder.register_uri_scheme_protocol("tt-ext", move |ctx, request| {
        handle_dev_protocol_request(ctx, request)
    });

    builder
        .setup(move |app| {
            let app_handle = app.handle().clone();
            logger::bind_app_handle(app_handle.clone());

            // Resolve and publish runtime paths before any managed service is created so every
            // host-facing subsystem reads from the same directory layout.
            let runtime_paths = resolve_runtime_paths(&app_handle)?;
            app.manage(runtime_paths.clone());

            if let Err(error) = devtools::purge_old_log_files(
                &runtime_paths.log_root,
                std::time::Duration::from_secs(14 * 24 * 60 * 60),
            ) {
                eprintln!(
                    "Failed to purge old log files in {:?}: {}",
                    runtime_paths.log_root, error
                );
            }

            let http_client_pool = std::sync::Arc::new(HttpClientPool::new());
            app.manage(http_client_pool.clone());

            #[cfg(any(target_os = "macos", windows, target_os = "linux"))]
            install_window_state_plugin(&app_handle, &runtime_paths.data_root)?;

            // These stores are shell-level observability sinks. They stay in the host so
            // frontend tooling and backend commands can share one source of truth for logs
            // without teaching feature code about window/event plumbing.
            let backend_log_store =
                std::sync::Arc::new(devtools::BackendLogStore::new(app_handle.clone()));
            app.manage(backend_log_store.clone());

            let llm_api_log_store = std::sync::Arc::new(llm_api_logs::LlmApiLogStore::new(
                app_handle.clone(),
                runtime_paths.log_root.clone(),
            ));
            app.manage(llm_api_log_store.clone());

            if let Err(error) =
                logger::init_logger(&runtime_paths.log_root, Some(backend_log_store))
            {
                eprintln!("Failed to initialize logger: {}", error);
            }

            tracing::debug!("Starting TauriTavern application");

            // Custom web resource handlers below serve files from the runtime data root through
            // normal browser URLs. The scope extension keeps that policy centralized in the host
            // instead of leaking Tauri-specific file access into frontend/upstream code.
            if let Err(error) = app_handle
                .asset_protocol_scope()
                .allow_directory(&runtime_paths.data_root, true)
            {
                tracing::warn!(
                    "Failed to extend asset protocol scope for {:?}: {}",
                    runtime_paths.data_root,
                    error
                );
            }

            let third_party_dirs =
                ThirdPartyExtensionDirs::from_data_root(&runtime_paths.data_root);
            let user_dirs = DefaultUserWebDirs::from_data_root(&runtime_paths.data_root);
            app.manage(third_party_dirs.clone());
            app.manage(user_dirs.clone());

            let tauritavern_settings = load_tauritavern_settings(&runtime_paths.data_root)?;
            let thumbnail_policy = std::sync::Arc::new(ThumbnailEndpointPolicy::new(
                tauritavern_settings.avatar_persona_original_images_enabled,
            ));
            app.manage(thumbnail_policy.clone());
            http_client_pool.apply_request_proxy_settings(&tauritavern_settings.request_proxy)?;
            llm_api_log_store.apply_settings(tauritavern_settings.dev.effective_llm_api_keep());
            let _main_window =
                create_main_window(app, third_party_dirs, user_dirs, thumbnail_policy)?;

            #[cfg(target_os = "windows")]
            {
                let close_to_tray_on_close =
                    load_close_to_tray_on_close_setting(&runtime_paths.data_root)?;
                let tray_state = std::sync::Arc::new(
                    presentation::windows_tray::WindowsTrayState::new(close_to_tray_on_close),
                );
                presentation::windows_tray::install_windows_tray(
                    &app_handle,
                    &_main_window,
                    tray_state,
                )?;
            }

            // Heavy app state initialization is spawned after the shell is ready so window
            // creation and host plumbing stay responsive. The async initializer emits the
            // readiness/error events consumed by the frontend bootstrap.
            spawn_initialization(app_handle.clone(), runtime_paths.clone());
            Ok(())
        })
        .invoke_handler(invoke_handler())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Builds the main webview window and attaches host-owned browser/runtime policy.
///
/// Keep this function focused on shell concerns:
/// - resource URL interception for local/runtime-backed assets
/// - `window.open()` policy and popup/external-link routing
/// - platform-specific window presentation details
///
/// Do not move feature behavior here; frontend and command layers should continue to observe
/// browser-like contracts without depending on Tauri window APIs.
fn create_main_window(
    app: &mut tauri::App,
    third_party_dirs: ThirdPartyExtensionDirs,
    user_dirs: DefaultUserWebDirs,
    thumbnail_policy: std::sync::Arc<ThumbnailEndpointPolicy>,
) -> Result<tauri::webview::WebviewWindow, Box<dyn std::error::Error>> {
    let window_config = app
        .config()
        .app
        .windows
        .iter()
        .find(|config| config.label == "main")
        .expect("Main window config with label 'main' is missing");

    let local_extensions_dir = third_party_dirs.local_dir;
    let global_extensions_dir = third_party_dirs.global_dir;
    let user_dirs = user_dirs;
    let thumbnail_policy = thumbnail_policy;

    let builder = tauri::webview::WebviewWindowBuilder::from_config(app.handle(), window_config)?
        // Route browser-visible URLs to host-owned file handlers here so the frontend can keep
        // using stable HTTP-like paths for extensions, thumbnails, and user data assets.
        .on_web_resource_request(move |request, response| {
            handle_third_party_asset_web_request(
                &local_extensions_dir,
                &global_extensions_dir,
                &request,
                response,
            );
            handle_thumbnail_web_request(&user_dirs, &thumbnail_policy, &request, response);
            handle_user_data_asset_web_request(&user_dirs, &request, response);
        });

    #[cfg(any(target_os = "macos", windows, target_os = "linux"))]
    let builder = {
        let app_handle = app.handle().clone();

        // `window.open()` semantics belong to the host/runtime boundary, not to upstream JS.
        // We keep OAuth-style popups inside the app to preserve opener/postMessage behavior,
        // and hand ordinary external links to the operating system.
        builder.on_new_window(move |url, features| {
            let is_popup = features.size().is_some() || features.position().is_some();

            if is_popup {
                // Popups intentionally receive fresh labels and inherit only the opener-related
                // webview features required by Tauri/Wry. That keeps popup policy centralized
                // here instead of spreading per-extension window logic through the frontend.
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
    // Desktop windows start hidden so restored size/position can be applied before first paint.
    let builder = builder.visible(false);

    let window = builder.build()?;

    #[cfg(target_os = "ios")]
    infrastructure::ios_webview::configure_main_wkwebview(&window)?;

    #[cfg(any(target_os = "macos", windows, target_os = "linux"))]
    {
        use tauri_plugin_window_state::WindowExt;

        // Restore persisted desktop geometry only after the window exists, then reveal/focus it.
        let flags = desktop_window_state_flags();
        window.restore_state(flags)?;
        window.show()?;
        window.set_focus()?;
    }

    Ok(window)
}

#[cfg(target_os = "windows")]
fn load_close_to_tray_on_close_setting(
    data_root: &std::path::Path,
) -> Result<bool, Box<dyn std::error::Error>> {
    let settings = load_tauritavern_settings(data_root)?;
    Ok(settings.close_to_tray_on_close)
}

fn load_tauritavern_settings(
    data_root: &std::path::Path,
) -> Result<crate::domain::models::settings::TauriTavernSettings, Box<dyn std::error::Error>> {
    let path = data_root
        .join("default-user")
        .join("tauritavern-settings.json");

    if !path.is_file() {
        return Ok(crate::domain::models::settings::TauriTavernSettings::default());
    }

    let raw = std::fs::read_to_string(&path)?;
    let settings =
        crate::domain::models::settings::TauriTavernSettings::from_json_str_with_compat(&raw)?;
    Ok(settings)
}
