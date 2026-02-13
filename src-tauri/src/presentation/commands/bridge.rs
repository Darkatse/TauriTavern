use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{Emitter, Window};

use crate::infrastructure::assets::read_resource_text;
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventData {
    pub event_type: EventType,
    pub data: Value,
}

#[tauri::command]
pub fn emit_event(window: Window, event_type: EventType, data: Value) -> Result<(), CommandError> {
    log_command(format!("emit_event {:?}", event_type));

    let event_data = EventData { event_type, data };
    window
        .emit("tauri-event", event_data)
        .map_err(map_command_error("Failed to emit event"))
}

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

#[tauri::command]
pub fn get_version() -> Result<String, CommandError> {
    Ok(env!("CARGO_PKG_VERSION").to_string())
}

#[tauri::command]
pub fn get_client_version() -> Result<VersionInfo, CommandError> {
    log_command("get_client_version");

    let pkg_version = env!("CARGO_PKG_VERSION");
    let version_info = VersionInfo {
        agent: format!("TauriTavern/{}", pkg_version),
        pkg_version: pkg_version.to_string(),
        git_revision: Some("tauri".to_string()),
        git_branch: Some("main".to_string()),
    };

    Ok(version_info)
}

#[tauri::command]
pub fn is_ready() -> Result<bool, CommandError> {
    Ok(true)
}

/// Read a frontend template file from the bundled resources.
/// On Android, resources are stored as APK assets accessible via asset://localhost/.
/// This command uses Tauri's FsExt to handle both desktop and Android paths.
#[tauri::command]
pub fn read_frontend_template(app: tauri::AppHandle, name: String) -> Result<String, CommandError> {
    // Sanitize: only allow simple filenames (no path separators or ..)
    if name.contains('/') || name.contains('\\') || name.contains("..") || name.is_empty() {
        return Err(CommandError::BadRequest(format!(
            "Invalid template name: {}",
            name
        )));
    }

    let content = read_resource_text(&app, &format!("frontend-templates/{}", name)).map_err(|e| {
        match e {
            crate::domain::errors::DomainError::NotFound(message) => CommandError::NotFound(message),
            other => CommandError::InternalServerError(format!(
                "Failed to read template '{}': {}",
                name, other
            )),
        }
    })?;

    Ok(content)
}
