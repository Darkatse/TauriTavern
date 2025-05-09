use std::sync::Arc;
use tauri::{State, Window};
use tauri::Emitter;
use serde::{Serialize, Deserialize};
use serde_json::Value;

use crate::infrastructure::logging::logger;
use crate::presentation::errors::CommandError;

/// Event types that can be emitted to the frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventType {
    CharacterCreated,
    CharacterUpdated,
    CharacterDeleted,
    ChatCreated,
    ChatUpdated,
    ChatDeleted,
    MessageAdded,
    UserCreated,
    UserUpdated,
    UserDeleted,
    SettingsUpdated,
    Error,
}

/// Event data structure for frontend communication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventData {
    pub event_type: EventType,
    pub data: Value,
}

/// Emit an event to the frontend
#[tauri::command]
pub fn emit_event(
    window: Window,
    event_type: EventType,
    data: Value,
) -> Result<(), CommandError> {
    logger::debug(&format!("Emitting event: {:?}", event_type));

    let event_data = EventData {
        event_type,
        data,
    };

    window.emit("tauri-event", event_data)
        .map_err(|e| {
            logger::error(&format!("Failed to emit event: {}", e));
            CommandError::InternalServerError(format!("Failed to emit event: {}", e))
        })
}

use serde_json::json;

/// Version information structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionInfo {
    pub agent: String,
    #[serde(rename = "pkgVersion")]
    pub pkg_version: String,
    #[serde(rename = "gitRevision")]
    pub git_revision: Option<String>,
    #[serde(rename = "gitBranch")]
    pub git_branch: Option<String>,
}

/// Get the application version
#[tauri::command]
pub fn get_version() -> Result<String, CommandError> {
    let version = env!("CARGO_PKG_VERSION");
    Ok(version.to_string())
}

/// Get detailed client version information
/// This is used by the frontend to display version information
#[tauri::command]
pub fn get_client_version() -> Result<VersionInfo, CommandError> {
    logger::debug("Getting client version information");

    // Get the package version from Cargo.toml
    let pkg_version = env!("CARGO_PKG_VERSION");

    // Create a version info object
    // Note: We're using fixed values for git information since Tauri apps
    // might not have access to git commands when packaged
    let version_info = VersionInfo {
        agent: format!("TauriTavern/{}", pkg_version),
        pkg_version: pkg_version.to_string(),
        git_revision: Some("tauri".to_string()),  // Fixed value for Tauri builds
        git_branch: Some("main".to_string()),     // Fixed value for Tauri builds
    };

    logger::debug(&format!("Client version: {:?}", version_info));

    Ok(version_info)
}

/// Check if the backend is ready
#[tauri::command]
pub fn is_ready() -> Result<bool, CommandError> {
    Ok(true)
}
