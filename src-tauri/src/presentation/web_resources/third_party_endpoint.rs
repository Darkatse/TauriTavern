use std::borrow::Cow;
use std::path::Path;

use tauri::http::StatusCode;

use crate::domain::errors::DomainError;
use crate::infrastructure::css_compat::{contains_layer_keyword, flatten_css_layers};
use crate::infrastructure::third_party_assets::resolve_third_party_extension_asset;
use crate::infrastructure::third_party_paths::{
    THIRD_PARTY_EXTENSION_ROUTE_PREFIX, ThirdPartyPathError, parse_third_party_asset_request_path,
};
use crate::presentation::web_resources::response_helpers::{
    respond_bytes, respond_method_not_allowed, respond_no_content, respond_plain_text,
};

const THIRD_PARTY_ALLOWED_METHODS: &str = "GET, HEAD, OPTIONS";
const MAX_MOBILE_INLINE_THIRD_PARTY_ASSET_BYTES: u64 = 32 * 1024 * 1024;
const THIRD_PARTY_LAYER_COMPAT_QUERY: &str = "ttCompat=layer";

fn should_apply_third_party_layer_compat(request: &tauri::http::Request<Vec<u8>>) -> bool {
    request.uri().query().is_some_and(|query| {
        query.split('&').any(|pair| {
            if pair == THIRD_PARTY_LAYER_COMPAT_QUERY {
                return true;
            }

            let Some((key, value)) = pair.split_once('=') else {
                return false;
            };

            key == "ttCompat" && value == "layer"
        })
    })
}

pub fn handle_third_party_asset_web_request(
    local_extensions_dir: &Path,
    global_extensions_dir: &Path,
    request: &tauri::http::Request<Vec<u8>>,
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

fn handle_third_party_asset_route_request(
    local_extensions_dir: &Path,
    global_extensions_dir: &Path,
    request: &tauri::http::Request<Vec<u8>>,
    response: &mut tauri::http::Response<Cow<'static, [u8]>>,
) {
    use tauri::http::Method;

    match request.method() {
        &Method::OPTIONS => {
            respond_no_content(response, THIRD_PARTY_ALLOWED_METHODS);
            return;
        }
        &Method::GET | &Method::HEAD => {}
        _ => {
            respond_method_not_allowed(response, THIRD_PARTY_ALLOWED_METHODS);
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
                respond_bytes(response, StatusCode::OK, Vec::new(), &resolved.mime_type);
                return;
            }

            if cfg!(mobile) && resolved.size_bytes > MAX_MOBILE_INLINE_THIRD_PARTY_ASSET_BYTES {
                tracing::warn!(
                    "Rejected large third-party asset ({} bytes): {}/{}",
                    resolved.size_bytes,
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

            let should_apply_layer_compat =
                resolved.mime_type == "text/css" && should_apply_third_party_layer_compat(request);

            match std::fs::read(&resolved.path) {
                Ok(bytes) => {
                    let bytes = if should_apply_layer_compat && contains_layer_keyword(&bytes) {
                        flatten_css_layers(&bytes)
                    } else {
                        bytes
                    };

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tauri::http::header::{ALLOW, CONTENT_TYPE, HeaderValue};

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

        handle_third_party_asset_web_request(&temp.path, &temp.path, &request, &mut response);

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

        handle_third_party_asset_web_request(&local_root, &global_root, &request, &mut response);

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(CONTENT_TYPE),
            Some(&HeaderValue::from_static("application/json"))
        );
        assert!(response.body().is_empty());
    }

    #[test]
    fn serves_assets_with_redundant_relative_separators() {
        let temp = TempDirGuard::new("third-party-endpoint-redundant-separators");
        let local_root = temp.path.join("local");
        let global_root = temp.path.join("global");
        std::fs::create_dir_all(local_root.join("mobile")).expect("create extension dir");
        std::fs::write(
            local_root.join("mobile").join("style.css"),
            b".example { color: red; }",
        )
        .expect("write stylesheet");

        let request = tauri::http::Request::builder()
            .method("GET")
            .uri("/scripts/extensions/third-party/mobile//style.css")
            .body(Vec::new())
            .expect("request");
        let mut response = tauri::http::Response::new(Cow::Owned(Vec::new()));

        handle_third_party_asset_web_request(&local_root, &global_root, &request, &mut response);

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(CONTENT_TYPE),
            Some(&HeaderValue::from_static("text/css"))
        );
        assert_eq!(response.body().as_ref(), b".example { color: red; }");
    }

    #[test]
    fn applies_layer_compat_query_to_stylesheets() {
        let temp = TempDirGuard::new("third-party-endpoint-layer-compat");
        let local_root = temp.path.join("local");
        let global_root = temp.path.join("global");
        std::fs::create_dir_all(local_root.join("mobile")).expect("create extension dir");
        std::fs::write(
            local_root.join("mobile").join("style.css"),
            b"@layer base{.x{color:red;}}",
        )
        .expect("write stylesheet");

        let request = tauri::http::Request::builder()
            .method("GET")
            .uri("/scripts/extensions/third-party/mobile/style.css?ttCompat=layer")
            .body(Vec::new())
            .expect("request");
        let mut response = tauri::http::Response::new(Cow::Owned(Vec::new()));

        handle_third_party_asset_web_request(&local_root, &global_root, &request, &mut response);

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(CONTENT_TYPE),
            Some(&HeaderValue::from_static("text/css"))
        );
        assert_eq!(response.body().as_ref(), b".x{color:red;}");
    }
}
