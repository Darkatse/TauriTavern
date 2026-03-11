use std::borrow::Cow;
use std::path::Path;

use tauri::http::StatusCode;
use tauri::http::header::{ALLOW, CACHE_CONTROL, CONTENT_TYPE, HeaderValue};

use crate::domain::errors::DomainError;
use crate::infrastructure::third_party_assets::resolve_third_party_extension_asset;
use crate::infrastructure::third_party_paths::{
    THIRD_PARTY_EXTENSION_ROUTE_PREFIX, ThirdPartyPathError, parse_third_party_asset_request_path,
};

const THIRD_PARTY_ALLOWED_METHODS: &str = "GET, HEAD, OPTIONS";
const MAX_MOBILE_INLINE_THIRD_PARTY_ASSET_BYTES: u64 = 32 * 1024 * 1024;

pub fn handle_third_party_asset_web_request(
    local_extensions_dir: &Path,
    global_extensions_dir: &Path,
    request: tauri::http::Request<Vec<u8>>,
    response: &mut tauri::http::Response<Cow<'static, [u8]>>,
) {
    if !request
        .uri()
        .path()
        .starts_with(THIRD_PARTY_EXTENSION_ROUTE_PREFIX)
    {
        return;
    }

    handle_third_party_asset_route_request(
        local_extensions_dir,
        global_extensions_dir,
        request,
        response,
    );
}

#[cfg(dev)]
pub fn handle_third_party_extension_protocol_request<R: tauri::Runtime>(
    ctx: tauri::UriSchemeContext<'_, R>,
    request: tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<Cow<'static, [u8]>> {
    use tauri::Manager;
    use tauri::http::header::{
        ACCESS_CONTROL_ALLOW_HEADERS, ACCESS_CONTROL_ALLOW_METHODS, ACCESS_CONTROL_ALLOW_ORIGIN,
    };

    let mut response = tauri::http::Response::new(Cow::Owned(Vec::new()));

    response
        .headers_mut()
        .insert(ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
    response.headers_mut().insert(
        ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static(THIRD_PARTY_ALLOWED_METHODS),
    );
    response
        .headers_mut()
        .insert(ACCESS_CONTROL_ALLOW_HEADERS, HeaderValue::from_static("*"));

    if !request
        .uri()
        .path()
        .starts_with(THIRD_PARTY_EXTENSION_ROUTE_PREFIX)
    {
        respond_plain_text(&mut response, StatusCode::NOT_FOUND, "Not Found");
        return response;
    }

    let dirs = ctx
        .app_handle()
        .state::<crate::infrastructure::third_party_assets::ThirdPartyExtensionDirs>();
    handle_third_party_asset_route_request(
        &dirs.local_dir,
        &dirs.global_dir,
        request,
        &mut response,
    );
    response
}

fn handle_third_party_asset_route_request(
    local_extensions_dir: &Path,
    global_extensions_dir: &Path,
    request: tauri::http::Request<Vec<u8>>,
    response: &mut tauri::http::Response<Cow<'static, [u8]>>,
) {
    use tauri::http::Method;

    match request.method() {
        &Method::OPTIONS => {
            respond_no_content(response);
            return;
        }
        &Method::GET | &Method::HEAD => {}
        _ => {
            respond_method_not_allowed(response);
            return;
        }
    }

    let request_path = request.uri().path();
    let parsed = match parse_third_party_asset_request_path(request_path) {
        Ok(Some(value)) => value,
        Ok(None) => return,
        Err(ThirdPartyPathError::MissingExtension | ThirdPartyPathError::MissingAssetPath) => {
            respond_plain_text(response, StatusCode::NOT_FOUND, "Not Found");
            return;
        }
        Err(ThirdPartyPathError::InvalidPath) => {
            respond_plain_text(
                response,
                StatusCode::BAD_REQUEST,
                "Invalid third-party asset path",
            );
            return;
        }
    };

    match resolve_third_party_extension_asset(
        local_extensions_dir,
        global_extensions_dir,
        &parsed.extension_folder,
        &parsed.relative_path,
    ) {
        Ok(resolved) => {
            if request.method() == Method::HEAD {
                respond_bytes(
                    response,
                    StatusCode::OK,
                    Vec::new(),
                    &resolved.mime_type,
                );
                return;
            }

            let metadata = match std::fs::metadata(&resolved.path) {
                Ok(value) => value,
                Err(error) => {
                    let status = match error.kind() {
                        std::io::ErrorKind::NotFound => StatusCode::NOT_FOUND,
                        _ => StatusCode::INTERNAL_SERVER_ERROR,
                    };
                    respond_plain_text(
                        response,
                        status,
                        &format!("Failed to stat third-party asset: {}", error),
                    );
                    return;
                }
            };

            let size_bytes = metadata.len();
            if cfg!(mobile) && size_bytes > MAX_MOBILE_INLINE_THIRD_PARTY_ASSET_BYTES {
                tracing::warn!(
                    "Rejected large third-party asset ({} bytes): {}/{}",
                    size_bytes,
                    parsed.extension_folder,
                    parsed.relative_path_display
                );
                respond_plain_text(
                    response,
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "Third-party asset is too large to load on mobile.",
                );
                return;
            }

            match std::fs::read(&resolved.path) {
                Ok(bytes) => {
                    respond_bytes(response, StatusCode::OK, bytes, &resolved.mime_type);
                    tracing::debug!(
                        "Third-party asset hit: {}/{}",
                        parsed.extension_folder,
                        parsed.relative_path_display
                    );
                }
                Err(error) => {
                    respond_plain_text(
                        response,
                        StatusCode::INTERNAL_SERVER_ERROR,
                        &format!("Failed to read third-party asset: {}", error),
                    );
                }
            }
        }
        Err(DomainError::NotFound(_)) => {
            respond_plain_text(response, StatusCode::NOT_FOUND, "Not Found");
            tracing::debug!(
                "Third-party asset 404: {}/{}",
                parsed.extension_folder,
                parsed.relative_path_display
            );
        }
        Err(error) => {
            respond_plain_text(
                response,
                StatusCode::INTERNAL_SERVER_ERROR,
                &error.to_string(),
            );
        }
    }
}

fn respond_no_content(response: &mut tauri::http::Response<Cow<'static, [u8]>>) {
    *response.status_mut() = StatusCode::NO_CONTENT;
    set_allowed_methods_header(response);
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    *response.body_mut() = Cow::Owned(Vec::new());
}

fn respond_plain_text(
    response: &mut tauri::http::Response<Cow<'static, [u8]>>,
    status: StatusCode,
    message: &str,
) {
    *response.status_mut() = status;
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    *response.body_mut() = Cow::Owned(message.as_bytes().to_vec());
}

fn respond_method_not_allowed(response: &mut tauri::http::Response<Cow<'static, [u8]>>) {
    respond_plain_text(
        response,
        StatusCode::METHOD_NOT_ALLOWED,
        "Method not allowed",
    );
    set_allowed_methods_header(response);
}

fn set_allowed_methods_header(response: &mut tauri::http::Response<Cow<'static, [u8]>>) {
    response
        .headers_mut()
        .insert(ALLOW, HeaderValue::from_static(THIRD_PARTY_ALLOWED_METHODS));
}

fn respond_bytes(
    response: &mut tauri::http::Response<Cow<'static, [u8]>>,
    status: StatusCode,
    bytes: Vec<u8>,
    content_type: &str,
) {
    *response.status_mut() = status;
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_str(content_type).expect("Invalid Content-Type"),
    );
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    *response.body_mut() = Cow::Owned(bytes);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct TempDirGuard {
        path: PathBuf,
    }

    impl TempDirGuard {
        fn new(test_name: &str) -> Self {
            let mut path = std::env::temp_dir();
            path.push(format!("tauritavern-{test_name}-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&path).expect("create temp dir");
            Self { path }
        }
    }

    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn rejects_methods_outside_endpoint_contract() {
        let temp = TempDirGuard::new("third-party-endpoint-method-gate");
        let request = tauri::http::Request::builder()
            .method("POST")
            .uri("/scripts/extensions/third-party/mobile/manifest.json")
            .body(Vec::new())
            .expect("request");
        let mut response = tauri::http::Response::new(Cow::Owned(Vec::new()));

        handle_third_party_asset_web_request(&temp.path, &temp.path, request, &mut response);

        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(
            response.headers().get(ALLOW),
            Some(&HeaderValue::from_static(THIRD_PARTY_ALLOWED_METHODS))
        );
    }

    #[test]
    fn head_responses_keep_headers_and_clear_body() {
        let temp = TempDirGuard::new("third-party-endpoint-head");
        let local_root = temp.path.join("local");
        let global_root = temp.path.join("global");
        std::fs::create_dir_all(local_root.join("mobile")).expect("create extension dir");
        std::fs::write(
            local_root.join("mobile").join("manifest.json"),
            br#"{"ok":true}"#,
        )
        .expect("write manifest");

        let request = tauri::http::Request::builder()
            .method("HEAD")
            .uri("/scripts/extensions/third-party/mobile/manifest.json")
            .body(Vec::new())
            .expect("request");
        let mut response = tauri::http::Response::new(Cow::Owned(Vec::new()));

        handle_third_party_asset_web_request(&local_root, &global_root, request, &mut response);

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(CONTENT_TYPE),
            Some(&HeaderValue::from_static("application/json"))
        );
        assert!(response.body().is_empty());
    }
}
