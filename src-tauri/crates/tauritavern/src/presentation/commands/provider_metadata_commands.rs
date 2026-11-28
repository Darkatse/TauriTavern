use std::sync::Arc;

use tauri::State;

use crate::app::AppState;
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;
use tt_application::dto::provider_metadata_dto::{
    OpenRouterCodeExchangeRequestDto, ProviderModelProvidersRequestDto,
};
use tt_ports::repositories::provider_metadata_repository::{
    ModelProvider, NanoGptCredits, NanoGptModelProviders, OpenRouterCredits,
};

#[tauri::command]
pub async fn get_openrouter_providers(
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<ModelProvider>, CommandError> {
    log_command("get_openrouter_providers");

    app_state
        .services
        .provider_metadata_service
        .openrouter_providers()
        .await
        .map_err(map_command_error("Failed to get OpenRouter providers"))
}

#[tauri::command]
pub async fn get_openrouter_model_providers(
    dto: ProviderModelProvidersRequestDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<ModelProvider>, CommandError> {
    log_command(format!("get_openrouter_model_providers {}", dto.model));

    app_state
        .services
        .provider_metadata_service
        .openrouter_model_providers(dto)
        .await
        .map_err(map_command_error(
            "Failed to get OpenRouter model providers",
        ))
}

#[tauri::command]
pub async fn get_openrouter_credits(
    app_state: State<'_, Arc<AppState>>,
) -> Result<OpenRouterCredits, CommandError> {
    log_command("get_openrouter_credits");

    app_state
        .services
        .provider_metadata_service
        .openrouter_credits()
        .await
        .map_err(map_command_error("Failed to get OpenRouter credits"))
}

#[tauri::command]
pub async fn exchange_openrouter_code(
    dto: OpenRouterCodeExchangeRequestDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<String, CommandError> {
    log_command("exchange_openrouter_code");

    app_state
        .services
        .provider_metadata_service
        .exchange_openrouter_code(dto)
        .await
        .map_err(map_command_error("Failed to authorize OpenRouter"))
}

#[tauri::command]
pub async fn get_nanogpt_model_providers(
    dto: ProviderModelProvidersRequestDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<NanoGptModelProviders, CommandError> {
    log_command(format!("get_nanogpt_model_providers {}", dto.model));

    app_state
        .services
        .provider_metadata_service
        .nanogpt_model_providers(dto)
        .await
        .map_err(map_command_error("Failed to get NanoGPT model providers"))
}

#[tauri::command]
pub async fn get_nanogpt_credits(
    app_state: State<'_, Arc<AppState>>,
) -> Result<NanoGptCredits, CommandError> {
    log_command("get_nanogpt_credits");

    app_state
        .services
        .provider_metadata_service
        .nanogpt_credits()
        .await
        .map_err(map_command_error("Failed to get NanoGPT credits"))
}
