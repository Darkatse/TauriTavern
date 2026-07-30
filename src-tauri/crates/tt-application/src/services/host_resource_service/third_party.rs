use super::css_compat::{contains_layer_keyword, flatten_css_layers};
use super::response::{self, RepresentationMetadata, RetrievalDecision};
use super::{HostResourceDeliveryCapabilities, HostResourceResponse};
use crate::client_asset_paths::{ThirdPartyPathError, parse_third_party_asset_request_path};
use http::{Method, Request, StatusCode};
use tt_ports::host_resource::{
    HostResourceAssetStore, HostResourceSourceRequest, HostResourceStoreError,
};

const THIRD_PARTY_ALLOWED_METHODS: &str = "GET, HEAD, OPTIONS";
const MAX_MOBILE_INLINE_THIRD_PARTY_ASSET_BYTES: u64 = 32 * 1024 * 1024;
const THIRD_PARTY_LAYER_COMPAT_QUERY: &str = "ttCompat=layer";
const THIRD_PARTY_LAYER_COMPAT_REVISION: &[u8] = b"tt-compat-layer-v1";

pub(super) fn serve_third_party_asset(
    store: &dyn HostResourceAssetStore,
    request: &Request<Vec<u8>>,
    delivery: HostResourceDeliveryCapabilities,
) -> HostResourceResponse {
    match *request.method() {
        Method::OPTIONS => {
            return response::no_content(THIRD_PARTY_ALLOWED_METHODS);
        }
        Method::GET | Method::HEAD => {}
        _ => return response::method_not_allowed(THIRD_PARTY_ALLOWED_METHODS),
    }

    let parsed = match parse_third_party_asset_request_path(request.uri().path()) {
        Ok(Some(value)) => value,
        Ok(None) => return response::error(StatusCode::NOT_FOUND, "Not Found"),
        Err(ThirdPartyPathError::MissingExtension | ThirdPartyPathError::MissingAssetPath) => {
            return response::error(StatusCode::NOT_FOUND, "Not Found");
        }
        Err(ThirdPartyPathError::InvalidPath) => {
            return response::error(StatusCode::BAD_REQUEST, "Invalid third-party asset path");
        }
    };

    let opened = match store.open(HostResourceSourceRequest::ThirdParty {
        extension_folder: &parsed.extension_folder,
        relative_path: &parsed.relative_path,
    }) {
        Ok(opened) => opened,
        Err(HostResourceStoreError::NotFound(_)) => {
            return response::error(StatusCode::NOT_FOUND, "Not Found");
        }
        Err(error) => return store_error_response(error),
    };
    if cfg!(mobile) && opened.metadata.content_length > MAX_MOBILE_INLINE_THIRD_PARTY_ASSET_BYTES {
        tracing::warn!(
            "Rejected large third-party asset ({} bytes > {} bytes): {}/{}",
            opened.metadata.content_length,
            MAX_MOBILE_INLINE_THIRD_PARTY_ASSET_BYTES,
            parsed.extension_folder,
            parsed.relative_path_display
        );
        return response::error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "Third-party asset is too large to load on mobile.",
        );
    }

    let should_apply_layer_compat = opened.metadata.content_type == "text/css"
        && should_apply_third_party_layer_compat(request);
    let metadata = if should_apply_layer_compat {
        RepresentationMetadata::derived(&opened.metadata, THIRD_PARTY_LAYER_COMPAT_REVISION)
    } else {
        RepresentationMetadata::raw(&opened.metadata, None)
    };
    let metadata = match metadata {
        Ok(metadata) => metadata,
        Err(error) => return store_error_response(error),
    };

    match response::decide_retrieval(request, &metadata, delivery) {
        RetrievalDecision::NotModified => return response::not_modified(&metadata),
        RetrievalDecision::Head => return response::head(&metadata),
        RetrievalDecision::Full | RetrievalDecision::Continue => {}
    }

    let bytes = match opened.read(None) {
        Ok(bytes) if should_apply_layer_compat && contains_layer_keyword(&bytes) => {
            flatten_css_layers(&bytes)
        }
        Ok(bytes) => bytes,
        Err(error) => return store_error_response(error),
    };

    tracing::debug!(
        "Third-party asset hit: {}/{}",
        parsed.extension_folder,
        parsed.relative_path_display
    );
    response::ok(&metadata, bytes)
}

fn should_apply_third_party_layer_compat(request: &Request<Vec<u8>>) -> bool {
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

fn store_error_response(error: HostResourceStoreError) -> HostResourceResponse {
    response::store_error(error, "Not Found")
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use http::header::{CONTENT_LENGTH, ETAG, LAST_MODIFIED};
    use tt_ports::host_resource::{
        HostResourceSourceRequest, HostResourceStoreError, OpenedHostResource,
    };

    use super::*;
    use crate::services::host_resource_service::test_support;

    struct Store {
        reads: Arc<AtomicUsize>,
    }

    impl HostResourceAssetStore for Store {
        fn open(
            &self,
            request: HostResourceSourceRequest<'_>,
        ) -> Result<OpenedHostResource, HostResourceStoreError> {
            assert!(matches!(
                request,
                HostResourceSourceRequest::ThirdParty { .. }
            ));
            Ok(test_support::opened(
                b"@layer base { body {} }",
                "text/css",
                Arc::clone(&self.reads),
            ))
        }
    }

    #[test]
    fn raw_head_has_length_while_transformed_head_omits_unknown_length() {
        let reads = Arc::new(AtomicUsize::new(0));
        let store = Store {
            reads: Arc::clone(&reads),
        };
        let delivery = HostResourceDeliveryCapabilities::new(true, false);
        let raw = serve_third_party_asset(
            &store,
            &test_support::request(
                Method::HEAD,
                "/scripts/extensions/third-party/mobile/style.css",
            ),
            delivery,
        );
        let transformed = serve_third_party_asset(
            &store,
            &test_support::request(
                Method::HEAD,
                "/scripts/extensions/third-party/mobile/style.css?ttCompat=layer",
            ),
            delivery,
        );

        assert_eq!(raw.status(), StatusCode::OK);
        assert_eq!(raw.headers()[CONTENT_LENGTH], "23");
        assert!(raw.headers().contains_key(LAST_MODIFIED));
        assert!(!transformed.headers().contains_key(CONTENT_LENGTH));
        assert!(!transformed.headers().contains_key(LAST_MODIFIED));
        assert_ne!(raw.headers()[ETAG], transformed.headers()[ETAG]);
        assert_eq!(reads.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn applies_css_layer_compat_only_when_requested() {
        let reads = Arc::new(AtomicUsize::new(0));
        let response = serve_third_party_asset(
            &Store { reads },
            &test_support::request(
                Method::GET,
                "/scripts/extensions/third-party/mobile/style.css?ttCompat=layer",
            ),
            HostResourceDeliveryCapabilities::new(true, false),
        );

        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            !String::from_utf8(response.into_body())
                .expect("utf8")
                .contains("@layer")
        );
    }
}
