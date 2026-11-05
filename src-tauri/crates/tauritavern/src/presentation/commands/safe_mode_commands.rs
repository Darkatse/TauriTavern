use std::sync::Arc;

use tauri::{AppHandle, State};

use crate::app::SafeMode;
use crate::presentation::commands::helpers::log_command;
use crate::presentation::errors::CommandError;
use tt_contracts::safe_mode::SafeModePrompt;

// The page asks for these before the backend is ready, so they use no AppState.

#[tauri::command]
pub async fn get_safe_mode_prompt(
    safe_mode: State<'_, Arc<SafeMode>>,
) -> Result<Option<SafeModePrompt>, CommandError> {
    log_command("get_safe_mode_prompt");
    Ok(safe_mode.prompt())
}

#[tauri::command]
pub async fn dismiss_safe_mode_prompt(
    safe_mode: State<'_, Arc<SafeMode>>,
) -> Result<(), CommandError> {
    log_command("dismiss_safe_mode_prompt");
    let safe_mode = safe_mode.inner().clone();
    tauri::async_runtime::spawn_blocking(move || safe_mode.dismiss_prompt())
        .await
        .map_err(|error| CommandError::InternalServerError(error.to_string()))
}

#[tauri::command]
pub async fn request_safe_mode(
    app_handle: AppHandle,
    safe_mode: State<'_, Arc<SafeMode>>,
) -> Result<(), CommandError> {
    log_command("request_safe_mode");
    let safe_mode = safe_mode.inner().clone();
    tauri::async_runtime::spawn_blocking(move || safe_mode.request())
        .await
        .map_err(|error| CommandError::InternalServerError(error.to_string()))?
        .map_err(CommandError::from)?;
    crate::app::host::exit_for_safe_mode(&app_handle);
    Ok(())
}
