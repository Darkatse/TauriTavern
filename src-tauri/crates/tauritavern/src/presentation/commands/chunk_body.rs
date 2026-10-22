use std::borrow::Cow;

use base64::Engine;
use tauri::ipc::InvokeBody;

use crate::presentation::errors::CommandError;

const CHUNK_ENCODING_BASE64: &str = "base64";
const HEADER_CHUNK_ENCODING: &str = "chunk-encoding";

pub(super) fn commit_headers<'a>(
    request: &'a tauri::ipc::Request<'_>,
) -> Result<(&'a str, u64), CommandError> {
    let session_id = required_header(request, "session-id")?;
    let offset = required_header(request, "offset")?
        .parse::<u64>()
        .map_err(|_| CommandError::BadRequest("Commit offset is invalid".into()))?;
    Ok((session_id, offset))
}

fn required_header<'a>(
    request: &'a tauri::ipc::Request<'_>,
    name: &str,
) -> Result<&'a str, CommandError> {
    request
        .headers()
        .get(name)
        .ok_or_else(|| CommandError::BadRequest(format!("Missing commit header: {name}")))?
        .to_str()
        .map_err(|_| CommandError::BadRequest(format!("Invalid commit header: {name}")))
}

pub(super) fn chunk_bytes_from_request<'a>(
    request: &'a tauri::ipc::Request<'_>,
) -> Result<Cow<'a, [u8]>, CommandError> {
    match request.headers().get(HEADER_CHUNK_ENCODING) {
        Some(value) if value == CHUNK_ENCODING_BASE64 => {
            chunk_base64_bytes_from_body(request.body())
        }
        Some(value) => Err(CommandError::BadRequest(format!(
            "Unsupported chunk encoding: {}",
            value.to_str().unwrap_or("<invalid>")
        ))),
        None => chunk_bytes_from_body(request.body()),
    }
}

fn chunk_bytes_from_body(body: &InvokeBody) -> Result<Cow<'_, [u8]>, CommandError> {
    match body {
        InvokeBody::Raw(data) => Ok(Cow::Borrowed(data)),
        InvokeBody::Json(_) => Err(CommandError::BadRequest(
            "Chunk body must be raw bytes".to_string(),
        )),
    }
}

fn chunk_base64_bytes_from_body(body: &InvokeBody) -> Result<Cow<'_, [u8]>, CommandError> {
    let value = match body {
        InvokeBody::Json(serde_json::Value::Object(values)) => values
            .get("data")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                CommandError::BadRequest(
                    "Base64 chunk body must contain a string data field".to_string(),
                )
            })?,
        _ => {
            return Err(CommandError::BadRequest(
                "Base64 chunk body must contain a string data field".to_string(),
            ));
        }
    };

    base64::engine::general_purpose::STANDARD
        .decode(value)
        .map(Cow::Owned)
        .map_err(|_| CommandError::BadRequest("Base64 chunk body is invalid".to_string()))
}
