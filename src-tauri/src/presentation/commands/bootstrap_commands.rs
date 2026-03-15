use std::sync::Arc;

use tauri::State;

use crate::app::AppState;
use crate::application::dto::bootstrap_dto::BootstrapSnapshotDto;
use crate::application::dto::group_dto::GroupDto;
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;

#[tauri::command]
pub async fn get_bootstrap_snapshot(
    app_state: State<'_, Arc<AppState>>,
) -> Result<BootstrapSnapshotDto, CommandError> {
    log_command("get_bootstrap_snapshot");

    let settings_fut = async {
        app_state
            .settings_service
            .get_sillytavern_settings()
            .await
            .map_err(map_command_error("Failed to load bootstrap settings snapshot"))
    };

    let characters_fut = async {
        app_state
            .character_service
            .get_all_characters(true)
            .await
            .map_err(map_command_error("Failed to load bootstrap characters snapshot"))
    };

    let groups_fut = async {
        app_state
            .group_service
            .get_all_groups()
            .await
            .map(|groups| groups.into_iter().map(GroupDto::from).collect())
            .map_err(map_command_error("Failed to load bootstrap groups snapshot"))
    };

    let avatars_fut = async {
        app_state
            .avatar_service
            .get_avatars()
            .await
            .map_err(map_command_error("Failed to load bootstrap avatars snapshot"))
    };

    let secret_state_fut = async {
        app_state
            .secret_service
            .read_secret_state()
            .await
            .map_err(map_command_error(
                "Failed to load bootstrap secret state snapshot",
            ))
    };

    let (settings, characters, groups, avatars, secret_state) = tokio::try_join!(
        settings_fut,
        characters_fut,
        groups_fut,
        avatars_fut,
        secret_state_fut
    )?;

    Ok(BootstrapSnapshotDto {
        settings,
        characters,
        groups,
        avatars,
        secret_state,
    })
}

