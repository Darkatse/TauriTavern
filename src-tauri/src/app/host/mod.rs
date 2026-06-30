//! Tauri host shell composition root.
//!
//! Keep this module at the framework edge: plugin registration, setup sequencing,
//! invoke handler wiring, and shutdown hooks. Application services are still built
//! in `app::bootstrap`; frontend-visible behavior is still owned by presentation
//! commands and web resource adapters.

mod observability;
mod plugins;
mod resources;
mod runtime_paths;
mod setup;
mod shutdown;
mod window;

use crate::presentation::commands::registry::invoke_handler;

pub(crate) fn run() {
    // Builder order is part of the host contract: install native capabilities,
    // run setup to publish managed state and create the window, then expose the
    // fixed command registry.
    plugins::install(tauri::Builder::default())
        .setup(setup::setup)
        .invoke_handler(invoke_handler())
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(shutdown::handle_run_event);
}
