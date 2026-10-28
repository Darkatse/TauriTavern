// Infrastructure layer - implements interfaces defined in the domain layer
pub(crate) mod agent_extension_tools;
pub mod apis;
#[cfg(any(target_os = "ios", target_os = "macos"))]
pub mod apple_webview_js_dialogs;
#[cfg(any(target_os = "ios", target_os = "macos"))]
pub mod apple_webview_refresh_rate;
pub mod assets;
pub mod builtin_skills;
pub mod bundled_resources;
pub mod ios_policy_cache;
#[cfg(target_os = "ios")]
pub mod ios_webview;
#[cfg(target_os = "ios")]
pub mod ios_webview_capture;
pub mod logging;
#[cfg(target_os = "macos")]
pub mod macos_webview;
#[cfg(target_os = "macos")]
pub mod macos_webview_capture;
pub mod paths;
pub mod persistence;
pub mod repositories;
#[cfg(desktop)]
pub mod runtime_paths_config_store;
pub mod staging;
pub mod sync_host;
#[cfg(all(target_os = "linux", not(target_env = "ohos")))]
pub mod webkitgtk_capture;
#[cfg(target_os = "windows")]
pub mod webview2_capture;
pub mod zipkit;
