mod app;
mod application;
mod domain;
mod infrastructure;
mod presentation;

use app::spawn_initialization;
use infrastructure::http_client_pool::HttpClientPool;
use infrastructure::logging::logger;
use infrastructure::paths::resolve_runtime_paths;
use infrastructure::third_party_assets::ThirdPartyExtensionDirs;
use infrastructure::user_data_dirs::DefaultUserWebDirs;
use presentation::commands::registry::invoke_handler;
#[cfg(any(dev, debug_assertions))]
use presentation::web_resources::dev_protocol_endpoint::handle_dev_protocol_request;
use presentation::web_resources::third_party_endpoint::handle_third_party_asset_web_request;
use presentation::web_resources::thumbnail_endpoint::handle_thumbnail_web_request;
use presentation::web_resources::user_data_endpoint::handle_user_data_asset_web_request;
use tauri::Manager;

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
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init());

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

            let runtime_paths = resolve_runtime_paths(&app_handle)?;
            let http_client_pool = std::sync::Arc::new(HttpClientPool::new());
            app.manage(http_client_pool.clone());

            #[cfg(any(target_os = "macos", windows, target_os = "linux"))]
            install_window_state_plugin(&app_handle, &runtime_paths.data_root)?;

            if let Err(error) = logger::init_logger(&runtime_paths.log_root) {
                eprintln!("Failed to initialize logger: {}", error);
            }

            tracing::debug!("Starting TauriTavern application");

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

            let tauritavern_settings =
                load_tauritavern_settings(&runtime_paths.data_root)?;
            http_client_pool
                .apply_request_proxy_settings(&tauritavern_settings.request_proxy)?;
            let _main_window = create_main_window(app, third_party_dirs, user_dirs)?;

            #[cfg(target_os = "windows")]
            {
                let close_to_tray_on_close =
                    load_close_to_tray_on_close_setting(&runtime_paths.data_root)?;
                let tray_state = std::sync::Arc::new(
                    presentation::windows_tray::WindowsTrayState::new(
                        close_to_tray_on_close,
                    ),
                );
                presentation::windows_tray::install_windows_tray(
                    &app_handle,
                    &_main_window,
                    tray_state,
                )?;
            }

            spawn_initialization(app_handle.clone(), runtime_paths.clone());
            Ok(())
        })
        .invoke_handler(invoke_handler())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn create_main_window(
    app: &mut tauri::App,
    third_party_dirs: ThirdPartyExtensionDirs,
    user_dirs: DefaultUserWebDirs,
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

    let builder = tauri::webview::WebviewWindowBuilder::from_config(app.handle(), window_config)?
        .on_web_resource_request(move |request, response| {
            handle_third_party_asset_web_request(
                &local_extensions_dir,
                &global_extensions_dir,
                &request,
                response,
            );
            handle_thumbnail_web_request(&user_dirs, &request, response);
            handle_user_data_asset_web_request(&user_dirs, &request, response);
        });

    #[cfg(any(target_os = "macos", windows, target_os = "linux"))]
    let builder = builder.visible(false);

    let window = builder.build()?;

    #[cfg(target_os = "ios")]
    infrastructure::ios_webview::disable_wkwebview_content_inset_adjustment(&window)?;

    #[cfg(any(target_os = "macos", windows, target_os = "linux"))]
    {
        use tauri_plugin_window_state::WindowExt;

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
) -> Result<crate::domain::models::settings::TauriTavernSettings, Box<dyn std::error::Error>>
{
    let path = data_root
        .join("default-user")
        .join("tauritavern-settings.json");

    if !path.is_file() {
        return Ok(crate::domain::models::settings::TauriTavernSettings::default());
    }

    let raw = std::fs::read_to_string(&path)?;
    let settings = serde_json::from_str(&raw)?;
    Ok(settings)
}
