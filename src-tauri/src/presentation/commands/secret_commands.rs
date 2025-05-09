use std::sync::Arc;
use tauri::State;

use crate::app::AppState;
use crate::application::dto::secret_dto::{SecretStateDto, AllSecretsDto, FindSecretDto, FindSecretResponseDto, WriteSecretDto};
use crate::presentation::errors::CommandError;
use crate::infrastructure::logging::logger;

/// 写入密钥
#[tauri::command]
pub async fn write_secret(
    dto: WriteSecretDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<String, CommandError> {
    logger::debug(&format!("Command: write_secret {}", dto.key));

    app_state.secret_service.write_secret(&dto.key, &dto.value).await
        .map_err(|e| {
            logger::error(&format!("Failed to write secret {}: {}", dto.key, e));
            CommandError::from(e)
        })?;

    Ok("ok".to_string())
}

/// 读取密钥状态
#[tauri::command]
pub async fn read_secret_state(
    app_state: State<'_, Arc<AppState>>,
) -> Result<SecretStateDto, CommandError> {
    logger::debug("Command: read_secret_state");

    app_state.secret_service.read_secret_state().await
        .map_err(|e| {
            logger::error(&format!("Failed to read secret state: {}", e));
            CommandError::from(e)
        })
}

/// 查看所有密钥
#[tauri::command]
pub async fn view_secrets(
    app_state: State<'_, Arc<AppState>>,
) -> Result<AllSecretsDto, CommandError> {
    logger::debug("Command: view_secrets");

    app_state.secret_service.view_secrets().await
        .map_err(|e| {
            logger::error(&format!("Failed to view secrets: {}", e));
            CommandError::from(e)
        })
}

/// 查找特定密钥
#[tauri::command]
pub async fn find_secret(
    dto: FindSecretDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<FindSecretResponseDto, CommandError> {
    logger::debug(&format!("Command: find_secret {}", dto.key));

    app_state.secret_service.find_secret(&dto.key).await
        .map_err(|e| {
            logger::error(&format!("Failed to find secret {}: {}", dto.key, e));
            CommandError::from(e)
        })
}
