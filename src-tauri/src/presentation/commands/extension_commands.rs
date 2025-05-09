use std::sync::Arc;
use tauri::State;

use crate::app::AppState;
use crate::domain::models::extension::{
    Extension, ExtensionInstallResult, ExtensionUpdateResult, ExtensionVersion,
};
use crate::application::errors::ApplicationError;
use crate::infrastructure::logging::logger;
use crate::presentation::errors::CommandError;

/// Get all extensions
#[tauri::command]
pub async fn get_extensions(
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<Extension>, CommandError> {
    logger::debug("Command: get_extensions");

    app_state
        .extension_service
        .get_extensions()
        .await
        .map_err(|e| {
            logger::error(&format!("Failed to get extensions: {}", e));
            CommandError::from(ApplicationError::from(e))
        })
}

/// Install an extension from a URL
#[tauri::command]
pub async fn install_extension(
    url: String,
    global: bool,
    app_state: State<'_, Arc<AppState>>,
) -> Result<ExtensionInstallResult, CommandError> {
    logger::debug(&format!("Command: install_extension {}", url));

    app_state
        .extension_service
        .install_extension(&url, global)
        .await
        .map_err(|e| {
            logger::error(&format!("Failed to install extension: {}", e));
            CommandError::from(ApplicationError::from(e))
        })
}

/// Update an extension
#[tauri::command]
pub async fn update_extension(
    extension_name: String,
    global: bool,
    app_state: State<'_, Arc<AppState>>,
) -> Result<ExtensionUpdateResult, CommandError> {
    logger::debug(&format!("Command: update_extension {}", extension_name));

    app_state
        .extension_service
        .update_extension(&extension_name, global)
        .await
        .map_err(|e| {
            logger::error(&format!("Failed to update extension: {}", e));
            CommandError::from(ApplicationError::from(e))
        })
}

/// Delete an extension
#[tauri::command]
pub async fn delete_extension(
    extension_name: String,
    global: bool,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    logger::debug(&format!("Command: delete_extension {}", extension_name));

    app_state
        .extension_service
        .delete_extension(&extension_name, global)
        .await
        .map_err(|e| {
            logger::error(&format!("Failed to delete extension: {}", e));
            CommandError::from(ApplicationError::from(e))
        })
}

/// Get extension version information
#[tauri::command]
pub async fn get_extension_version(
    extension_name: String,
    global: bool,
    app_state: State<'_, Arc<AppState>>,
) -> Result<ExtensionVersion, CommandError> {
    logger::debug(&format!("Command: get_extension_version {}", extension_name));

    app_state
        .extension_service
        .get_extension_version(&extension_name, global)
        .await
        .map_err(|e| {
            logger::error(&format!("Failed to get extension version: {}", e));
            CommandError::from(ApplicationError::from(e))
        })
}

/// Move an extension between local and global directories
#[tauri::command]
pub async fn move_extension(
    extension_name: String,
    source: String,
    destination: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    logger::debug(&format!(
        "Command: move_extension {} from {} to {}",
        extension_name, source, destination
    ));

    app_state
        .extension_service
        .move_extension(&extension_name, &source, &destination)
        .await
        .map_err(|e| {
            logger::error(&format!("Failed to move extension: {}", e));
            CommandError::from(ApplicationError::from(e))
        })
}
