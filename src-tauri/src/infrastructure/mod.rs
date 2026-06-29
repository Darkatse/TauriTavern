// Infrastructure layer - implements interfaces defined in the domain layer
pub mod apis;
#[cfg(any(target_os = "ios", target_os = "macos"))]
pub mod apple_webview_js_dialogs;
pub mod assets;
pub mod github;
pub mod host_resources;
pub mod http_client;
pub mod http_client_pool;
pub mod http_error;
pub mod ios_policy_cache;
#[cfg(target_os = "ios")]
pub mod ios_webview;
pub mod lan_sync;
pub mod logging;
#[cfg(target_os = "macos")]
pub mod macos_webview;
pub mod paths;
pub mod persistence;
pub mod preset_file_naming;
pub mod repositories;
pub mod sillytavern_sorting;
pub mod sync;
pub mod sync_automation_store;
pub mod sync_fs;
pub mod sync_transfer;
pub mod thumbnails;
pub mod tt_sync;
pub mod user_media_store;
pub mod zipkit;

#[cfg(test)]
mod platform_boundary_contract_tests;
#[cfg(test)]
mod webview_js_dialogs_contract_tests;
