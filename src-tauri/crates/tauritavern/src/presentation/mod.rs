// Presentation layer - handles communication with the frontend
pub mod commands;
pub mod errors;
pub mod web_resources;

#[cfg(any(target_os = "macos", windows, target_os = "linux"))]
pub mod main_window_presenter;

#[cfg(target_os = "windows")]
pub mod windows_notifications;

#[cfg(target_os = "windows")]
pub mod windows_tray;
