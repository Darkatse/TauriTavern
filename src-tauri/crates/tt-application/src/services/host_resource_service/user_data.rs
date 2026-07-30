use super::range::{RangeHeaderError, parse_single_range_header};
use super::response::{self, RepresentationMetadata, RetrievalDecision};
use super::{HostResourceDeliveryCapabilities, HostResourceResponse};
use crate::client_asset_paths::{
    UserDataAssetKind, UserDataPathError, parse_user_data_asset_request_path,
};
use http::header::RANGE;
use http::{Method, Request, StatusCode};
use tt_ports::host_resource::{
    HostResourceAssetStore, HostResourceSourceRequest, HostResourceStoreError,
};

const USER_DATA_ALLOWED_METHODS: &str = "GET, HEAD, OPTIONS";

pub(super) fn serve_user_data_asset(
    store: &dyn HostResourceAssetStore,
    request: &Request<Vec<u8>>,
    delivery: HostResourceDeliveryCapabilities,
) -> HostResourceResponse {
    match *request.method() {
        Method::OPTIONS => {
            return response::no_content(USER_DATA_ALLOWED_METHODS);
        }
        Method::GET | Method::HEAD => {}
        _ => return response::method_not_allowed(USER_DATA_ALLOWED_METHODS),
    }

    let parsed = match parse_user_data_asset_request_path(request.uri().path()) {
        Ok(Some(value)) => value,
        Ok(None) => return response::error(StatusCode::NOT_FOUND, "Not Found"),
        Err(UserDataPathError::MissingAssetPath) => {
            return response::error(StatusCode::NOT_FOUND, "Not Found");
        }
        Err(UserDataPathError::InvalidPath) => {
            return response::error(StatusCode::BAD_REQUEST, "Invalid asset path");
        }
    };

    let opened = match store.open(HostResourceSourceRequest::UserData {
        kind: parsed.kind,
        relative_path: &parsed.relative_path,
    }) {
        Ok(opened) => opened,
        Err(error) => return store_error_response(error),
    };
    let metadata = match RepresentationMetadata::raw(&opened.metadata, None) {
        Ok(metadata) => metadata,
        Err(error) => return store_error_response(error),
    };
    let total_size = opened.metadata.content_length;

    match response::decide_retrieval(request, &metadata, delivery) {
        RetrievalDecision::NotModified => {
            return response::with_accept_ranges(response::not_modified(&metadata));
        }
        RetrievalDecision::Head => {
            return response::with_accept_ranges(response::head(&metadata));
        }
        RetrievalDecision::Full => {
            return read_full_response(opened, &metadata);
        }
        RetrievalDecision::Continue => {}
    }

    let webview_reapplies_background_video_range = delivery.webview_reapplies_range_semantics()
        && parsed.kind == UserDataAssetKind::Background
        && opened.metadata.content_type.starts_with("video/");

    let mut range_headers = request.headers().get_all(RANGE).iter();
    if let Some(range_header) = range_headers.next() {
        if range_headers.next().is_some() {
            return response::range_not_satisfiable(
                "Multiple ranges are not supported",
                total_size,
            );
        }
        if !response::if_range_allows_range(request, &metadata) {
            return read_full_response(opened, &metadata);
        }

        let header_value = match range_header.to_str() {
            Ok(value) => value,
            Err(_) => {
                return response::range_not_satisfiable("Invalid Range header", total_size);
            }
        };

        let range = match parse_single_range_header(header_value, total_size) {
            Ok(value) => value,
            Err(RangeHeaderError::Invalid) => {
                return response::range_not_satisfiable("Invalid Range header", total_size);
            }
            Err(RangeHeaderError::Unsatisfiable) => {
                return response::range_not_satisfiable("Range not satisfiable", total_size);
            }
        };

        if webview_reapplies_background_video_range && range.start() != 0 {
            return match opened.read(None) {
                Ok(bytes) => {
                    let response = response::partial(
                        &metadata,
                        bytes,
                        format!("bytes {}-{}/{}", range.start(), range.end(), total_size),
                        range.byte_len(),
                    );
                    tracing::debug!(
                        "User data asset Android video range workaround hit: {}",
                        parsed.relative_path_display
                    );
                    response::with_accept_ranges(response)
                }
                Err(error) => response::with_accept_ranges(store_error_response(error)),
            };
        }

        if usize::try_from(range.byte_len()).is_err() {
            return response::with_accept_ranges(response::error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Range is too large to serve",
            ));
        }

        return match opened.read(Some(range)) {
            Ok(bytes) => {
                let response = response::partial(
                    &metadata,
                    bytes,
                    format!("bytes {}-{}/{}", range.start(), range.end(), total_size),
                    range.byte_len(),
                );

                tracing::debug!(
                    "User data asset range hit: {:?}/{}",
                    parsed.kind,
                    parsed.relative_path_display
                );
                response::with_accept_ranges(response)
            }
            Err(error) => response::with_accept_ranges(store_error_response(error)),
        };
    }

    match opened.read(None) {
        Ok(bytes) => {
            tracing::debug!(
                "User data asset hit: {:?}/{}",
                parsed.kind,
                parsed.relative_path_display
            );
            response::with_accept_ranges(response::ok(&metadata, bytes))
        }
        Err(error) => response::with_accept_ranges(store_error_response(error)),
    }
}

fn read_full_response(
    opened: tt_ports::host_resource::OpenedHostResource,
    metadata: &RepresentationMetadata,
) -> HostResourceResponse {
    match opened.read(None) {
        Ok(bytes) => response::with_accept_ranges(response::ok(metadata, bytes)),
        Err(error) => response::with_accept_ranges(store_error_response(error)),
    }
}

fn store_error_response(error: HostResourceStoreError) -> HostResourceResponse {
    response::store_error(error, "Not Found")
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use http::header::{CONTENT_LENGTH, CONTENT_RANGE, ETAG, IF_RANGE, RANGE};
    use tt_ports::host_resource::{
        HostResourceSourceRequest, HostResourceStoreError, OpenedHostResource,
    };

    use super::*;
    use crate::services::host_resource_service::test_support;

    struct Store {
        opens: Arc<AtomicUsize>,
    }

    impl HostResourceAssetStore for Store {
        fn open(
            &self,
            request: HostResourceSourceRequest<'_>,
        ) -> Result<OpenedHostResource, HostResourceStoreError> {
            self.opens.fetch_add(1, Ordering::Relaxed);
            let HostResourceSourceRequest::UserData { kind, .. } = request else {
                unreachable!()
            };
            let mime_type = if kind == UserDataAssetKind::Background {
                "video/mp4"
            } else {
                "application/octet-stream"
            };
            Ok(test_support::opened(
                b"abcd",
                mime_type,
                Arc::new(AtomicUsize::new(0)),
            ))
        }
    }

    fn store() -> Store {
        Store {
            opens: Arc::new(AtomicUsize::new(0)),
        }
    }

    #[test]
    fn serves_user_data_ranges() {
        let mut request = test_support::request(Method::GET, "/backgrounds/a.mp4");
        request
            .headers_mut()
            .insert(RANGE, "bytes=1-2".parse().expect("range"));

        let response = serve_user_data_asset(
            &store(),
            &request,
            HostResourceDeliveryCapabilities::new(true, false),
        );

        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(response.body(), b"bc");
        assert_eq!(response.headers()[CONTENT_RANGE], "bytes 1-2/4");
    }

    #[test]
    fn rejects_multiple_range_field_lines() {
        let mut request = test_support::request(Method::GET, "/backgrounds/a.mp4");
        request
            .headers_mut()
            .append(RANGE, "bytes=0-0".parse().expect("first range"));
        request
            .headers_mut()
            .append(RANGE, "bytes=2-2".parse().expect("second range"));

        let response = serve_user_data_asset(
            &store(),
            &request,
            HostResourceDeliveryCapabilities::new(true, false),
        );

        assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE);
        assert_eq!(response.headers()[CONTENT_RANGE], "bytes */4");
    }

    #[test]
    fn android_background_video_range_returns_full_body_with_range_headers() {
        let mut request = test_support::request(Method::GET, "/backgrounds/a.mp4");
        request
            .headers_mut()
            .insert(RANGE, "bytes=1-2".parse().expect("range"));

        let response = serve_user_data_asset(
            &store(),
            &request,
            HostResourceDeliveryCapabilities::new(false, true),
        );

        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(response.body(), b"abcd");
        assert_eq!(response.headers()[CONTENT_LENGTH], "2");
    }

    #[test]
    fn weak_if_range_returns_full_representation() {
        let store = store();
        let first = serve_user_data_asset(
            &store,
            &test_support::request(Method::GET, "/backgrounds/a.mp4"),
            HostResourceDeliveryCapabilities::new(true, false),
        );
        let mut request = test_support::request(Method::GET, "/backgrounds/a.mp4");
        request
            .headers_mut()
            .insert(RANGE, "bytes=1-2".parse().expect("range"));
        request
            .headers_mut()
            .insert(IF_RANGE, first.headers()[ETAG].clone());

        let response = serve_user_data_asset(
            &store,
            &request,
            HostResourceDeliveryCapabilities::new(true, false),
        );

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.body(), b"abcd");
    }

    #[test]
    fn rejects_invalid_user_data_paths_before_store_access() {
        let store = store();
        let response = serve_user_data_asset(
            &store,
            &test_support::request(Method::GET, "/backgrounds/%2Fbad.png"),
            HostResourceDeliveryCapabilities::new(true, false),
        );

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(store.opens.load(Ordering::Relaxed), 0);
    }
}
