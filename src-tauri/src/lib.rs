mod app;
mod application;
mod domain;
mod infrastructure;
mod observability_targets;
mod platform;
mod presentation;
mod product;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    app::host::run();
}
