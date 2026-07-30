use std::borrow::Cow;

use base64::Engine;
use tauri::ipc::InvokeBody;

use crate::presentation::errors::CommandError;

const CHUNK_ENCODING_BASE64: &str = "base64";
const HEADER_CHUNK_ENCODING: &str = "chunk-encoding";

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_body_is_borrowed() {
        let body = InvokeBody::Raw(vec![1, 2, 3]);
        let bytes = chunk_bytes_from_body(&body).expect("raw bytes should parse");

        assert!(matches!(bytes, Cow::Borrowed(_)));
        assert_eq!(bytes.as_ref(), &[1, 2, 3]);
    }

    #[test]
    fn json_byte_array_is_not_a_raw_body() {
        let body = InvokeBody::Json(serde_json::json!([0, 127, 255]));
        assert!(matches!(
            chunk_bytes_from_body(&body),
            Err(CommandError::BadRequest(_))
        ));
    }

    #[test]
    fn android_base64_body_decodes() {
        let body = InvokeBody::Json(serde_json::json!({ "data": "AQIDBA==" }));
        let bytes = chunk_base64_bytes_from_body(&body).expect("base64 bytes should parse");

        assert_eq!(bytes.as_ref(), &[1, 2, 3, 4]);
    }

    #[test]
    fn malformed_base64_bodies_are_rejected() {
        for body in [
            InvokeBody::Json(serde_json::json!("AQIDBA==")),
            InvokeBody::Json(serde_json::json!({ "data": "***" })),
        ] {
            assert!(matches!(
                chunk_base64_bytes_from_body(&body),
                Err(CommandError::BadRequest(_))
            ));
        }
    }
}
