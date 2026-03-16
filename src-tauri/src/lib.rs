mod app;
mod application;
mod domain;
mod infrastructure;
mod presentation;

use app::spawn_initialization;
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
            create_main_window(app, third_party_dirs, user_dirs)?;
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
) -> Result<(), Box<dyn std::error::Error>> {
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

    tauri::webview::WebviewWindowBuilder::from_config(app.handle(), window_config)?
        .on_web_resource_request(move |request, response| {
            handle_third_party_asset_web_request(
                &local_extensions_dir,
                &global_extensions_dir,
                &request,
                response,
            );
            handle_thumbnail_web_request(&user_dirs, &request, response);
            handle_user_data_asset_web_request(&user_dirs, &request, response);
        })
        .build()?;

    Ok(())
}
