use http::{Method, Request};
use tt_ports::host_resource::{HostResourceAssetStore, HostResourceSourceRequest};

use super::response::{self, RepresentationMetadata, RetrievalDecision};
use super::{HostResourceDeliveryCapabilities, HostResourceResponse};

const USER_CSS_ALLOWED_METHODS: &str = "GET, HEAD, OPTIONS";
const USER_CSS_CONTENT_TYPE: &str = "text/css; charset=utf-8";

pub(super) fn serve_user_css(
    store: &dyn HostResourceAssetStore,
    request: &Request<Vec<u8>>,
    delivery: HostResourceDeliveryCapabilities,
) -> HostResourceResponse {
    match *request.method() {
        Method::OPTIONS => {
            return response::no_content(USER_CSS_ALLOWED_METHODS);
        }
        Method::GET | Method::HEAD => {}
        _ => return response::method_not_allowed(USER_CSS_ALLOWED_METHODS),
    }

    let opened = match store.open(HostResourceSourceRequest::UserCss) {
        Ok(opened) => opened,
        Err(error) => return response::store_error(error, "User CSS not found"),
    };
    let metadata = match RepresentationMetadata::raw(&opened.metadata, Some(USER_CSS_CONTENT_TYPE))
    {
        Ok(metadata) => metadata,
        Err(error) => return response::store_error(error, "User CSS not found"),
    };

    match response::decide_retrieval(request, &metadata, delivery) {
        RetrievalDecision::NotModified => response::not_modified(&metadata),
        RetrievalDecision::Head => response::head(&metadata),
        RetrievalDecision::Full | RetrievalDecision::Continue => match opened.read(None) {
            Ok(bytes) => response::ok(&metadata, bytes),
            Err(error) => response::store_error(error, "User CSS not found"),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use http::StatusCode;
    use http::header::{CACHE_CONTROL, CONTENT_LENGTH, ETAG, IF_NONE_MATCH};
    use tt_ports::host_resource::{
        HostResourceSourceRequest, HostResourceStoreError, OpenedHostResource,
    };

    use super::*;
    use crate::services::host_resource_service::test_support;

    struct Store {
        missing: bool,
        reads: Arc<AtomicUsize>,
    }

    impl HostResourceAssetStore for Store {
        fn open(
            &self,
            request: HostResourceSourceRequest<'_>,
        ) -> Result<OpenedHostResource, HostResourceStoreError> {
            assert!(matches!(request, HostResourceSourceRequest::UserCss));
            if self.missing {
                return Err(HostResourceStoreError::not_found("missing"));
            }
            Ok(test_support::opened(
                b"body {}",
                "text/css",
                Arc::clone(&self.reads),
            ))
        }
    }

    #[test]
    fn serves_revalidatable_css_without_reading_head_or_304() {
        let reads = Arc::new(AtomicUsize::new(0));
        let store = Store {
            missing: false,
            reads: Arc::clone(&reads),
        };
        let delivery = HostResourceDeliveryCapabilities::new(true, false);

        let get = serve_user_css(
            &store,
            &test_support::request(Method::GET, "/css/user.css"),
            delivery,
        );
        let head = serve_user_css(
            &store,
            &test_support::request(Method::HEAD, "/css/user.css"),
            delivery,
        );
        let mut conditional = test_support::request(Method::GET, "/css/user.css");
        conditional
            .headers_mut()
            .insert(IF_NONE_MATCH, get.headers()[ETAG].clone());
        let not_modified = serve_user_css(&store, &conditional, delivery);
        let full_fallback = serve_user_css(
            &store,
            &conditional,
            HostResourceDeliveryCapabilities::new(false, false),
        );

        assert_eq!(get.status(), StatusCode::OK);
        assert_eq!(get.body(), b"body {}");
        assert_eq!(get.headers()[CACHE_CONTROL], "private, no-cache");
        assert_eq!(head.status(), StatusCode::OK);
        assert!(head.body().is_empty());
        assert_eq!(head.headers()[CONTENT_LENGTH], "7");
        assert_eq!(not_modified.status(), StatusCode::NOT_MODIFIED);
        assert_eq!(full_fallback.status(), StatusCode::OK);
        assert_eq!(full_fallback.body(), b"body {}");
        assert_eq!(reads.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn returns_not_found_when_user_css_is_missing() {
        let store = Store {
            missing: true,
            reads: Arc::new(AtomicUsize::new(0)),
        };

        let response = serve_user_css(
            &store,
            &test_support::request(Method::GET, "/css/user.css"),
            HostResourceDeliveryCapabilities::new(true, false),
        );

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(response.headers()[CACHE_CONTROL], "no-store");
    }
}
