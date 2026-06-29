use std::path::Path;

use crate::application::services::host_resource_service::contract::{
    HostResourceMethod, HostResourceRequest, HostResourceResponse, status,
};
use crate::application::services::host_resource_service::css_compat::{
    contains_layer_keyword, flatten_css_layers,
};
use crate::application::services::host_resource_service::routes::{
    ThirdPartyPathError, parse_third_party_asset_request_path,
};
use crate::domain::errors::DomainError;
use crate::infrastructure::third_party_assets::resolve_third_party_extension_asset;

const THIRD_PARTY_ALLOWED_METHODS: &str = "GET, HEAD, OPTIONS";
const MAX_MOBILE_INLINE_THIRD_PARTY_ASSET_BYTES: u64 = 32 * 1024 * 1024;
const THIRD_PARTY_LAYER_COMPAT_QUERY: &str = "ttCompat=layer";

fn should_apply_third_party_layer_compat(request: &HostResourceRequest<'_>) -> bool {
    request.query.is_some_and(|query| {
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

pub(crate) fn serve_third_party_asset(
    local_extensions_dir: &Path,
    global_extensions_dir: &Path,
    request: &HostResourceRequest<'_>,
) -> HostResourceResponse {
    match request.method {
        HostResourceMethod::Options => {
            return HostResourceResponse::no_content(THIRD_PARTY_ALLOWED_METHODS);
        }
        HostResourceMethod::Get | HostResourceMethod::Head => {}
        _ => return HostResourceResponse::method_not_allowed(THIRD_PARTY_ALLOWED_METHODS),
    }

    let parsed = match parse_third_party_asset_request_path(request.path) {
        Ok(Some(value)) => value,
        Ok(None) => return HostResourceResponse::plain_text(status::NOT_FOUND, "Not Found"),
        Err(ThirdPartyPathError::MissingExtension | ThirdPartyPathError::MissingAssetPath) => {
            return HostResourceResponse::plain_text(status::NOT_FOUND, "Not Found");
        }
        Err(ThirdPartyPathError::InvalidPath) => {
            return HostResourceResponse::plain_text(
                status::BAD_REQUEST,
                "Invalid third-party asset path",
            );
        }
    };

    match resolve_third_party_extension_asset(
        local_extensions_dir,
        global_extensions_dir,
        &parsed.extension_folder,
        &parsed.relative_path,
    ) {
        Ok(resolved) => {
            if request.method == HostResourceMethod::Head {
                return HostResourceResponse::bytes(status::OK, Vec::new(), &resolved.mime_type);
            }

            if cfg!(mobile) && resolved.size_bytes > MAX_MOBILE_INLINE_THIRD_PARTY_ASSET_BYTES {
                tracing::warn!(
                    "Rejected large third-party asset ({} bytes): {}/{}",
                    resolved.size_bytes,
                    parsed.extension_folder,
                    parsed.relative_path_display
                );
                return HostResourceResponse::plain_text(
                    status::PAYLOAD_TOO_LARGE,
                    "Third-party asset is too large to load on mobile.",
                );
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

                    tracing::debug!(
                        "Third-party asset hit: {}/{}",
                        parsed.extension_folder,
                        parsed.relative_path_display
                    );
                    HostResourceResponse::bytes(status::OK, bytes, &resolved.mime_type)
                }
                Err(error) => HostResourceResponse::plain_text(
                    status::INTERNAL_SERVER_ERROR,
                    &format!("Failed to read third-party asset: {}", error),
                ),
            }
        }
        Err(DomainError::NotFound(_)) => {
            tracing::debug!(
                "Third-party asset 404: {}/{}",
                parsed.extension_folder,
                parsed.relative_path_display
            );
            HostResourceResponse::plain_text(status::NOT_FOUND, "Not Found")
        }
        Err(error) => {
            HostResourceResponse::plain_text(status::INTERNAL_SERVER_ERROR, &error.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::services::host_resource_service::contract::{
        HostResourceHeaders, HostResourceMethod, HostResourceRequest, header,
    };
    use std::path::PathBuf;

    fn request(method: HostResourceMethod, uri: &'static str) -> HostResourceRequest<'static> {
        let (path, query) = uri
            .split_once('?')
            .map_or((uri, None), |(path, query)| (path, Some(query)));
        HostResourceRequest::new(method, path, query, HostResourceHeaders::empty())
    }

    fn header<'a>(response: &'a HostResourceResponse, name: &str) -> Option<&'a str> {
        response
            .headers
            .iter()
            .find(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

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
        let response = serve_third_party_asset(
            &temp.path,
            &temp.path,
            &request(
                HostResourceMethod::Other,
                "/scripts/extensions/third-party/mobile/manifest.json",
            ),
        );

        assert_eq!(response.status, status::METHOD_NOT_ALLOWED);
        assert_eq!(
            header(&response, header::ALLOW),
            Some(THIRD_PARTY_ALLOWED_METHODS)
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

        let response = serve_third_party_asset(
            &local_root,
            &global_root,
            &request(
                HostResourceMethod::Head,
                "/scripts/extensions/third-party/mobile/manifest.json",
            ),
        );

        assert_eq!(response.status, status::OK);
        assert_eq!(
            header(&response, header::CONTENT_TYPE),
            Some("application/json")
        );
        assert!(response.body.is_empty());
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

        let response = serve_third_party_asset(
            &local_root,
            &global_root,
            &request(
                HostResourceMethod::Get,
                "/scripts/extensions/third-party/mobile//style.css",
            ),
        );

        assert_eq!(response.status, status::OK);
        assert_eq!(header(&response, header::CONTENT_TYPE), Some("text/css"));
        assert_eq!(response.body, b".example { color: red; }");
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

        let response = serve_third_party_asset(
            &local_root,
            &global_root,
            &request(
                HostResourceMethod::Get,
                "/scripts/extensions/third-party/mobile/style.css?ttCompat=layer",
            ),
        );

        assert_eq!(response.status, status::OK);
        assert_eq!(header(&response, header::CONTENT_TYPE), Some("text/css"));
        assert_eq!(response.body, b".x{color:red;}");
    }
}
