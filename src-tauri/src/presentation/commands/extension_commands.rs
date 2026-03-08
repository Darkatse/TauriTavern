use std::sync::Arc;

use tauri::State;

use crate::app::AppState;
use crate::domain::models::extension::{
    Extension, ExtensionInstallResult, ExtensionUpdateResult, ExtensionVersion,
};
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;

#[tauri::command]
pub async fn get_extensions(
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<Extension>, CommandError> {
    log_command("get_extensions");

    app_state
        .extension_service
        .get_extensions()
        .await
        .map_err(map_command_error("Failed to get extensions"))
}

#[tauri::command]
pub async fn install_extension(
    url: String,
    global: bool,
    branch: Option<String>,
    app_state: State<'_, Arc<AppState>>,
) -> Result<ExtensionInstallResult, CommandError> {
    log_command(format!("install_extension {}", url));

    app_state
        .extension_service
        .install_extension(&url, global, branch)
        .await
        .map_err(map_command_error("Failed to install extension"))
}

#[tauri::command]
pub async fn update_extension(
    extension_name: String,
    global: bool,
    app_state: State<'_, Arc<AppState>>,
) -> Result<ExtensionUpdateResult, CommandError> {
    log_command(format!("update_extension {}", extension_name));

    app_state
        .extension_service
        .update_extension(&extension_name, global)
        .await
        .map_err(map_command_error("Failed to update extension"))
}

#[tauri::command]
pub async fn delete_extension(
    extension_name: String,
    global: bool,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command(format!("delete_extension {}", extension_name));

    app_state
        .extension_service
        .delete_extension(&extension_name, global)
        .await
        .map_err(map_command_error("Failed to delete extension"))
}

#[tauri::command]
pub async fn get_extension_version(
    extension_name: String,
    global: bool,
    app_state: State<'_, Arc<AppState>>,
) -> Result<ExtensionVersion, CommandError> {
    log_command(format!("get_extension_version {}", extension_name));

    app_state
        .extension_service
        .get_extension_version(&extension_name, global)
        .await
        .map_err(map_command_error("Failed to get extension version"))
}

#[tauri::command]
pub async fn move_extension(
    extension_name: String,
    source: String,
    destination: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command(format!(
        "move_extension {} from {} to {}",
        extension_name, source, destination
    ));

    app_state
        .extension_service
        .move_extension(&extension_name, &source, &destination)
        .await
        .map_err(map_command_error("Failed to move extension"))
}
