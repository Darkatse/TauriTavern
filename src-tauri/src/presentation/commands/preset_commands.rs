use serde_json::Value;
use std::sync::Arc;
use tauri::State;

use crate::app::AppState;
use crate::application::dto::preset_dto::{
    DeleteOpenAIPresetDto, DeleteOpenAIPresetResponseDto, DeletePresetDto, RestorePresetDto,
    RestorePresetResponseDto, SaveOpenAIPresetDto, SavePresetDto, SavePresetResponseDto,
};
use crate::domain::models::preset::PresetType;
use crate::infrastructure::logging::logger;
use crate::presentation::errors::CommandError;

/// Save a preset
#[tauri::command]
pub async fn save_preset(
    app_state: State<'_, Arc<AppState>>,
    dto: SavePresetDto,
) -> Result<SavePresetResponseDto, CommandError> {
    logger::debug(&format!(
        "Command: save_preset, name: {}, api_id: {}",
        dto.name, dto.api_id
    ));

    // Validate input
    if dto.name.trim().is_empty() {
        logger::warn("Preset name is empty");
        return Err(CommandError::BadRequest(
            "Preset name cannot be empty".to_string(),
        ));
    }

    if dto.preset.is_null() {
        logger::warn("Preset data is null");
        return Err(CommandError::BadRequest(
            "Preset data cannot be null".to_string(),
        ));
    }

    // Create preset from DTO
    let preset = app_state
        .preset_service
        .create_preset(dto.name.clone(), &dto.api_id, dto.preset)
        .map_err(|e| {
            logger::error(&format!("Failed to create preset: {}", e));
            CommandError::from(e)
        })?;

    // Save preset
    app_state
        .preset_service
        .save_preset(&preset)
        .await
        .map_err(|e| {
            logger::error(&format!("Failed to save preset: {}", e));
            CommandError::from(e)
        })?;

    logger::info(&format!("Preset saved successfully: {}", preset.name));
    Ok(SavePresetResponseDto::new(preset.name))
}

/// Delete a preset
#[tauri::command]
pub async fn delete_preset(
    app_state: State<'_, Arc<AppState>>,
    dto: DeletePresetDto,
) -> Result<(), CommandError> {
    logger::debug(&format!(
        "Command: delete_preset, name: {}, api_id: {}",
        dto.name, dto.api_id
    ));

    // Validate input
    if dto.name.trim().is_empty() {
        logger::warn("Preset name is empty");
        return Err(CommandError::BadRequest(
            "Preset name cannot be empty".to_string(),
        ));
    }

    // Get preset type
    let preset_type = PresetType::from_api_id(&dto.api_id).ok_or_else(|| {
        logger::error(&format!("Unknown API ID: {}", dto.api_id));
        CommandError::BadRequest(format!("Unknown API ID: {}", dto.api_id))
    })?;

    // Delete preset
    app_state
        .preset_service
        .delete_preset(&dto.name, &preset_type)
        .await
        .map_err(|e| {
            logger::error(&format!("Failed to delete preset: {}", e));
            CommandError::from(e)
        })?;

    logger::info(&format!("Preset deleted successfully: {}", dto.name));
    Ok(())
}

/// Restore a default preset
#[tauri::command]
pub async fn restore_preset(
    app_state: State<'_, Arc<AppState>>,
    dto: RestorePresetDto,
) -> Result<RestorePresetResponseDto, CommandError> {
    logger::debug(&format!(
        "Command: restore_preset, name: {}, api_id: {}",
        dto.name, dto.api_id
    ));

    // Validate input
    if dto.name.trim().is_empty() {
        logger::warn("Preset name is empty");
        return Err(CommandError::BadRequest(
            "Preset name cannot be empty".to_string(),
        ));
    }

    // Get preset type
    let preset_type = PresetType::from_api_id(&dto.api_id).ok_or_else(|| {
        logger::error(&format!("Unknown API ID: {}", dto.api_id));
        CommandError::BadRequest(format!("Unknown API ID: {}", dto.api_id))
    })?;

    // Try to restore default preset
    match app_state
        .preset_service
        .restore_default_preset(&dto.name, &preset_type)
        .await
    {
        Ok(Some(default_preset)) => {
            logger::info(&format!(
                "Default preset found for restoration: {}",
                dto.name
            ));
            Ok(RestorePresetResponseDto::new(true, default_preset.data))
        }
        Ok(None) => {
            logger::debug(&format!("Default preset not found: {}", dto.name));
            Ok(RestorePresetResponseDto::not_found())
        }
        Err(e) => {
            logger::error(&format!("Failed to restore preset: {}", e));
            Err(CommandError::from(e))
        }
    }
}

/// Save an OpenAI preset (specialized endpoint)
#[tauri::command]
pub async fn save_openai_preset(
    app_state: State<'_, Arc<AppState>>,
    name: String,
    dto: SaveOpenAIPresetDto,
) -> Result<SavePresetResponseDto, CommandError> {
    logger::debug(&format!("Command: save_openai_preset, name: {}", name));

    // Validate input
    if name.trim().is_empty() {
        logger::warn("Preset name is empty");
        return Err(CommandError::BadRequest(
            "Preset name cannot be empty".to_string(),
        ));
    }

    // Create preset
    let preset = app_state
        .preset_service
        .create_preset(name.clone(), "openai", dto.preset)
        .map_err(|e| {
            logger::error(&format!("Failed to create OpenAI preset: {}", e));
            CommandError::from(e)
        })?;

    // Save preset
    app_state
        .preset_service
        .save_preset(&preset)
        .await
        .map_err(|e| {
            logger::error(&format!("Failed to save OpenAI preset: {}", e));
            CommandError::from(e)
        })?;

    logger::info(&format!(
        "OpenAI preset saved successfully: {}",
        preset.name
    ));
    Ok(SavePresetResponseDto::new(preset.name))
}

/// Delete an OpenAI preset (specialized endpoint)
#[tauri::command]
pub async fn delete_openai_preset(
    app_state: State<'_, Arc<AppState>>,
    dto: DeleteOpenAIPresetDto,
) -> Result<DeleteOpenAIPresetResponseDto, CommandError> {
    logger::debug(&format!(
        "Command: delete_openai_preset, name: {}",
        dto.name
    ));

    // Validate input
    if dto.name.trim().is_empty() {
        logger::warn("Preset name is empty");
        return Err(CommandError::BadRequest(
            "Preset name cannot be empty".to_string(),
        ));
    }

    // Delete preset
    match app_state
        .preset_service
        .delete_preset(&dto.name, &PresetType::OpenAI)
        .await
    {
        Ok(()) => {
            logger::info(&format!("OpenAI preset deleted successfully: {}", dto.name));
            Ok(DeleteOpenAIPresetResponseDto::success())
        }
        Err(e) => {
            logger::error(&format!("Failed to delete OpenAI preset: {}", e));
            Ok(DeleteOpenAIPresetResponseDto::error())
        }
    }
}

/// List presets of a specific type
#[tauri::command]
pub async fn list_presets(
    app_state: State<'_, Arc<AppState>>,
    api_id: String,
) -> Result<Vec<String>, CommandError> {
    logger::debug(&format!("Command: list_presets, api_id: {}", api_id));

    // Get preset type
    let preset_type = PresetType::from_api_id(&api_id).ok_or_else(|| {
        logger::error(&format!("Unknown API ID: {}", api_id));
        CommandError::BadRequest(format!("Unknown API ID: {}", api_id))
    })?;

    // List presets
    let presets = app_state
        .preset_service
        .list_presets(&preset_type)
        .await
        .map_err(|e| {
            logger::error(&format!("Failed to list presets: {}", e));
            CommandError::from(e)
        })?;

    logger::debug(&format!(
        "Found {} presets of type {}",
        presets.len(),
        api_id
    ));
    Ok(presets)
}

/// Check if a preset exists
#[tauri::command]
pub async fn preset_exists(
    app_state: State<'_, Arc<AppState>>,
    name: String,
    api_id: String,
) -> Result<bool, CommandError> {
    logger::debug(&format!(
        "Command: preset_exists, name: {}, api_id: {}",
        name, api_id
    ));

    // Get preset type
    let preset_type = PresetType::from_api_id(&api_id).ok_or_else(|| {
        logger::error(&format!("Unknown API ID: {}", api_id));
        CommandError::BadRequest(format!("Unknown API ID: {}", api_id))
    })?;

    // Check if preset exists
    let exists = app_state
        .preset_service
        .preset_exists(&name, &preset_type)
        .await
        .map_err(|e| {
            logger::error(&format!("Failed to check preset existence: {}", e));
            CommandError::from(e)
        })?;

    logger::debug(&format!("Preset {} exists: {}", name, exists));
    Ok(exists)
}

/// Get a preset by name and type
#[tauri::command]
pub async fn get_preset(
    app_state: State<'_, Arc<AppState>>,
    name: String,
    api_id: String,
) -> Result<Option<Value>, CommandError> {
    logger::debug(&format!(
        "Command: get_preset, name: {}, api_id: {}",
        name, api_id
    ));

    // Get preset type
    let preset_type = PresetType::from_api_id(&api_id).ok_or_else(|| {
        logger::error(&format!("Unknown API ID: {}", api_id));
        CommandError::BadRequest(format!("Unknown API ID: {}", api_id))
    })?;

    // Get preset
    let preset = app_state
        .preset_service
        .get_preset(&name, &preset_type)
        .await
        .map_err(|e| {
            logger::error(&format!("Failed to get preset: {}", e));
            CommandError::from(e)
        })?;

    match preset {
        Some(preset) => {
            logger::debug(&format!("Preset found: {}", name));
            Ok(Some(preset.data_with_name()))
        }
        None => {
            logger::debug(&format!("Preset not found: {}", name));
            Ok(None)
        }
    }
}
