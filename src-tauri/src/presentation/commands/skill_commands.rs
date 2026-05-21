use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use serde::Serialize;
use tauri::State;

use crate::app::AppState;
use crate::domain::models::skill::{
    SkillFileRef, SkillImportInput, SkillImportPreview, SkillIndexEntry, SkillInstallRequest,
    SkillInstallResult, SkillMoveRequest, SkillReadRequest, SkillReadResult, SkillScope,
    SkillScopeFilter, SkillScopeRetargetRequest, SkillScopeRetargetResult, SkillWriteRequest,
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
    scope: Option<SkillScopeFilter>,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<SkillIndexEntry>, CommandError> {
    log_command("list_skills");

    app_state
        .skill_service
        .list_skills(scope.unwrap_or_default())
        .await
        .map_err(map_command_error("Failed to list Agent Skills"))
}

#[tauri::command]
pub async fn list_skill_files(
    name: String,
    scope: Option<SkillScope>,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<SkillFileRef>, CommandError> {
    log_command(format!("list_skill_files {}", name));

    app_state
        .skill_service
        .list_skill_files(scope.unwrap_or_default(), &name)
        .await
        .map_err(map_command_error("Failed to list Agent Skill files"))
}

#[tauri::command]
pub async fn preview_skill_import(
    input: SkillImportInput,
    target_scope: Option<SkillScope>,
    app_state: State<'_, Arc<AppState>>,
) -> Result<SkillImportPreview, CommandError> {
    log_command("preview_skill_import");

    app_state
        .skill_service
        .preview_import(input, target_scope.unwrap_or_default())
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
    scope: Option<SkillScope>,
    max_chars: Option<usize>,
    start_line: Option<usize>,
    line_count: Option<usize>,
    start_char: Option<usize>,
    app_state: State<'_, Arc<AppState>>,
) -> Result<SkillReadResult, CommandError> {
    log_command(format!("read_skill_file {}/{}", name, path));

    app_state
        .skill_service
        .read_skill_file(SkillReadRequest {
            scope: scope.unwrap_or_default(),
            name,
            path,
            start_line,
            line_count,
            start_char,
            max_chars,
        })
        .await
        .map_err(map_command_error("Failed to read Agent Skill file"))
}

#[tauri::command]
pub async fn write_skill_file(
    name: String,
    path: String,
    content: String,
    scope: Option<SkillScope>,
    expected_sha256: Option<String>,
    app_state: State<'_, Arc<AppState>>,
) -> Result<SkillReadResult, CommandError> {
    log_command(format!("write_skill_file {}/{}", name, path));

    app_state
        .skill_service
        .write_skill_file(SkillWriteRequest {
            scope: scope.unwrap_or_default(),
            name,
            path,
            content,
            expected_sha256,
        })
        .await
        .map_err(map_command_error("Failed to write Agent Skill file"))
}

#[tauri::command]
pub async fn export_skill(
    name: String,
    scope: Option<SkillScope>,
    app_state: State<'_, Arc<AppState>>,
) -> Result<SkillExportPayload, CommandError> {
    log_command(format!("export_skill {}", name));

    let exported = app_state
        .skill_service
        .export_skill(scope.unwrap_or_default(), &name)
        .await
        .map_err(map_command_error("Failed to export Agent Skill"))?;

    Ok(SkillExportPayload {
        file_name: exported.file_name,
        content_base64: BASE64_STANDARD.encode(exported.bytes),
        sha256: exported.sha256,
    })
}

#[tauri::command]
pub async fn delete_skill(
    name: String,
    scope: Option<SkillScope>,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command(format!("delete_skill {}", name));

    app_state
        .skill_service
        .delete_skill(scope.unwrap_or_default(), &name)
        .await
        .map_err(map_command_error("Failed to delete Agent Skill"))
}

#[tauri::command]
pub async fn move_skill(
    request: SkillMoveRequest,
    app_state: State<'_, Arc<AppState>>,
) -> Result<SkillInstallResult, CommandError> {
    log_command(format!("move_skill {}", request.name));

    app_state
        .skill_service
        .move_skill(request)
        .await
        .map_err(map_command_error("Failed to move Agent Skill"))
}

#[tauri::command]
pub async fn retarget_skill_scope(
    request: SkillScopeRetargetRequest,
    app_state: State<'_, Arc<AppState>>,
) -> Result<SkillScopeRetargetResult, CommandError> {
    log_command(format!(
        "retarget_skill_scope {} -> {}",
        request.from_scope.label(),
        request.to_scope.label()
    ));

    app_state
        .skill_service
        .retarget_scope(request)
        .await
        .map_err(map_command_error("Failed to retarget Agent Skill scope"))
}
