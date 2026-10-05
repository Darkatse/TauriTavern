use crate::{platform::window_layout, presentation::errors::CommandError};
use tauri::AppHandle;
use tt_contracts::window_layout::{WindowBackdropRequest, WindowSnapshot};

#[tauri::command]
pub async fn get_window_snapshot(app: AppHandle) -> Result<Option<WindowSnapshot>, CommandError> {
    window_layout::snapshot(&app).await.map_err(Into::into)
}

#[tauri::command]
pub async fn set_window_backdrop(
    app: AppHandle,
    request: WindowBackdropRequest,
) -> Result<(), CommandError> {
    // Publication errors reach the caller; wallpaper failure leaves the accepted native color.
    window_layout::set_backdrop(&app, request)
        .await
        .map_err(Into::into)
}
