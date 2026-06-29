use std::borrow::Cow;
use std::sync::Arc;

use tauri::Manager;
use tauri::http::header::{HeaderName, HeaderValue};

use crate::application::services::host_resource_service::contract::{
    HostResourceHeader, HostResourceHeaders, HostResourceMethod, HostResourceRequest,
    HostResourceResponse, status,
};
use crate::application::services::host_resource_service::policy::HostResourceRuntimePolicy;
use crate::application::services::host_resource_service::roots::HostResourceRoots;
use crate::application::services::host_resource_service::route_classifier::{
    HostResourceRoute, classify_host_resource_route,
};
use crate::infrastructure::data_root_content_dirs::DataRootContentDirs;
use crate::infrastructure::third_party_assets::ThirdPartyExtensionDirs;
use crate::infrastructure::user_data_dirs::DefaultUserWebDirs;
use crate::presentation::web_resources::third_party_endpoint::serve_third_party_asset;
use crate::presentation::web_resources::thumbnail_endpoint::serve_thumbnail;
use crate::presentation::web_resources::user_css_endpoint::serve_user_css;
use crate::presentation::web_resources::user_data_endpoint::serve_user_data_asset;

pub(crate) fn build_host_resource_roots(
    third_party_dirs: &ThirdPartyExtensionDirs,
    user_dirs: &DefaultUserWebDirs,
    data_root_content_dirs: &DataRootContentDirs,
) -> HostResourceRoots {
    HostResourceRoots {
        user_css_file: data_root_content_dirs.user_css_file.clone(),
        local_extensions_dir: third_party_dirs.local_dir.clone(),
        global_extensions_dir: third_party_dirs.global_dir.clone(),
        characters_dir: user_dirs.characters_dir.clone(),
        avatars_dir: user_dirs.avatars_dir.clone(),
        backgrounds_dir: user_dirs.backgrounds_dir.clone(),
        assets_dir: user_dirs.assets_dir.clone(),
        user_images_dir: user_dirs.user_images_dir.clone(),
        user_files_dir: user_dirs.user_files_dir.clone(),
        thumbnails_bg_dir: user_dirs.thumbnails_bg_dir.clone(),
        thumbnails_avatar_dir: user_dirs.thumbnails_avatar_dir.clone(),
        thumbnails_persona_dir: user_dirs.thumbnails_persona_dir.clone(),
    }
}

pub(crate) fn dispatch_host_resource_request(
    roots: &HostResourceRoots,
    policy: &HostResourceRuntimePolicy,
    request: &HostResourceRequest<'_>,
) -> Option<HostResourceResponse> {
    match classify_host_resource_route(request)? {
        HostResourceRoute::UserCss => Some(serve_user_css(&roots.user_css_file, request)),
        HostResourceRoute::ThirdPartyAsset => Some(serve_third_party_asset(
            &roots.local_extensions_dir,
            &roots.global_extensions_dir,
            request,
        )),
        HostResourceRoute::Thumbnail => Some(serve_thumbnail(roots, policy, request)),
        HostResourceRoute::UserDataAsset => Some(serve_user_data_asset(roots, request)),
    }
}

pub(crate) fn handle_tauri_web_resource_request(
    roots: &HostResourceRoots,
    policy: &HostResourceRuntimePolicy,
    request: &tauri::http::Request<Vec<u8>>,
    response: &mut tauri::http::Response<Cow<'static, [u8]>>,
) {
    if let Some(host_response) = dispatch_tauri_host_resource_request(roots, policy, request) {
        apply_host_resource_response(response, host_response);
    }
}

pub(crate) fn dispatch_tauri_host_resource_request(
    roots: &HostResourceRoots,
    policy: &HostResourceRuntimePolicy,
    request: &tauri::http::Request<Vec<u8>>,
) -> Option<HostResourceResponse> {
    let headers = host_headers_from_tauri_request(request);
    let host_request = host_request_from_tauri(request, &headers);
    dispatch_host_resource_request(roots, policy, &host_request)
}

#[cfg(any(dev, debug_assertions))]
pub(crate) fn serve_dev_web_resource_from_app<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    request: &tauri::http::Request<Vec<u8>>,
) -> HostResourceResponse {
    let roots = host_resource_roots_from_app(app_handle);
    let policy = app_handle.state::<Arc<HostResourceRuntimePolicy>>();

    dispatch_tauri_host_resource_request(&roots, policy.inner().as_ref(), request)
        .unwrap_or_else(|| HostResourceResponse::plain_text(status::NOT_FOUND, "Not Found"))
}

#[cfg(any(dev, debug_assertions))]
fn host_resource_roots_from_app<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
) -> HostResourceRoots {
    let third_party_dirs = app_handle.state::<ThirdPartyExtensionDirs>();
    let user_dirs = app_handle.state::<DefaultUserWebDirs>();
    let data_root_content_dirs = app_handle.state::<DataRootContentDirs>();

    build_host_resource_roots(
        third_party_dirs.inner(),
        user_dirs.inner(),
        data_root_content_dirs.inner(),
    )
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
    use std::path::PathBuf;

    use tauri::http::header::CONTENT_LENGTH;
    use tauri::http::{Request, Response, StatusCode};

    use crate::application::services::host_resource_service::contract::header;

    fn roots(root: PathBuf) -> HostResourceRoots {
        HostResourceRoots {
            user_css_file: root.join("user.css"),
            local_extensions_dir: root.join("local-extensions"),
            global_extensions_dir: root.join("global-extensions"),
            characters_dir: root.join("characters"),
            avatars_dir: root.join("avatars"),
            backgrounds_dir: root.join("backgrounds"),
            assets_dir: root.join("assets"),
            user_images_dir: root.join("user/images"),
            user_files_dir: root.join("user/files"),
            thumbnails_bg_dir: root.join("thumbnails/bg"),
            thumbnails_avatar_dir: root.join("thumbnails/avatar"),
            thumbnails_persona_dir: root.join("thumbnails/persona"),
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
        let roots = roots(PathBuf::from("unused"));
        let policy = HostResourceRuntimePolicy::new(false);
        let request = Request::builder()
            .method("GET")
            .uri("/index.html")
            .body(Vec::new())
            .expect("request");
        let mut response: Response<Cow<'static, [u8]>> =
            Response::new(Cow::Owned(b"frontend".to_vec()));
        *response.status_mut() = StatusCode::OK;

        handle_tauri_web_resource_request(&roots, &policy, &request, &mut response);

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
