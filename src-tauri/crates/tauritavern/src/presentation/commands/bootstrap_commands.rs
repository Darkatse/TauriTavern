use std::sync::Arc;

use tauri::State;

use crate::app::backend_errors::BackendErrorHub;
use crate::app::{AppState, BackendReadiness};
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;
use tt_application::dto::bootstrap_dto::BootstrapMetadataDto;
use tt_application::dto::group_dto::GroupDto;

#[tauri::command]
pub async fn get_bootstrap_metadata(
    app_state: State<'_, Arc<AppState>>,
) -> Result<BootstrapMetadataDto, CommandError> {
    log_command("get_bootstrap_metadata");

    let characters_fut = async {
        app_state
            .services
            .character_service
            .get_all_characters(true)
            .await
            .map_err(map_command_error(
                "Failed to load bootstrap characters snapshot",
            ))
    };

    let groups_fut = async {
        app_state
            .services
            .group_service
            .get_all_groups()
            .await
            .map(|groups| groups.into_iter().map(GroupDto::from).collect())
            .map_err(map_command_error(
                "Failed to load bootstrap groups snapshot",
            ))
    };

    let avatars_fut = async {
        app_state
            .services
            .avatar_service
            .get_avatars()
            .await
            .map_err(map_command_error(
                "Failed to load bootstrap avatars snapshot",
            ))
    };

    let secret_state_fut = async {
        app_state
            .services
            .secret_service
            .read_secret_state()
            .await
            .map_err(map_command_error(
                "Failed to load bootstrap secret state snapshot",
            ))
    };

    let (characters, groups, avatars, secret_state) =
        tokio::join!(characters_fut, groups_fut, avatars_fut, secret_state_fut);

    // Character identity is startup-critical. Independent UI domains
    // remain locally unavailable after their already user-visible load error.
    let characters = characters?;
    let groups = groups.unwrap_or_default();
    let avatars = avatars.unwrap_or_default();
    let secret_state = secret_state.unwrap_or_default();

    Ok(BootstrapMetadataDto {
        ios_policy: app_state.ios_policy.clone(),
        characters,
        groups,
        avatars,
        secret_state,
    })
}

#[tauri::command]
pub async fn backend_error_bridge_ready(
    backend_errors: State<'_, Arc<BackendErrorHub>>,
) -> Result<Vec<String>, CommandError> {
    log_command("backend_error_bridge_ready");
    Ok(backend_errors.mark_bridge_ready_and_drain())
}

#[tauri::command]
pub async fn wait_for_backend_ready(
    backend_readiness: State<'_, Arc<BackendReadiness>>,
) -> Result<(), CommandError> {
    log_command("wait_for_backend_ready");
    backend_readiness
        .wait_ready()
        .await
        .map_err(|error| CommandError::InternalServerError(error.to_string()))
}
