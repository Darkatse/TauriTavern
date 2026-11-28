mod app;
mod infrastructure;
mod observability_targets;
#[cfg(feature = "perf")]
pub mod perf;
mod platform;
mod presentation;
mod product;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    app::host::run();
}
