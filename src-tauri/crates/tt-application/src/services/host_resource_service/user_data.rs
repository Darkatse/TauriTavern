use super::contract::{
    HostResourceMethod, HostResourceRequest, HostResourceResponse, header, status,
};
use super::ports::{HostResourceAssetStore, HostResourceStoreError};
use super::range::{RangeHeaderError, parse_single_range_header};
use crate::client_asset_paths::{
    UserDataAssetKind, UserDataPathError, parse_user_data_asset_request_path,
};

const USER_DATA_ALLOWED_METHODS: &str = "GET, HEAD, OPTIONS";

#[derive(Clone, Copy)]
pub(crate) struct UserDataAssetRequestPolicy {
    android_webview_reapplies_range_semantics: bool,
}

impl UserDataAssetRequestPolicy {
    pub(crate) const fn for_current_platform() -> Self {
        Self {
            android_webview_reapplies_range_semantics: cfg!(target_os = "android"),
        }
    }

    #[cfg(test)]
    const fn android_workaround(enabled: bool) -> Self {
        Self {
            android_webview_reapplies_range_semantics: enabled,
        }
    }
}

pub(super) fn serve_user_data_asset(
    store: &dyn HostResourceAssetStore,
    request: &HostResourceRequest<'_>,
    policy: UserDataAssetRequestPolicy,
) -> HostResourceResponse {
    match request.method {
        HostResourceMethod::Options => {
            return HostResourceResponse::no_content(USER_DATA_ALLOWED_METHODS);
        }
        HostResourceMethod::Get | HostResourceMethod::Head => {}
        _ => return HostResourceResponse::method_not_allowed(USER_DATA_ALLOWED_METHODS),
    }

    let parsed = match parse_user_data_asset_request_path(request.path) {
        Ok(Some(value)) => value,
        Ok(None) => return HostResourceResponse::plain_text(status::NOT_FOUND, "Not Found"),
        Err(UserDataPathError::MissingAssetPath) => {
            return HostResourceResponse::plain_text(status::NOT_FOUND, "Not Found");
        }
        Err(UserDataPathError::InvalidPath) => {
            return HostResourceResponse::plain_text(status::BAD_REQUEST, "Invalid asset path");
        }
    };

    let stat = match store.stat_user_data_asset(parsed.kind, &parsed.relative_path) {
        Ok(stat) => stat,
        Err(error) => return store_error_response(error),
    };

    if request.method == HostResourceMethod::Head {
        return with_accept_ranges(
            HostResourceResponse::bytes(status::OK, Vec::new(), &stat.mime_type)
                .with_header(header::CONTENT_LENGTH, stat.len.to_string()),
        );
    }

    let is_android_background_video = policy.android_webview_reapplies_range_semantics
        && parsed.kind == UserDataAssetKind::Background
        && stat.mime_type.starts_with("video/");

    if let Some(range_header) = request.headers.get(header::RANGE) {
        let header_value = match std::str::from_utf8(range_header) {
            Ok(value) => value,
            Err(_) => {
                return range_not_satisfiable("Invalid Range header", stat.len);
            }
        };

        let range = match parse_single_range_header(header_value, stat.len) {
            Ok(value) => value,
            Err(RangeHeaderError::Invalid) => {
                return range_not_satisfiable("Invalid Range header", stat.len);
            }
            Err(RangeHeaderError::Unsatisfiable) => {
                return range_not_satisfiable("Range not satisfiable", stat.len);
            }
        };

        if is_android_background_video && range.start != 0 {
            return match store.read_user_data_asset(parsed.kind, &parsed.relative_path) {
                Ok(bytes) => {
                    let response = HostResourceResponse::bytes(
                        status::PARTIAL_CONTENT,
                        bytes,
                        &stat.mime_type,
                    )
                    .with_header(
                        header::CONTENT_RANGE,
                        format!("bytes {}-{}/{}", range.start, range.end, stat.len),
                    )
                    .with_header(header::CONTENT_LENGTH, range.len().to_string());
                    tracing::debug!(
                        "User data asset Android video range workaround hit: {}",
                        parsed.relative_path_display
                    );
                    with_accept_ranges(response)
                }
                Err(error) => with_accept_ranges(store_error_response(error)),
            };
        }

        if usize::try_from(range.len()).is_err() {
            return with_accept_ranges(HostResourceResponse::plain_text(
                status::INTERNAL_SERVER_ERROR,
                "Range is too large to serve",
            ));
        }

        return match store.read_user_data_asset_range(parsed.kind, &parsed.relative_path, range) {
            Ok(bytes) => {
                let response =
                    HostResourceResponse::bytes(status::PARTIAL_CONTENT, bytes, &stat.mime_type)
                        .with_header(
                            header::CONTENT_RANGE,
                            format!("bytes {}-{}/{}", range.start, range.end, stat.len),
                        )
                        .with_header(header::CONTENT_LENGTH, range.len().to_string());

                tracing::debug!(
                    "User data asset range hit: {:?}/{}",
                    parsed.kind,
                    parsed.relative_path_display
                );
                with_accept_ranges(response)
            }
            Err(error) => with_accept_ranges(store_error_response(error)),
        };
    }

    match store.read_user_data_asset(parsed.kind, &parsed.relative_path) {
        Ok(bytes) => {
            tracing::debug!(
                "User data asset hit: {:?}/{}",
                parsed.kind,
                parsed.relative_path_display
            );
            with_accept_ranges(
                HostResourceResponse::bytes(status::OK, bytes, &stat.mime_type)
                    .with_header(header::CONTENT_LENGTH, stat.len.to_string()),
            )
        }
        Err(error) => with_accept_ranges(store_error_response(error)),
    }
}

fn with_accept_ranges(response: HostResourceResponse) -> HostResourceResponse {
    response.with_header(header::ACCEPT_RANGES, "bytes")
}

fn range_not_satisfiable(message: &str, total_size: u64) -> HostResourceResponse {
    with_accept_ranges(
        HostResourceResponse::plain_text(status::RANGE_NOT_SATISFIABLE, message)
            .with_header(header::CONTENT_RANGE, format!("bytes */{}", total_size)),
    )
}

fn store_error_response(error: HostResourceStoreError) -> HostResourceResponse {
    match error {
        HostResourceStoreError::NotFound(_) => {
            HostResourceResponse::plain_text(status::NOT_FOUND, "Not Found")
        }
        HostResourceStoreError::Forbidden(message) => {
            HostResourceResponse::plain_text(status::FORBIDDEN, &message)
        }
        HostResourceStoreError::Internal(message) => {
            HostResourceResponse::plain_text(status::INTERNAL_SERVER_ERROR, &message)
        }
        HostResourceStoreError::PayloadTooLarge { .. } => HostResourceResponse::plain_text(
            status::PAYLOAD_TOO_LARGE,
            "Host resource is too large to load.",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::host_resource_service::contract::{
        HostResourceHeader, HostResourceHeaders,
    };
    use crate::services::host_resource_service::ports::{
        HostResourceBinaryAsset, HostResourceFileStat, ThumbnailAssetRequest,
    };
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
            unreachable!()
        }

        fn read_third_party_asset(
            &self,
            _extension_folder: &str,
            _relative_path: &Path,
            _max_len: Option<u64>,
        ) -> Result<HostResourceBinaryAsset, HostResourceStoreError> {
            unreachable!()
        }

        fn stat_user_data_asset(
            &self,
            kind: UserDataAssetKind,
            _relative_path: &Path,
        ) -> Result<HostResourceFileStat, HostResourceStoreError> {
            let mime_type = if kind == UserDataAssetKind::Background {
                "video/mp4"
            } else {
                "application/octet-stream"
            };
            Ok(HostResourceFileStat {
                len: 4,
                mime_type: mime_type.to_string(),
            })
        }

        fn read_user_data_asset(
            &self,
            _kind: UserDataAssetKind,
            _relative_path: &Path,
        ) -> Result<Vec<u8>, HostResourceStoreError> {
            Ok(b"abcd".to_vec())
        }

        fn read_user_data_asset_range(
            &self,
            _kind: UserDataAssetKind,
            _relative_path: &Path,
            range: super::super::range::ByteRange,
        ) -> Result<Vec<u8>, HostResourceStoreError> {
            Ok(b"abcd"[range.start as usize..=range.end as usize].to_vec())
        }

        fn read_thumbnail_asset(
            &self,
            _request: ThumbnailAssetRequest,
        ) -> Result<HostResourceBinaryAsset, HostResourceStoreError> {
            unreachable!()
        }
    }

    fn request(method: HostResourceMethod, uri: &'static str) -> HostResourceRequest<'static> {
        request_with_headers(method, uri, &[])
    }

    fn request_with_headers<'a>(
        method: HostResourceMethod,
        uri: &'static str,
        headers: &'a [HostResourceHeader<'a>],
    ) -> HostResourceRequest<'a> {
        let (path, query) = uri
            .split_once('?')
            .map_or((uri, None), |(path, query)| (path, Some(query)));
        HostResourceRequest::new(method, path, query, HostResourceHeaders::new(headers))
    }

    fn header<'a>(response: &'a HostResourceResponse, name: &str) -> Option<&'a str> {
        response
            .headers
            .iter()
            .find(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    #[test]
    fn serves_user_data_ranges() {
        let headers = [HostResourceHeader {
            name: header::RANGE,
            value: b"bytes=1-2",
        }];
        let request = request_with_headers(HostResourceMethod::Get, "/backgrounds/a.mp4", &headers);

        let response = serve_user_data_asset(
            &Store,
            &request,
            UserDataAssetRequestPolicy::android_workaround(false),
        );

        assert_eq!(response.status, status::PARTIAL_CONTENT);
        assert_eq!(response.body, b"bc");
        assert_eq!(
            header(&response, header::CONTENT_RANGE),
            Some("bytes 1-2/4")
        );
    }

    #[test]
    fn android_background_video_range_returns_full_body_with_range_headers() {
        let headers = [HostResourceHeader {
            name: header::RANGE,
            value: b"bytes=1-2",
        }];
        let request = request_with_headers(HostResourceMethod::Get, "/backgrounds/a.mp4", &headers);

        let response = serve_user_data_asset(
            &Store,
            &request,
            UserDataAssetRequestPolicy::android_workaround(true),
        );

        assert_eq!(response.status, status::PARTIAL_CONTENT);
        assert_eq!(response.body, b"abcd");
        assert_eq!(header(&response, header::CONTENT_LENGTH), Some("2"));
    }

    #[test]
    fn rejects_invalid_user_data_paths_before_store_access() {
        let response = serve_user_data_asset(
            &Store,
            &request(HostResourceMethod::Get, "/backgrounds/%2Fbad.png"),
            UserDataAssetRequestPolicy::android_workaround(false),
        );

        assert_eq!(response.status, status::BAD_REQUEST);
    }
}
