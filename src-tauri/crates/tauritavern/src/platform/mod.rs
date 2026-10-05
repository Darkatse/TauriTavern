#[cfg(target_os = "android")]
mod android_window_layout;
pub mod file_transfer;
pub mod generation_background;
pub mod identity;
#[cfg(target_os = "ios")]
pub mod ios_document_picker;
#[cfg(target_os = "ios")]
pub mod ios_share_sheet;
#[cfg(target_os = "ios")]
pub mod ios_ui;
#[cfg(target_os = "ios")]
pub mod ios_window_layout;
pub mod ipc;
pub mod lan_discovery;
#[cfg(target_os = "android")]
pub mod speech_synthesis;
pub mod window_layout;
