use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use serde::Serialize;
use tauri::State;

use crate::app::AppState;
use crate::domain::models::skill::{
    SkillImportInput, SkillImportPreview, SkillIndexEntry, SkillInstallRequest, SkillInstallResult,
    SkillReadResult,
};
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillExportPayload {
    pub file_name: String,
    pub content_base64: String,
    pub sha256: String,
}

#[tauri::command]
pub async fn list_skills(
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<SkillIndexEntry>, CommandError> {
    log_command("list_skills");

    app_state
        .skill_service
        .list_skills()
        .await
        .map_err(map_command_error("Failed to list Agent Skills"))
}

#[tauri::command]
pub async fn preview_skill_import(
    input: SkillImportInput,
    app_state: State<'_, Arc<AppState>>,
) -> Result<SkillImportPreview, CommandError> {
    log_command("preview_skill_import");

    app_state
        .skill_service
        .preview_import(input)
        .await
        .map_err(map_command_error("Failed to preview Agent Skill import"))
}

#[tauri::command]
pub async fn install_skill_import(
    request: SkillInstallRequest,
    app_state: State<'_, Arc<AppState>>,
) -> Result<SkillInstallResult, CommandError> {
    log_command("install_skill_import");

    app_state
        .skill_service
        .install_import(request)
        .await
        .map_err(map_command_error("Failed to install Agent Skill"))
}

#[tauri::command]
pub async fn read_skill_file(
    name: String,
    path: String,
    max_chars: Option<usize>,
    app_state: State<'_, Arc<AppState>>,
) -> Result<SkillReadResult, CommandError> {
    log_command(format!("read_skill_file {}/{}", name, path));

    app_state
        .skill_service
        .read_skill_file(&name, &path, max_chars)
        .await
        .map_err(map_command_error("Failed to read Agent Skill file"))
}

#[tauri::command]
pub async fn export_skill(
    name: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<SkillExportPayload, CommandError> {
    log_command(format!("export_skill {}", name));

    let exported = app_state
        .skill_service
        .export_skill(&name)
        .await
        .map_err(map_command_error("Failed to export Agent Skill"))?;

    Ok(SkillExportPayload {
        file_name: exported.file_name,
        content_base64: BASE64_STANDARD.encode(exported.bytes),
        sha256: exported.sha256,
    })
}
