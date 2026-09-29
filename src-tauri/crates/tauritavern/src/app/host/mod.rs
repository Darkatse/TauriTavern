//! Tauri host shell composition root.
//!
//! Keep this module at the framework edge: plugin registration, setup sequencing,
//! invoke handler wiring, and shutdown hooks. Application services are still built
//! in `app::composition`; frontend-visible behavior is still owned by presentation
//! commands and web resource adapters.

mod observability;
mod plugins;
mod resources;
mod runtime_paths;
mod setup;
mod shutdown;
mod window;

use std::sync::Arc;

use tauri::Manager;

use crate::infrastructure::agent_extension_tools::AgentExtensionTools;
use crate::presentation::commands::registry::invoke_handler;

#[cfg(target_os = "windows")]
pub(crate) use shutdown::request_frontend_shutdown;

pub(crate) fn run() {
    // Builder order is part of the host contract: install native capabilities,
    // run setup to publish managed state and create the window, then expose the
    // fixed command registry.
    let builder = tauri::Builder::default();
    #[cfg(target_env = "ohos")]
    let builder = {
        // The experimental runtime takes APP during build; capture filesDir first.
        let ability = tauri::ohos::APP.lock().expect("OHOS Ability lock poisoned");
        let root = ability.as_ref().and_then(|app| app.base_path())
            .map(std::path::PathBuf::from)
            .filter(|path| path.is_absolute())
            .expect("OHOS did not provide an absolute private files directory");
        builder.manage(crate::infrastructure::paths::OhosDataDirectory(root))
    };
    plugins::install(builder)
        .setup(setup::setup)
        .invoke_handler(invoke_handler())
        .on_page_load(|webview, payload| {
            if payload.event() == tauri::webview::PageLoadEvent::Started {
                if webview.label() == "main" {
                    webview
                        .state::<Arc<AgentExtensionTools>>()
                        .clear_page(webview.label());
                }
                crate::presentation::commands::chat_swipe_commands::close_page_chat_resources(
                    webview,
                );
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(shutdown::handle_run_event);
}
