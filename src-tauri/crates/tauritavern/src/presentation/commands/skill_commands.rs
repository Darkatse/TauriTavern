use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use serde::Serialize;
use tauri::{AppHandle, State, WebviewWindow};

use crate::app::AppState;
use crate::infrastructure::staging;
#[cfg(desktop)]
use crate::platform::file_transfer;
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::commands::import_source::ImportSources;
use crate::presentation::errors::CommandError;
use tt_domain::models::skill::{
    DEFAULT_SKILL_READ_FALLBACK_MAX_CHARS, SkillFileRef, SkillImportInput, SkillImportPreview,
    SkillIndexEntry, SkillInstallRequest, SkillInstallResult, SkillMoveRequest, SkillReadRequest,
    SkillReadResult, SkillScope, SkillScopeFilter, SkillScopeRetargetRequest,
    SkillScopeRetargetResult, SkillWriteRequest,
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillExportPayload {
    pub file_name: String,
    pub content_base64: String,
    pub sha256: String,
}

#[tauri::command]
pub async fn download_skill_import_url(
    window: WebviewWindow,
    url: String,
    locale: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<SkillImportInput, CommandError> {
    log_command("download_skill_import_url");

    let sources = ImportSources::new(&window, &app_state, &locale)?;
    app_state
        .services
        .skill_service
        .download_import_url(&url, &sources)
        .await
        .map_err(map_command_error(
            "Failed to download Agent Skill import URL",
        ))
}

#[tauri::command]
pub async fn list_skills(
    scope: Option<SkillScopeFilter>,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<SkillIndexEntry>, CommandError> {
    log_command("list_skills");

    app_state
        .services
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
        .services
        .skill_service
        .list_skill_files(scope.unwrap_or_default(), &name)
        .await
        .map_err(map_command_error("Failed to list Agent Skill files"))
}

/// Directory and archive inputs name staged copies of what the user picked.
async fn validate_import_input(
    app: &AppHandle,
    input: &SkillImportInput,
) -> Result<(), CommandError> {
    match input {
        SkillImportInput::Directory { path, .. } | SkillImportInput::ArchiveFile { path, .. } => {
            staging::validate_staged_path(app, path).await?;
        }
        SkillImportInput::InlineFiles { .. } | SkillImportInput::ArchiveBase64 { .. } => {}
    }
    Ok(())
}

/// Stages a copy of the Skill packages in each folder the user picks.
#[cfg(desktop)]
#[tauri::command]
pub async fn pick_skill_import_directories(
    app: AppHandle,
    window: WebviewWindow,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Option<Vec<SkillImportInput>>, CommandError> {
    log_command("pick_skill_import_directories");

    let Some(directories) = file_transfer::pick_directories(&window, true).await? else {
        return Ok(None);
    };
    let mut staged = Vec::with_capacity(directories.len());
    let result = async {
        for directory in directories {
            let target = staging::create_staged_directory(&app, "skill-directory").await?;
            staged.push(target.clone());
            app_state
                .services
                .skill_service
                .copy_import_directory(&directory, &target)
                .await?;
        }
        Ok::<_, CommandError>(())
    }
    .await;
    if let Err(error) = result {
        for path in &staged {
            if let Err(cleanup_error) = staging::discard(path).await {
                tracing::warn!(%cleanup_error, "Failed to discard staged Skill directory");
            }
        }
        return Err(error);
    }
    Ok(Some(
        staged
            .into_iter()
            .map(|path| SkillImportInput::Directory {
                path: path.to_string_lossy().into_owned(),
                skill_root: None,
                source: Default::default(),
            })
            .collect(),
    ))
}

#[tauri::command]
pub async fn discover_skill_imports(
    app: AppHandle,
    input: SkillImportInput,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<SkillImportInput>, CommandError> {
    log_command("discover_skill_imports");
    validate_import_input(&app, &input).await?;

    app_state
        .services
        .skill_service
        .discover_imports(input)
        .await
        .map_err(map_command_error("Failed to discover Agent Skill imports"))
}

#[tauri::command]
pub async fn discard_skill_import_archive(
    path: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command("discard_skill_import_archive");

    app_state
        .services
        .skill_service
        .discard_import_archive(&path)
        .await
        .map_err(map_command_error("Failed to discard Agent Skill archive"))
}

#[tauri::command]
pub async fn preview_skill_import(
    app: AppHandle,
    input: SkillImportInput,
    target_scope: Option<SkillScope>,
    app_state: State<'_, Arc<AppState>>,
) -> Result<SkillImportPreview, CommandError> {
    log_command("preview_skill_import");
    validate_import_input(&app, &input).await?;

    app_state
        .services
        .skill_service
        .preview_import(input, target_scope.unwrap_or_default())
        .await
        .map_err(map_command_error("Failed to preview Agent Skill import"))
}

#[tauri::command]
pub async fn install_skill_import(
    app: AppHandle,
    request: SkillInstallRequest,
    app_state: State<'_, Arc<AppState>>,
) -> Result<SkillInstallResult, CommandError> {
    log_command("install_skill_import");
    validate_import_input(&app, &request.input).await?;

    app_state
        .services
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
    start_line: Option<usize>,
    line_count: Option<usize>,
    app_state: State<'_, Arc<AppState>>,
) -> Result<SkillReadResult, CommandError> {
    log_command(format!("read_skill_file {}/{}", name, path));

    app_state
        .services
        .skill_service
        .read_skill_file(SkillReadRequest {
            scope: scope.unwrap_or_default(),
            name,
            path,
            start_line,
            line_count,
            max_output_chars: DEFAULT_SKILL_READ_FALLBACK_MAX_CHARS,
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
        .services
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
        .services
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
        .services
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
        .services
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
        .services
        .skill_service
        .retarget_scope(request)
        .await
        .map_err(map_command_error("Failed to retarget Agent Skill scope"))
}
