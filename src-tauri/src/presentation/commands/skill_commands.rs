use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use futures_util::TryStreamExt;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::State;

use crate::app::AppState;
use crate::domain::models::skill::{
    SkillFileRef, SkillImportInput, SkillImportPreview, SkillIndexEntry, SkillInlineFile,
    SkillInstallRequest, SkillInstallResult, SkillMoveRequest, SkillReadRequest, SkillReadResult,
    SkillScope, SkillScopeFilter, SkillScopeRetargetRequest, SkillScopeRetargetResult,
    SkillWriteRequest,
};
use crate::infrastructure::http_client_pool::{HttpClientPool, HttpClientProfile};
use crate::presentation::commands::helpers::{
    ensure_ios_policy_allows, log_command, map_command_error,
};
use crate::presentation::errors::CommandError;

const MAX_REMOTE_SKILL_MD_BYTES: usize = 1024 * 1024;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillExportPayload {
    pub file_name: String,
    pub content_base64: String,
    pub sha256: String,
}

#[tauri::command]
pub async fn download_skill_import_url(
    url: String,
    app_state: State<'_, Arc<AppState>>,
    http_clients: State<'_, Arc<HttpClientPool>>,
) -> Result<SkillImportInput, CommandError> {
    log_command("download_skill_import_url");

    ensure_ios_policy_allows(
        &app_state.ios_policy,
        app_state.ios_policy.capabilities.content.external_import,
        "content.external_import",
    )?;

    let parsed_url = normalize_skill_import_url(&url)?;
    let client = http_clients
        .client(HttpClientProfile::Download)
        .map_err(|error| CommandError::InternalServerError(error.to_string()))?;
    let response = client
        .get(parsed_url.clone())
        .send()
        .await
        .map_err(|error| CommandError::InternalServerError(error.to_string()))?;

    if !response.status().is_success() {
        return Err(CommandError::InternalServerError(format!(
            "Skill download upstream responded with HTTP {}",
            response.status()
        )));
    }

    if response
        .content_length()
        .is_some_and(|length| length > MAX_REMOTE_SKILL_MD_BYTES as u64)
    {
        return Err(CommandError::BadRequest(format!(
            "Remote SKILL.md must be <= {MAX_REMOTE_SKILL_MD_BYTES} bytes"
        )));
    }

    let bytes = read_remote_skill_bytes(response).await?;
    let content = String::from_utf8(bytes.clone())
        .map_err(|_| CommandError::BadRequest("Remote SKILL.md must be valid UTF-8".to_string()))?;
    let sha256 = sha256_hex(&bytes);
    let source_url = sanitized_source_url(parsed_url);

    Ok(SkillImportInput::InlineFiles {
        files: vec![SkillInlineFile {
            path: "SKILL.md".to_string(),
            encoding: "utf8".to_string(),
            content,
            media_type: Some("text/markdown".to_string()),
            size_bytes: Some(bytes.len() as u64),
            sha256: Some(sha256),
        }],
        source: serde_json::json!({
            "kind": "url",
            "id": source_url,
            "label": source_url,
        }),
    })
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

fn normalize_skill_import_url(raw: &str) -> Result<reqwest::Url, CommandError> {
    let url = reqwest::Url::parse(raw.trim())
        .map_err(|_| CommandError::BadRequest("Skill import URL must be valid".to_string()))?;
    if url.scheme() != "https" {
        return Err(CommandError::BadRequest(
            "Skill import URL must use https".to_string(),
        ));
    }
    if url.host_str().is_none() {
        return Err(CommandError::BadRequest(
            "Skill import URL host is required".to_string(),
        ));
    }
    let file_name = url
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        .unwrap_or_default();
    if file_name != "SKILL.md" {
        return Err(CommandError::BadRequest(
            "Skill import URL must point to a raw SKILL.md file".to_string(),
        ));
    }
    Ok(url)
}

async fn read_remote_skill_bytes(response: reqwest::Response) -> Result<Vec<u8>, CommandError> {
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream
        .try_next()
        .await
        .map_err(|error| CommandError::InternalServerError(error.to_string()))?
    {
        let next_len = bytes
            .len()
            .checked_add(chunk.len())
            .ok_or_else(|| CommandError::BadRequest("Remote SKILL.md is too large".to_string()))?;
        if next_len > MAX_REMOTE_SKILL_MD_BYTES {
            return Err(CommandError::BadRequest(format!(
                "Remote SKILL.md must be <= {MAX_REMOTE_SKILL_MD_BYTES} bytes"
            )));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn sanitized_source_url(mut url: reqwest::Url) -> String {
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.set_query(None);
    url.set_fragment(None);
    url.to_string()
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::{normalize_skill_import_url, sanitized_source_url};

    #[test]
    fn skill_import_url_requires_https_skill_md() {
        assert!(
            normalize_skill_import_url(
                "https://github.com/anthropics/skills/raw/refs/heads/main/skills/frontend-design/SKILL.md"
            )
            .is_ok()
        );

        let http_error = normalize_skill_import_url("http://example.com/SKILL.md").unwrap_err();
        assert!(http_error.to_string().contains("https"));

        let path_error = normalize_skill_import_url("https://example.com/README.md").unwrap_err();
        assert!(path_error.to_string().contains("SKILL.md"));
    }

    #[test]
    fn sanitized_source_url_drops_secret_parts() {
        let url = normalize_skill_import_url(
            "https://user:pass@example.com/a/SKILL.md?token=secret#frag",
        )
        .unwrap();

        assert_eq!(sanitized_source_url(url), "https://example.com/a/SKILL.md");
    }
}
