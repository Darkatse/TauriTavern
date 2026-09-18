use std::sync::Arc;

use tauri::State;
use tt_contracts::database::{DatabaseRequest, DatabaseResponse};

use crate::app::AppState;
use crate::presentation::commands::helpers::map_command_error;
use crate::presentation::errors::CommandError;

#[tauri::command]
pub async fn database_handle(
    request: DatabaseRequest,
    app_state: State<'_, Arc<AppState>>,
) -> Result<DatabaseResponse, CommandError> {
    app_state
        .services
        .database_service
        .execute(request)
        .await
        .map_err(map_command_error("Database request failed"))
}
