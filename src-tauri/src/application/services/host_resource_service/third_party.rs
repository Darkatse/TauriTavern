use super::contract::{HostResourceMethod, HostResourceRequest, HostResourceResponse, status};
use super::css_compat::{contains_layer_keyword, flatten_css_layers};
use super::ports::{HostResourceAssetStore, HostResourceStoreError};
use crate::application::client_asset_paths::{
    ThirdPartyPathError, parse_third_party_asset_request_path,
};

const THIRD_PARTY_ALLOWED_METHODS: &str = "GET, HEAD, OPTIONS";
const MAX_MOBILE_INLINE_THIRD_PARTY_ASSET_BYTES: u64 = 32 * 1024 * 1024;
const THIRD_PARTY_LAYER_COMPAT_QUERY: &str = "ttCompat=layer";

pub(super) fn serve_third_party_asset(
    store: &dyn HostResourceAssetStore,
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

    if request.method == HostResourceMethod::Head {
        return match store.stat_third_party_asset(&parsed.extension_folder, &parsed.relative_path) {
            Ok(stat) => HostResourceResponse::bytes(status::OK, Vec::new(), &stat.mime_type)
                .with_header(
                    super::contract::header::CONTENT_LENGTH,
                    stat.len.to_string(),
                ),
            Err(HostResourceStoreError::NotFound(_)) => {
                tracing::debug!(
                    "Third-party asset 404: {}/{}",
                    parsed.extension_folder,
                    parsed.relative_path_display
                );
                HostResourceResponse::plain_text(status::NOT_FOUND, "Not Found")
            }
            Err(error) => store_error_response(error),
        };
    }

    let max_len = if cfg!(mobile) {
        Some(MAX_MOBILE_INLINE_THIRD_PARTY_ASSET_BYTES)
    } else {
        None
    };
    let asset = match store.read_third_party_asset(
        &parsed.extension_folder,
        &parsed.relative_path,
        max_len,
    ) {
        Ok(asset) => asset,
        Err(HostResourceStoreError::NotFound(_)) => {
            return HostResourceResponse::plain_text(status::NOT_FOUND, "Not Found");
        }
        Err(HostResourceStoreError::PayloadTooLarge {
            size_bytes,
            limit_bytes,
        }) => {
            tracing::warn!(
                "Rejected large third-party asset ({} bytes > {} bytes): {}/{}",
                size_bytes,
                limit_bytes,
                parsed.extension_folder,
                parsed.relative_path_display
            );
            return HostResourceResponse::plain_text(
                status::PAYLOAD_TOO_LARGE,
                "Third-party asset is too large to load on mobile.",
            );
        }
        Err(error) => return store_error_response(error),
    };

    let should_apply_layer_compat =
        asset.mime_type == "text/css" && should_apply_third_party_layer_compat(request);
    let bytes = if should_apply_layer_compat && contains_layer_keyword(&asset.bytes) {
        flatten_css_layers(&asset.bytes)
    } else {
        asset.bytes
    };

    tracing::debug!(
        "Third-party asset hit: {}/{}",
        parsed.extension_folder,
        parsed.relative_path_display
    );
    HostResourceResponse::bytes(status::OK, bytes, &asset.mime_type)
}

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

fn store_error_response(error: HostResourceStoreError) -> HostResourceResponse {
    match error {
        HostResourceStoreError::Forbidden(message) => {
            HostResourceResponse::plain_text(status::FORBIDDEN, &message)
        }
        HostResourceStoreError::Internal(message) => {
            HostResourceResponse::plain_text(status::INTERNAL_SERVER_ERROR, &message)
        }
        HostResourceStoreError::PayloadTooLarge { .. } => HostResourceResponse::plain_text(
            status::PAYLOAD_TOO_LARGE,
            "Third-party asset is too large to load on mobile.",
        ),
        HostResourceStoreError::NotFound(_) => {
            HostResourceResponse::plain_text(status::NOT_FOUND, "Not Found")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::client_asset_paths::UserDataAssetKind;
    use crate::application::services::host_resource_service::contract::{
        HostResourceHeaders, header,
    };
    use crate::application::services::host_resource_service::ports::{
        HostResourceBinaryAsset, HostResourceFileStat, ThumbnailAssetRequest,
    };
    use crate::application::services::host_resource_service::range::ByteRange;
    use std::path::Path;

    struct Store;

    impl HostResourceAssetStore for Store {
        fn read_user_css(&self) -> Result<Vec<u8>, HostResourceStoreError> {
            unreachable!()
        }

        fn stat_third_party_asset(
            &self,
            _extension_folder: &str,
            _relative_path: &Path,
        ) -> Result<HostResourceFileStat, HostResourceStoreError> {
            Ok(HostResourceFileStat {
                len: 25,
                mime_type: "text/css".to_string(),
            })
        }

        fn read_third_party_asset(
            &self,
            _extension_folder: &str,
            _relative_path: &Path,
            _max_len: Option<u64>,
        ) -> Result<HostResourceBinaryAsset, HostResourceStoreError> {
            Ok(HostResourceBinaryAsset {
                bytes: b"@layer base { body {} }".to_vec(),
                mime_type: "text/css".to_string(),
            })
        }

        fn stat_user_data_asset(
            &self,
            _kind: UserDataAssetKind,
            _relative_path: &Path,
        ) -> Result<HostResourceFileStat, HostResourceStoreError> {
            unreachable!()
        }

        fn read_user_data_asset(
            &self,
            _kind: UserDataAssetKind,
            _relative_path: &Path,
        ) -> Result<Vec<u8>, HostResourceStoreError> {
            unreachable!()
        }

        fn read_user_data_asset_range(
            &self,
            _kind: UserDataAssetKind,
            _relative_path: &Path,
            _range: ByteRange,
        ) -> Result<Vec<u8>, HostResourceStoreError> {
            unreachable!()
        }

        fn read_thumbnail_asset(
            &self,
            _request: ThumbnailAssetRequest,
        ) -> Result<HostResourceBinaryAsset, HostResourceStoreError> {
            unreachable!()
        }
    }

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

    #[test]
    fn head_responses_keep_content_type_and_clear_body() {
        let response = serve_third_party_asset(
            &Store,
            &request(
                HostResourceMethod::Head,
                "/scripts/extensions/third-party/mobile/style.css",
            ),
        );

        assert_eq!(response.status, status::OK);
        assert!(response.body.is_empty());
        assert_eq!(header(&response, header::CONTENT_TYPE), Some("text/css"));
    }

    #[test]
    fn applies_css_layer_compat_only_when_requested() {
        let response = serve_third_party_asset(
            &Store,
            &request(
                HostResourceMethod::Get,
                "/scripts/extensions/third-party/mobile/style.css?ttCompat=layer",
            ),
        );

        assert_eq!(response.status, status::OK);
        assert!(
            !String::from_utf8(response.body)
                .expect("utf8")
                .contains("@layer")
        );
    }
}
