use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{Emitter, Window};
use tauri_plugin_notification::{NotificationExt, PermissionState};

use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;
#[cfg(any(dev, debug_assertions))]
use crate::presentation::web_resources::tauri_resource_adapter::serve_dev_ipc_resource_from_app;

const SILLYTAVERN_COMPAT_VERSION: &str = "1.18.0";
use tt_domain::models::update::UpdateChannel;

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
    #[serde(rename = "tauriVersion")]
    pub tauri_version: String,
    #[serde(rename = "gitRevision")]
    pub git_revision: Option<String>,
    #[serde(rename = "gitBranch")]
    pub git_branch: Option<String>,
    #[serde(rename = "defaultUpdateChannel")]
    pub default_update_channel: UpdateChannel,
}

#[tauri::command]
pub fn get_version() -> Result<String, CommandError> {
    Ok(crate::product::VERSION.to_string())
}

#[tauri::command]
pub fn get_client_version() -> Result<VersionInfo, CommandError> {
    log_command("get_client_version");

    let version_info = VersionInfo {
        // Keep the upstream client-agent shape for extension compatibility checks.
        agent: format!("SillyTavern:{}:TauriTavern", SILLYTAVERN_COMPAT_VERSION),
        // Most upstream extensions parse pkgVersion as the SillyTavern SemVer.
        // Keep it aligned with the embedded frontend baseline to preserve plugin behavior.
        pkg_version: SILLYTAVERN_COMPAT_VERSION.to_string(),
        tauri_version: crate::product::VERSION.to_string(),
        git_revision: crate::product::optional_build_value(crate::product::GIT_REVISION)
            .map(str::to_string),
        git_branch: crate::product::optional_build_value(crate::product::GIT_BRANCH)
            .map(str::to_string),
        default_update_channel: crate::product::default_update_channel(),
    };

    Ok(version_info)
}

#[tauri::command]
pub fn is_ready() -> Result<bool, CommandError> {
    Ok(true)
}

#[cfg(any(dev, debug_assertions))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DevWebResourceRequest {
    pub pathname: String,
    pub search: Option<String>,
    pub method: Option<String>,
    pub headers: Vec<(String, String)>,
}

#[cfg(any(dev, debug_assertions))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DevWebResourceResponse {
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

#[cfg(any(dev, debug_assertions))]
#[tauri::command]
pub fn read_dev_web_resource(
    app: tauri::AppHandle,
    request: DevWebResourceRequest,
) -> Result<DevWebResourceResponse, CommandError> {
    let request = build_dev_web_resource_request(request)?;
    let response = serve_dev_ipc_resource_from_app(&app, &request);
    let (parts, body) = response.into_parts();
    let headers = parts
        .headers
        .iter()
        .map(|(name, value)| {
            value
                .to_str()
                .map(|value| (name.as_str().to_string(), value.to_string()))
                .map_err(|error| CommandError::InternalServerError(error.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(DevWebResourceResponse {
        status: parts.status.as_u16(),
        status_text: parts
            .status
            .canonical_reason()
            .unwrap_or_default()
            .to_string(),
        headers,
        body,
    })
}

#[cfg(any(dev, debug_assertions))]
fn build_dev_web_resource_request(
    request: DevWebResourceRequest,
) -> Result<tauri::http::Request<Vec<u8>>, CommandError> {
    let method = request.method.unwrap_or_else(|| "GET".to_string());
    let uri = format!("{}{}", request.pathname, request.search.unwrap_or_default());
    let mut builder = tauri::http::Request::builder()
        .method(method.as_str())
        .uri(uri);
    for (name, value) in request.headers {
        builder = builder.header(name, value);
    }

    builder
        .body(Vec::new())
        .map_err(|error| CommandError::BadRequest(error.to_string()))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShowSystemNotificationDto {
    pub title: String,
    pub body: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationPermissionStateDto {
    Granted,
    Denied,
    Prompt,
}

fn normalize_notification_permission_state(
    state: PermissionState,
) -> NotificationPermissionStateDto {
    match state {
        PermissionState::Granted => NotificationPermissionStateDto::Granted,
        PermissionState::Denied => NotificationPermissionStateDto::Denied,
        PermissionState::Prompt | PermissionState::PromptWithRationale => {
            NotificationPermissionStateDto::Prompt
        }
    }
}

fn get_notification_permission_state_inner(
    app: &tauri::AppHandle,
) -> Result<NotificationPermissionStateDto, CommandError> {
    let notification = app.notification();
    let current_state = notification.permission_state().map_err(|error| {
        CommandError::InternalServerError(format!(
            "Failed to query notification permission state: {}",
            error
        ))
    })?;

    Ok(normalize_notification_permission_state(current_state))
}

#[tauri::command]
pub fn get_notification_permission_state(
    app: tauri::AppHandle,
) -> Result<NotificationPermissionStateDto, CommandError> {
    log_command("get_notification_permission_state");
    get_notification_permission_state_inner(&app)
}

#[tauri::command]
pub fn request_notification_permission(
    app: tauri::AppHandle,
) -> Result<NotificationPermissionStateDto, CommandError> {
    log_command("request_notification_permission");

    if !matches!(
        get_notification_permission_state_inner(&app)?,
        NotificationPermissionStateDto::Prompt
    ) {
        return get_notification_permission_state_inner(&app);
    }

    let requested_state = app.notification().request_permission().map_err(|error| {
        CommandError::InternalServerError(format!(
            "Failed to request notification permission: {}",
            error
        ))
    })?;

    Ok(normalize_notification_permission_state(requested_state))
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
        return Err(CommandError::BadRequest(
            "Notification title and body cannot both be empty".to_string(),
        ));
    }

    if !matches!(
        get_notification_permission_state_inner(&app)?,
        NotificationPermissionStateDto::Granted
    ) {
        return Err(CommandError::Unauthorized(
            "Notification permission is not granted".to_string(),
        ));
    }

    #[cfg(target_os = "windows")]
    crate::presentation::windows_notifications::show_system_notification(&app, title, body)
        .map_err(|error| {
            CommandError::InternalServerError(format!(
                "Failed to show system notification: {}",
                error
            ))
        })?;

    #[cfg(not(target_os = "windows"))]
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

#[cfg(test)]
mod tests {
    use tauri::http::header::{IF_MODIFIED_SINCE, IF_NONE_MATCH, IF_RANGE, RANGE};

    use super::*;

    #[test]
    fn dev_web_resource_wire_request_preserves_http_headers_and_query() {
        let request = build_dev_web_resource_request(DevWebResourceRequest {
            pathname: "/backgrounds/a.mp4".to_string(),
            search: Some("?v=1".to_string()),
            method: Some("GET".to_string()),
            headers: vec![
                ("range".to_string(), "bytes=0-1".to_string()),
                ("range".to_string(), "bytes=2-3".to_string()),
                ("if-range".to_string(), "\"revision\"".to_string()),
                ("if-none-match".to_string(), "W/\"revision\"".to_string()),
                (
                    "if-modified-since".to_string(),
                    "Tue, 15 Nov 1994 12:45:26 GMT".to_string(),
                ),
            ],
        })
        .expect("request");

        assert_eq!(request.uri(), "/backgrounds/a.mp4?v=1");
        assert_eq!(request.headers().get_all(RANGE).iter().count(), 2);
        assert_eq!(request.headers()[IF_RANGE], "\"revision\"");
        assert_eq!(request.headers()[IF_NONE_MATCH], "W/\"revision\"");
        assert_eq!(
            request.headers()[IF_MODIFIED_SINCE],
            "Tue, 15 Nov 1994 12:45:26 GMT"
        );
    }

    #[test]
    fn dev_web_resource_wire_request_rejects_invalid_headers() {
        let result = build_dev_web_resource_request(DevWebResourceRequest {
            pathname: "/characters/a.png".to_string(),
            search: None,
            method: None,
            headers: vec![("invalid header".to_string(), "value".to_string())],
        });

        assert!(matches!(result, Err(CommandError::BadRequest(_))));
    }
}
