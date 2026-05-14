use std::sync::Arc;

use serde_json::Value;
use tauri::State;

use crate::app::AppState;
use crate::application::dto::provider_metadata_dto::{
    ProviderModelProvidersRequestDto, SiliconFlowEmbeddingModelsRequestDto,
    WorkersAiModelsRequestDto,
};
use crate::domain::repositories::provider_metadata_repository::{
    NanoGptCredits, NanoGptModelProviders, OpenRouterCredits,
};
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;

#[tauri::command]
pub async fn get_openrouter_model_providers(
    dto: ProviderModelProvidersRequestDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<String>, CommandError> {
    log_command(format!("get_openrouter_model_providers {}", dto.model));

    app_state
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
        .provider_metadata_service
        .openrouter_credits()
        .await
        .map_err(map_command_error("Failed to get OpenRouter credits"))
}

#[tauri::command]
pub async fn get_nanogpt_model_providers(
    dto: ProviderModelProvidersRequestDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<NanoGptModelProviders, CommandError> {
    log_command(format!("get_nanogpt_model_providers {}", dto.model));

    app_state
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
        .provider_metadata_service
        .nanogpt_credits()
        .await
        .map_err(map_command_error("Failed to get NanoGPT credits"))
}

#[tauri::command]
pub async fn get_siliconflow_embedding_models(
    dto: SiliconFlowEmbeddingModelsRequestDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<Value>, CommandError> {
    log_command("get_siliconflow_embedding_models");

    app_state
        .provider_metadata_service
        .siliconflow_embedding_models(dto)
        .await
        .map_err(map_command_error(
            "Failed to get SiliconFlow embedding models",
        ))
}

#[tauri::command]
pub async fn get_workers_ai_embedding_models(
    dto: WorkersAiModelsRequestDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<Value>, CommandError> {
    log_command("get_workers_ai_embedding_models");

    app_state
        .provider_metadata_service
        .workers_ai_embedding_models(dto)
        .await
        .map_err(map_command_error(
            "Failed to get Cloudflare Workers AI embedding models",
        ))
}

#[tauri::command]
pub async fn get_workers_ai_multimodal_models(
    dto: WorkersAiModelsRequestDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<String>, CommandError> {
    log_command("get_workers_ai_multimodal_models");

    app_state
        .provider_metadata_service
        .workers_ai_multimodal_models(dto)
        .await
        .map_err(map_command_error(
            "Failed to get Cloudflare Workers AI multimodal models",
        ))
}
