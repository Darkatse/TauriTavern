use std::borrow::Cow;
#[cfg(any(dev, debug_assertions, test))]
use std::sync::Arc;

#[cfg(any(dev, debug_assertions))]
use tauri::Manager;
use tauri::http::header::{
    ACCEPT_RANGES, ALLOW, CACHE_CONTROL, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, DATE, ETAG,
    EXPIRES, LAST_MODIFIED, PRAGMA, VARY,
};
use tauri::http::header::{
    CONTENT_DISPOSITION, CONTENT_ENCODING, CONTENT_LANGUAGE, CONTENT_LOCATION,
};
use tt_application::services::host_resource_service::{
    HostResourceDeliveryCapabilities, HostResourceResponse, HostResourceService,
};

const WRY_DELIVERY: HostResourceDeliveryCapabilities = HostResourceDeliveryCapabilities::new(
    !cfg!(target_os = "android"),
    cfg!(target_os = "android"),
);
#[cfg(any(dev, debug_assertions))]
const DEV_IPC_DELIVERY: HostResourceDeliveryCapabilities =
    HostResourceDeliveryCapabilities::new(!cfg!(target_os = "android"), false);

pub(crate) fn handle_tauri_web_resource_request(
    host_resources: &HostResourceService,
    request: &tauri::http::Request<Vec<u8>>,
    response: &mut tauri::http::Response<Cow<'static, [u8]>>,
) {
    if let Some(host_response) = dispatch_tauri_host_resource_request(host_resources, request) {
        apply_host_resource_response(response, host_response);
    }
}

pub(crate) fn dispatch_tauri_host_resource_request(
    host_resources: &HostResourceService,
    request: &tauri::http::Request<Vec<u8>>,
) -> Option<HostResourceResponse> {
    if !is_tauri_app_uri(request.uri()) {
        return None;
    }
    host_resources.try_serve(request, WRY_DELIVERY)
}

#[cfg(any(dev, debug_assertions))]
pub(crate) fn serve_dev_protocol_resource_from_app<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    request: &tauri::http::Request<Vec<u8>>,
) -> HostResourceResponse {
    serve_dev_web_resource_from_app(app_handle, request, WRY_DELIVERY)
}

#[cfg(any(dev, debug_assertions))]
pub(crate) fn serve_dev_ipc_resource_from_app<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    request: &tauri::http::Request<Vec<u8>>,
) -> HostResourceResponse {
    serve_dev_web_resource_from_app(app_handle, request, DEV_IPC_DELIVERY)
}

#[cfg(any(dev, debug_assertions))]
fn serve_dev_web_resource_from_app<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    request: &tauri::http::Request<Vec<u8>>,
    delivery: HostResourceDeliveryCapabilities,
) -> HostResourceResponse {
    let host_resources = app_handle.state::<Arc<HostResourceService>>();
    host_resources.serve(request, delivery)
}

fn is_tauri_app_uri(uri: &tauri::http::Uri) -> bool {
    uri.scheme_str() == Some("tauri")
        && uri
            .authority()
            .is_some_and(|authority| authority.as_str() == "localhost")
}

pub(crate) fn apply_host_resource_response(
    response: &mut tauri::http::Response<Cow<'static, [u8]>>,
    host_response: HostResourceResponse,
) {
    let (parts, body) = host_response.into_parts();
    *response.status_mut() = parts.status;

    clear_replaced_host_resource_headers(response);
    response.headers_mut().extend(parts.headers);

    *response.body_mut() = Cow::Owned(body);
}

fn clear_replaced_host_resource_headers(response: &mut tauri::http::Response<Cow<'static, [u8]>>) {
    let headers = response.headers_mut();
    for name in [
        ACCEPT_RANGES,
        ALLOW,
        CACHE_CONTROL,
        CONTENT_DISPOSITION,
        CONTENT_ENCODING,
        CONTENT_LANGUAGE,
        CONTENT_LENGTH,
        CONTENT_LOCATION,
        CONTENT_RANGE,
        CONTENT_TYPE,
        DATE,
        ETAG,
        EXPIRES,
        LAST_MODIFIED,
        PRAGMA,
        VARY,
    ] {
        headers.remove(name);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use tauri::http::HeaderValue;
    use tauri::http::header::{CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_SECURITY_POLICY, VARY};
    use tauri::http::{Request, Response, StatusCode};

    use tt_ports::host_resource::{
        HostResourceAssetStore, HostResourceSourceRequest, HostResourceStoreError,
        OpenedHostResource,
    };

    use super::*;

    #[derive(Default)]
    struct NoopStore {
        opens: AtomicUsize,
    }

    impl HostResourceAssetStore for NoopStore {
        fn open(
            &self,
            _request: HostResourceSourceRequest<'_>,
        ) -> Result<OpenedHostResource, HostResourceStoreError> {
            self.opens.fetch_add(1, Ordering::Relaxed);
            Err(HostResourceStoreError::not_found("missing"))
        }
    }

    #[test]
    fn production_origin_gate_accepts_only_canonical_tauri_origin() {
        for accepted in [
            "tauri://localhost/backgrounds/a.mp4",
            "tauri://localhost/backgrounds/a.mp4?x=1",
        ] {
            assert!(is_tauri_app_uri(&accepted.parse().expect("accepted URI")));
        }
        for rejected in [
            "/backgrounds/a.mp4",
            "https://example.com/backgrounds/a.mp4",
            "tauri://evil.example/backgrounds/a.mp4",
            "tauri://localhost:80/backgrounds/a.mp4",
        ] {
            assert!(!is_tauri_app_uri(&rejected.parse().expect("rejected URI")));
        }
    }

    #[test]
    fn dev_protocol_uses_the_same_wry_delivery_as_production() {
        assert_eq!(
            WRY_DELIVERY,
            HostResourceDeliveryCapabilities::new(
                !cfg!(target_os = "android"),
                cfg!(target_os = "android")
            )
        );
    }

    #[test]
    fn dev_ipc_matches_platform_304_support_without_wry_range_workaround() {
        assert_eq!(
            DEV_IPC_DELIVERY,
            HostResourceDeliveryCapabilities::new(!cfg!(target_os = "android"), false)
        );
    }

    #[test]
    fn unhandled_production_request_leaves_response_unchanged() {
        let host_resources = HostResourceService::new(false, Arc::new(NoopStore::default()));
        let request = Request::builder()
            .method("GET")
            .uri("tauri://localhost/index.html")
            .body(Vec::new())
            .expect("request");
        let mut response: Response<Cow<'static, [u8]>> =
            Response::new(Cow::Owned(b"frontend".to_vec()));
        *response.status_mut() = StatusCode::OK;

        handle_tauri_web_resource_request(&host_resources, &request, &mut response);

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.body().as_ref(), b"frontend");
        assert!(response.headers().get(CONTENT_TYPE).is_none());
    }

    #[test]
    fn external_matching_path_is_not_dispatched() {
        let store = Arc::new(NoopStore::default());
        let host_resources = HostResourceService::new(false, Arc::clone(&store));
        let request = Request::builder()
            .method("GET")
            .uri("https://example.com/backgrounds/a.mp4")
            .body(Vec::new())
            .expect("request");
        let mut response: Response<Cow<'static, [u8]>> =
            Response::new(Cow::Owned(b"external".to_vec()));

        handle_tauri_web_resource_request(&host_resources, &request, &mut response);

        assert_eq!(response.body().as_ref(), b"external");
        assert_eq!(store.opens.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn apply_host_response_applies_status_headers_and_body() {
        let mut host_response = HostResourceResponse::new(b"ab".to_vec());
        *host_response.status_mut() = StatusCode::PARTIAL_CONTENT;
        host_response
            .headers_mut()
            .insert(CONTENT_TYPE, HeaderValue::from_static("video/mp4"));
        host_response
            .headers_mut()
            .insert(CONTENT_LENGTH, HeaderValue::from_static("2"));
        let mut response: Response<Cow<'static, [u8]>> = Response::new(Cow::Owned(Vec::new()));

        apply_host_resource_response(&mut response, host_response);

        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(response.body().as_ref(), b"ab");
        assert_eq!(
            response
                .headers()
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("video/mp4")
        );
        assert_eq!(
            response
                .headers()
                .get(CONTENT_LENGTH)
                .and_then(|value| value.to_str().ok()),
            Some("2")
        );
    }

    #[test]
    fn apply_host_response_removes_stale_entity_headers() {
        let mut host_response = HostResourceResponse::new(b"ab".to_vec());
        host_response
            .headers_mut()
            .insert(CONTENT_TYPE, HeaderValue::from_static("text/plain"));
        host_response
            .headers_mut()
            .insert(ETAG, HeaderValue::from_static("W/\"new\""));
        let mut response: Response<Cow<'static, [u8]>> = Response::new(Cow::Owned(b"old".to_vec()));
        response
            .headers_mut()
            .insert(CONTENT_LENGTH, HeaderValue::from_static("999"));
        response
            .headers_mut()
            .insert(CONTENT_ENCODING, HeaderValue::from_static("br"));
        response
            .headers_mut()
            .insert(VARY, HeaderValue::from_static("accept-encoding"));
        response
            .headers_mut()
            .insert(ETAG, HeaderValue::from_static("W/\"old\""));
        response.headers_mut().insert(
            LAST_MODIFIED,
            HeaderValue::from_static("Tue, 15 Nov 1994 12:45:26 GMT"),
        );
        response.headers_mut().insert(
            CONTENT_SECURITY_POLICY,
            HeaderValue::from_static("default-src 'self'"),
        );

        apply_host_resource_response(&mut response, host_response);

        assert_eq!(response.body().as_ref(), b"ab");
        assert!(response.headers().get(CONTENT_LENGTH).is_none());
        assert!(response.headers().get(CONTENT_ENCODING).is_none());
        assert!(response.headers().get(VARY).is_none());
        assert_eq!(response.headers()[ETAG], "W/\"new\"");
        assert!(!response.headers().contains_key(LAST_MODIFIED));
        assert_eq!(
            response.headers()[CONTENT_SECURITY_POLICY],
            "default-src 'self'"
        );
        assert_eq!(
            response
                .headers()
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("text/plain")
        );
    }
}
