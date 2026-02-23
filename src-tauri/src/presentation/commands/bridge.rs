use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{Emitter, Window};
use tauri_plugin_notification::{NotificationExt, PermissionState};

use crate::infrastructure::assets::read_resource_text;
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;

const SILLYTAVERN_COMPAT_VERSION: &str = "1.16.0";
const BUILD_GIT_REVISION: &str = env!("TAURITAVERN_GIT_REVISION");
const BUILD_GIT_BRANCH: &str = env!("TAURITAVERN_GIT_BRANCH");

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
        // Keep the upstream client-agent shape for extension compatibility checks.
        agent: format!("SillyTavern:{}:TauriTavern", SILLYTAVERN_COMPAT_VERSION),
        pkg_version: pkg_version.to_string(),
        git_revision: normalize_optional_build_value(BUILD_GIT_REVISION),
        git_branch: normalize_optional_build_value(BUILD_GIT_BRANCH),
    };

    Ok(version_info)
}

fn normalize_optional_build_value(value: &str) -> Option<String> {
    let normalized = value.trim();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized.to_string())
    }
}

#[tauri::command]
pub fn is_ready() -> Result<bool, CommandError> {
    Ok(true)
}

fn validate_resource_segment(value: &str, field: &str) -> Result<(), CommandError> {
    if value.is_empty() || value.contains('/') || value.contains('\\') || value.contains("..") {
        return Err(CommandError::BadRequest(format!(
            "Invalid {}: {}",
            field, value
        )));
    }
    Ok(())
}

/// Read a frontend template file from the bundled resources.
/// On Android, resources are stored as APK assets accessible via asset://localhost/.
/// This command uses Tauri's FsExt to handle both desktop and Android paths.
#[tauri::command]
pub fn read_frontend_template(app: tauri::AppHandle, name: String) -> Result<String, CommandError> {
    validate_resource_segment(&name, "template name")?;

    let content =
        read_resource_text(&app, &format!("frontend-templates/{}", name)).map_err(|e| match e {
            crate::domain::errors::DomainError::NotFound(message) => {
                CommandError::NotFound(message)
            }
            other => CommandError::InternalServerError(format!(
                "Failed to read template '{}': {}",
                name, other
            )),
        })?;

    Ok(content)
}

/// Read a built-in extension template file from bundled resources.
/// This is used on mobile platforms where direct fetch from asset:// may be unreliable.
#[tauri::command]
pub fn read_frontend_extension_template(
    app: tauri::AppHandle,
    extension: String,
    name: String,
) -> Result<String, CommandError> {
    validate_resource_segment(&extension, "extension")?;
    validate_resource_segment(&name, "template name")?;

    let resource_path = format!("frontend-extensions/{}/{}.html", extension, name);
    let content = read_resource_text(&app, &resource_path).map_err(|e| match e {
        crate::domain::errors::DomainError::NotFound(message) => CommandError::NotFound(message),
        other => CommandError::InternalServerError(format!(
            "Failed to read extension template '{}': {}",
            resource_path, other
        )),
    })?;

    Ok(content)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShowSystemNotificationDto {
    pub title: String,
    pub body: String,
}

fn ensure_notification_permission(app: &tauri::AppHandle) -> Result<bool, CommandError> {
    let notification = app.notification();
    let current_state = notification.permission_state().map_err(|error| {
        CommandError::InternalServerError(format!(
            "Failed to query notification permission state: {}",
            error
        ))
    })?;

    if matches!(current_state, PermissionState::Granted) {
        return Ok(true);
    }

    if matches!(current_state, PermissionState::Denied) {
        return Ok(false);
    }

    let requested_state = notification.request_permission().map_err(|error| {
        CommandError::InternalServerError(format!(
            "Failed to request notification permission: {}",
            error
        ))
    })?;

    Ok(matches!(requested_state, PermissionState::Granted))
}

#[tauri::command]
pub fn show_system_notification(
    app: tauri::AppHandle,
    dto: ShowSystemNotificationDto,
) -> Result<(), CommandError> {
    log_command("show_system_notification");

    let title = dto.title.trim();
    let body = dto.body.trim();

    if title.is_empty() && body.is_empty() {
        return Ok(());
    }

    if !ensure_notification_permission(&app)? {
        return Ok(());
    }

    app.notification()
        .builder()
        .title(title)
        .body(body)
        .show()
        .map_err(|error| {
            CommandError::InternalServerError(format!(
                "Failed to show system notification: {}",
                error
            ))
        })?;

    Ok(())
}
