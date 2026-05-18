use std::sync::Arc;

use futures_util::TryStreamExt;
use serde::Serialize;
use tauri::State;
use tokio::io::AsyncWriteExt;

use crate::app::AppState;
use crate::domain::models::asset::{AssetCatalog, AssetCategory};
use crate::infrastructure::http_client_pool::{HttpClientPool, HttpClientProfile};
use crate::presentation::commands::helpers::{
    ensure_ios_policy_allows, log_command, map_command_error,
};
use crate::presentation::errors::CommandError;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetDownloadResult {
    pub data: Vec<u8>,
    pub mime_type: String,
}

#[tauri::command]
pub async fn get_assets_library(
    app_state: State<'_, Arc<AppState>>,
) -> Result<AssetCatalog, CommandError> {
    log_command("get_assets_library");

    app_state
        .asset_service
        .list_assets()
        .await
        .map_err(map_command_error("Failed to list assets library"))
}

#[tauri::command]
pub async fn download_asset(
    url: String,
    category: String,
    filename: String,
    app_state: State<'_, Arc<AppState>>,
    http_clients: State<'_, Arc<HttpClientPool>>,
) -> Result<AssetDownloadResult, CommandError> {
    log_command(format!("download_asset {}", category));

    ensure_ios_policy_allows(
        &app_state.ios_policy,
        app_state.ios_policy.capabilities.content.external_import,
        "content.external_import",
    )?;

    let category = app_state
        .asset_service
        .validate_download_request(&category, &filename)?;

    let parsed_url = reqwest::Url::parse(url.trim())
        .map_err(|_| CommandError::BadRequest("Asset download URL must be valid".to_string()))?;
    match parsed_url.scheme() {
        "http" | "https" => {}
        _ => {
            return Err(CommandError::BadRequest(
                "Unsupported asset download URL protocol".to_string(),
            ));
        }
    }

    let host = parsed_url
        .host_str()
        .ok_or_else(|| CommandError::BadRequest("Asset download URL host is required".to_string()))?
        .to_ascii_lowercase();
    if !is_import_host_whitelisted(&host) {
        return Err(CommandError::NotFound(format!(
            "Asset import host is not whitelisted: {}",
            host
        )));
    }

    let client = http_clients
        .client(HttpClientProfile::Download)
        .map_err(|error| CommandError::InternalServerError(error.to_string()))?;
    let response = client
        .get(parsed_url)
        .send()
        .await
        .map_err(|error| CommandError::InternalServerError(error.to_string()))?;

    if !response.status().is_success() {
        return Err(CommandError::InternalServerError(format!(
            "Asset download upstream responded with HTTP {}",
            response.status()
        )));
    }

    if category == AssetCategory::Character {
        let bytes = response
            .bytes()
            .await
            .map_err(|error| CommandError::InternalServerError(error.to_string()))?
            .to_vec();
        let mime_type = mime_guess::from_path(&filename)
            .first_or_octet_stream()
            .essence_str()
            .to_string();
        return Ok(AssetDownloadResult {
            data: bytes,
            mime_type,
        });
    }

    let (category, temp_path) = app_state
        .asset_service
        .stage_asset_file(category.as_str(), &filename)
        .await
        .map_err(map_command_error("Failed to stage asset download"))?;

    if let Err(error) = write_response_body_to_file(response, &temp_path).await {
        if let Err(cleanup_error) = app_state
            .asset_service
            .discard_staged_asset_file(&filename)
            .await
        {
            return Err(CommandError::InternalServerError(format!(
                "{}; additionally failed to remove partial asset download: {}",
                error, cleanup_error
            )));
        }

        return Err(error);
    }

    if let Err(error) = app_state
        .asset_service
        .commit_staged_asset_file(category, &filename)
        .await
    {
        let command_error = map_command_error("Failed to store downloaded asset")(error);
        if let Err(cleanup_error) = app_state
            .asset_service
            .discard_staged_asset_file(&filename)
            .await
        {
            return Err(CommandError::InternalServerError(format!(
                "{}; additionally failed to remove staged asset download: {}",
                command_error, cleanup_error
            )));
        }

        return Err(command_error);
    }

    Ok(AssetDownloadResult {
        data: Vec::new(),
        mime_type: "application/octet-stream".to_string(),
    })
}

#[tauri::command]
pub async fn delete_asset(
    category: String,
    filename: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command(format!("delete_asset {}", category));

    app_state
        .asset_service
        .delete_asset_file(&category, &filename)
        .await
        .map_err(map_command_error("Failed to delete asset"))
}

#[tauri::command]
pub async fn get_character_assets(
    name: String,
    category: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<String>, CommandError> {
    log_command(format!("get_character_assets {}", category));

    app_state
        .asset_service
        .list_character_assets(&name, &category)
        .await
        .map_err(map_command_error("Failed to list character assets"))
}

fn is_import_host_whitelisted(host: &str) -> bool {
    matches!(
        host,
        "localhost"
            | "127.0.0.1"
            | "::1"
            | "cdn.discordapp.com"
            | "files.catbox.moe"
            | "raw.githubusercontent.com"
    )
}

async fn write_response_body_to_file(
    response: reqwest::Response,
    path: &std::path::Path,
) -> Result<(), CommandError> {
    let mut file = tokio::fs::File::create(path)
        .await
        .map_err(|error| CommandError::InternalServerError(error.to_string()))?;
    let mut stream = response.bytes_stream();

    while let Some(chunk) = stream
        .try_next()
        .await
        .map_err(|error| CommandError::InternalServerError(error.to_string()))?
    {
        file.write_all(&chunk)
            .await
            .map_err(|error| CommandError::InternalServerError(error.to_string()))?;
    }

    file.flush()
        .await
        .map_err(|error| CommandError::InternalServerError(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::is_import_host_whitelisted;

    #[test]
    fn import_host_whitelist_matches_default_content_sources() {
        assert!(is_import_host_whitelisted("localhost"));
        assert!(is_import_host_whitelisted("raw.githubusercontent.com"));
        assert!(is_import_host_whitelisted("cdn.discordapp.com"));
        assert!(is_import_host_whitelisted("files.catbox.moe"));
        assert!(!is_import_host_whitelisted("example.com"));
    }
}
