use super::range::{RangeHeaderError, parse_single_range_header};
use super::response::{self, RepresentationMetadata, RetrievalDecision};
use super::{HostResourceDeliveryCapabilities, HostResourceResponse};
use crate::client_asset_paths::{
    UserDataAssetRequestPath, UserDataPathError, parse_user_data_asset_request_path,
};
use http::header::RANGE;
use http::{Method, Request, StatusCode};
use tt_ports::host_resource::{
    HostResourceAssetStore, HostResourceSourceRequest, HostResourceStoreError, OpenedHostResource,
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
            return full_response(&parsed, opened, &metadata, delivery);
        }
        RetrievalDecision::Continue => {}
    }

    let mut range_headers = request.headers().get_all(RANGE).iter();
    if let Some(range_header) = range_headers.next() {
        if range_headers.next().is_some() {
            return response::range_not_satisfiable(
                "Multiple ranges are not supported",
                total_size,
            );
        }
        if !response::if_range_allows_range(request, &metadata) {
            return full_response(&parsed, opened, &metadata, delivery);
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

        return match response::body(opened, Some(range), delivery) {
            Ok(body) => {
                let response = response::partial(
                    &metadata,
                    body,
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

    full_response(&parsed, opened, &metadata, delivery)
}

fn full_response(
    parsed: &UserDataAssetRequestPath,
    opened: OpenedHostResource,
    metadata: &RepresentationMetadata,
    delivery: HostResourceDeliveryCapabilities,
) -> HostResourceResponse {
    match response::body(opened, None, delivery) {
        Ok(body) => {
            tracing::debug!(
                "User data asset hit: {:?}/{}",
                parsed.kind,
                parsed.relative_path_display
            );
            response::with_accept_ranges(response::ok(metadata, body))
        }
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

    use http::header::{CONTENT_RANGE, ETAG, IF_RANGE, RANGE};
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
            _request: HostResourceSourceRequest<'_>,
        ) -> Result<OpenedHostResource, HostResourceStoreError> {
            self.opens.fetch_add(1, Ordering::Relaxed);
            Ok(test_support::opened(
                b"abcd",
                "video/mp4",
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
            HostResourceDeliveryCapabilities::new(true),
        );

        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(test_support::body_bytes(&response), b"bc");
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
            HostResourceDeliveryCapabilities::new(true),
        );

        assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE);
        assert_eq!(response.headers()[CONTENT_RANGE], "bytes */4");
    }

    #[test]
    fn weak_if_range_returns_full_representation() {
        let store = store();
        let first = serve_user_data_asset(
            &store,
            &test_support::request(Method::GET, "/backgrounds/a.mp4"),
            HostResourceDeliveryCapabilities::new(true),
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
            HostResourceDeliveryCapabilities::new(true),
        );

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(test_support::body_bytes(&response), b"abcd");
    }

    #[test]
    fn rejects_invalid_user_data_paths_before_store_access() {
        let store = store();
        let response = serve_user_data_asset(
            &store,
            &test_support::request(Method::GET, "/backgrounds/%2Fbad.png"),
            HostResourceDeliveryCapabilities::new(true),
        );

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(store.opens.load(Ordering::Relaxed), 0);
    }
}
