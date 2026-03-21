// Presentation layer - handles communication with the frontend
pub mod commands;
pub mod errors;
pub mod web_resources;

#[cfg(target_os = "windows")]
pub mod windows_tray;
