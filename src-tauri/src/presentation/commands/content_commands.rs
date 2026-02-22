use std::sync::Arc;

use reqwest::header::{CONTENT_DISPOSITION, CONTENT_TYPE};
use reqwest::redirect::Policy;
use serde::Serialize;
use tauri::State;

use crate::app::AppState;
use crate::infrastructure::http_client::build_http_client;
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;

#[tauri::command]
pub async fn initialize_default_content(
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command("initialize_default_content");

    app_state
        .content_service
        .initialize_default_content("default-user")
        .await
        .map_err(map_command_error("Failed to initialize default content"))
}

#[tauri::command]
pub async fn is_default_content_initialized(
    app_state: State<'_, Arc<AppState>>,
) -> Result<bool, CommandError> {
    log_command("is_default_content_initialized");

    app_state
        .content_service
        .is_default_content_initialized("default-user")
        .await
        .map_err(map_command_error(
            "Failed to check default content initialization state",
        ))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalImportDownloadResult {
    pub data: Vec<u8>,
    pub file_name: String,
    pub mime_type: String,
}

#[tauri::command]
pub async fn download_external_import_url(
    url: String,
) -> Result<ExternalImportDownloadResult, CommandError> {
    log_command("download_external_import_url");

    let parsed_url = reqwest::Url::parse(url.trim())
        .map_err(|_| CommandError::BadRequest("Invalid import URL".to_string()))?;

    match parsed_url.scheme() {
        "http" | "https" => {}
        _ => {
            return Err(CommandError::BadRequest(
                "Unsupported URL protocol".to_string(),
            ));
        }
    }

    let client = build_http_client(reqwest::Client::builder().redirect(Policy::limited(5)))
        .map_err(|error| CommandError::InternalServerError(error.to_string()))?;

    let response = client
        .get(parsed_url.clone())
        .send()
        .await
        .map_err(|error| CommandError::InternalServerError(error.to_string()))?;

    if !response.status().is_success() {
        return Err(CommandError::InternalServerError(format!(
            "Upstream responded with HTTP {}",
            response.status()
        )));
    }

    let headers = response.headers().clone();
    let content_type = headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_lowercase();

    let file_name = derive_file_name(&parsed_url, &headers);
    let is_png_content = content_type.starts_with("image/png");
    let is_png_file_name = file_name.to_lowercase().ends_with(".png");
    if !is_png_content && !is_png_file_name {
        return Err(CommandError::BadRequest(
            "Only PNG imports are supported".to_string(),
        ));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|error| CommandError::InternalServerError(error.to_string()))?;

    Ok(ExternalImportDownloadResult {
        data: bytes.to_vec(),
        file_name: if is_png_file_name {
            file_name
        } else {
            format!("{file_name}.png")
        },
        mime_type: "image/png".to_string(),
    })
}

fn derive_file_name(url: &reqwest::Url, headers: &reqwest::header::HeaderMap) -> String {
    let from_header = headers
        .get(CONTENT_DISPOSITION)
        .and_then(|value| value.to_str().ok())
        .and_then(parse_filename_from_content_disposition);

    if let Some(name) = from_header {
        return sanitize_file_name(&name);
    }

    let from_url = url
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        .unwrap_or("shared-character.png");

    sanitize_file_name(from_url)
}

fn parse_filename_from_content_disposition(value: &str) -> Option<String> {
    let utf8_prefix = "filename*=UTF-8''";
    if let Some(start) = value.find(utf8_prefix) {
        let encoded = value[start + utf8_prefix.len()..]
            .split(';')
            .next()
            .unwrap_or("")
            .trim();

        if !encoded.is_empty() {
            return Some(encoded.to_string());
        }
    }

    let marker = "filename=";
    if let Some(start) = value.find(marker) {
        let raw = value[start + marker.len()..]
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .trim_matches('"');

        if !raw.is_empty() {
            return Some(raw.to_string());
        }
    }

    None
}

fn sanitize_file_name(input: &str) -> String {
    let sanitized = input
        .chars()
        .map(|character| match character {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            control if control.is_control() => '_',
            other => other,
        })
        .collect::<String>()
        .trim()
        .trim_end_matches(['.', ' '])
        .to_string();

    if sanitized.is_empty() {
        "shared-character.png".to_string()
    } else {
        sanitized
    }
}
