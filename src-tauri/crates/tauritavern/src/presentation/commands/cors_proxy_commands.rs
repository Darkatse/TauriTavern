use std::sync::Arc;

use percent_encoding::percent_decode_str;
use serde::{Deserialize, Serialize};
use tauri::http::StatusCode;
use tauri::{Manager, ResourceId, State, WebviewWindow};

use super::byte_reader_commands::ByteResource;
use super::chunk_body::chunk_bytes_from_request;
use crate::app::AppState;
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::commands::user_endpoint_access::ensure_user_endpoint_access;
use crate::presentation::errors::CommandError;
use tt_application::dto::cors_proxy_dto::CorsProxyRequestDto;

const REQUEST_HEADER: &str = "cors-proxy-request";
const READ_CHUNK_BYTES: usize = 64 * 1024;

/// Header values on both sides are WebIDL ByteStrings: one character per byte.
#[derive(Deserialize)]
struct RequestMetadata {
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    locale: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CorsProxyResponseDto {
    status: u16,
    status_text: &'static str,
    headers: Vec<(String, String)>,
    rid: Option<ResourceId>,
}

#[tauri::command]
pub async fn send_cors_proxy_request(
    request: tauri::ipc::Request<'_>,
    window: WebviewWindow,
    app_state: State<'_, Arc<AppState>>,
) -> Result<CorsProxyResponseDto, CommandError> {
    log_command("send_cors_proxy_request");

    let metadata = request_metadata(&request)?;
    let headers = metadata
        .headers
        .into_iter()
        .map(|(name, value)| {
            let value = byte_string_bytes(&value).ok_or_else(|| {
                CommandError::BadRequest(format!("Header {name} is not a ByteString"))
            })?;
            Ok((name, value))
        })
        .collect::<Result<_, CommandError>>()?;
    let dto = CorsProxyRequestDto {
        method: metadata.method,
        url: metadata.url,
        headers,
        body: chunk_bytes_from_request(&request)?.into_owned(),
    };

    let service = &app_state.services.cors_proxy_service;
    let proxy_request = service
        .prepare_request(dto)
        .map_err(map_command_error("CORS proxy request is invalid"))?;
    ensure_user_endpoint_access(
        Some(proxy_request.endpoint.clone()),
        &metadata.locale,
        &window,
        &app_state.services.user_endpoint_access_service,
    )
    .await?;
    let response = service
        .send(proxy_request)
        .await
        .map_err(map_command_error("CORS proxy request failed"))?;

    Ok(CorsProxyResponseDto {
        status: response.status,
        status_text: StatusCode::from_u16(response.status)
            .ok()
            .and_then(|status| status.canonical_reason())
            .unwrap_or_default(),
        headers: response
            .headers
            .into_iter()
            .map(|(name, value)| (name, byte_string(&value)))
            .collect(),
        rid: response.body.map(|body| {
            window
                .resources_table()
                .add(ByteResource::new(body, READ_CHUNK_BYTES))
        }),
    })
}

fn request_metadata(request: &tauri::ipc::Request<'_>) -> Result<RequestMetadata, CommandError> {
    let invalid = || CommandError::BadRequest(format!("Invalid {REQUEST_HEADER} header"));
    let encoded = request
        .headers()
        .get(REQUEST_HEADER)
        .ok_or_else(|| CommandError::BadRequest(format!("Missing {REQUEST_HEADER} header")))?
        .to_str()
        .map_err(|_| invalid())?;
    let json = percent_decode_str(encoded)
        .decode_utf8()
        .map_err(|_| invalid())?;
    serde_json::from_str(&json).map_err(|_| invalid())
}

fn byte_string_bytes(value: &str) -> Option<Vec<u8>> {
    value
        .chars()
        .map(|character| u8::try_from(character).ok())
        .collect()
}

fn byte_string(bytes: &[u8]) -> String {
    bytes.iter().copied().map(char::from).collect()
}
