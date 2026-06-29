use std::borrow::Cow;
#[cfg(any(dev, debug_assertions, test))]
use std::sync::Arc;

#[cfg(any(dev, debug_assertions))]
use tauri::Manager;
use tauri::http::header::{HeaderName, HeaderValue};

use crate::application::services::host_resource_service::HostResourceService;
use crate::application::services::host_resource_service::contract::{
    HostResourceHeader, HostResourceHeaders, HostResourceMethod, HostResourceRequest,
    HostResourceResponse,
};

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
    let headers = host_headers_from_tauri_request(request);
    let host_request = host_request_from_tauri(request, &headers);
    host_resources.try_serve(&host_request)
}

#[cfg(any(dev, debug_assertions))]
pub(crate) fn serve_dev_web_resource_from_app<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    request: &tauri::http::Request<Vec<u8>>,
) -> HostResourceResponse {
    let host_resources = app_handle.state::<Arc<HostResourceService>>();
    let headers = host_headers_from_tauri_request(request);
    let host_request = host_request_from_tauri(request, &headers);
    host_resources.serve_dev_resource(&host_request)
}

fn host_headers_from_tauri_request<'a>(
    request: &'a tauri::http::Request<Vec<u8>>,
) -> Vec<HostResourceHeader<'a>> {
    request
        .headers()
        .iter()
        .map(|(name, value)| HostResourceHeader {
            name: name.as_str(),
            value: value.as_bytes(),
        })
        .collect()
}

fn host_request_from_tauri<'a>(
    request: &'a tauri::http::Request<Vec<u8>>,
    headers: &'a [HostResourceHeader<'a>],
) -> HostResourceRequest<'a> {
    HostResourceRequest::new(
        HostResourceMethod::from_str(request.method().as_str()),
        request.uri().path(),
        request.uri().query(),
        HostResourceHeaders::new(headers),
    )
}

pub(crate) fn apply_host_resource_response(
    response: &mut tauri::http::Response<Cow<'static, [u8]>>,
    host_response: HostResourceResponse,
) {
    *response.status_mut() =
        tauri::http::StatusCode::from_u16(host_response.status).expect("Invalid status code");

    for (name, value) in host_response.headers {
        response.headers_mut().insert(
            HeaderName::from_bytes(name.as_bytes()).expect("Invalid header name"),
            HeaderValue::from_str(&value).expect("Invalid header value"),
        );
    }

    *response.body_mut() = Cow::Owned(host_response.body);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    use tauri::http::header::CONTENT_LENGTH;
    use tauri::http::{Request, Response, StatusCode};

    use crate::application::services::host_resource_service::contract::{header, status};
    use crate::application::services::host_resource_service::policy::HostResourceRuntimePolicy;
    use crate::application::services::host_resource_service::ports::{
        HostResourceAssetStore, HostResourceBinaryAsset, HostResourceFileStat,
        HostResourceStoreError, ThumbnailAssetRequest,
    };
    use crate::application::services::host_resource_service::range::ByteRange;
    use crate::application::services::host_resource_service::routes::UserDataAssetKind;

    struct NoopStore;

    impl HostResourceAssetStore for NoopStore {
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

    #[test]
    fn host_request_from_tauri_keeps_range_header() {
        let request = Request::builder()
            .method("GET")
            .uri("/backgrounds/a.mp4?x=1")
            .header(header::RANGE, "bytes=1-2")
            .body(Vec::new())
            .expect("request");
        let headers = host_headers_from_tauri_request(&request);
        let host_request = host_request_from_tauri(&request, &headers);

        assert_eq!(host_request.path, "/backgrounds/a.mp4");
        assert_eq!(host_request.query, Some("x=1"));
        assert_eq!(
            host_request.headers.get(header::RANGE),
            Some(&b"bytes=1-2"[..])
        );
    }

    #[test]
    fn unhandled_production_request_leaves_response_unchanged() {
        let host_resources = HostResourceService::new(
            Arc::new(HostResourceRuntimePolicy::new(false)),
            Arc::new(NoopStore),
        );
        let request = Request::builder()
            .method("GET")
            .uri("/index.html")
            .body(Vec::new())
            .expect("request");
        let mut response: Response<Cow<'static, [u8]>> =
            Response::new(Cow::Owned(b"frontend".to_vec()));
        *response.status_mut() = StatusCode::OK;

        handle_tauri_web_resource_request(&host_resources, &request, &mut response);

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.body().as_ref(), b"frontend");
        assert!(response.headers().get(header::CONTENT_TYPE).is_none());
    }

    #[test]
    fn apply_host_response_applies_status_headers_and_body() {
        let host_response =
            HostResourceResponse::bytes(status::PARTIAL_CONTENT, b"ab".to_vec(), "video/mp4")
                .with_header(header::CONTENT_LENGTH, "2");
        let mut response: Response<Cow<'static, [u8]>> = Response::new(Cow::Owned(Vec::new()));

        apply_host_resource_response(&mut response, host_response);

        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(response.body().as_ref(), b"ab");
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
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
}
