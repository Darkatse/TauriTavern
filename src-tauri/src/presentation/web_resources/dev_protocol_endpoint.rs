use std::borrow::Cow;

use tauri::http::StatusCode;
use tauri::http::header::{
    ACCESS_CONTROL_ALLOW_HEADERS, ACCESS_CONTROL_ALLOW_METHODS, ACCESS_CONTROL_ALLOW_ORIGIN,
    HeaderValue,
};

use crate::infrastructure::third_party_assets::ThirdPartyExtensionDirs;
use crate::infrastructure::third_party_paths::THIRD_PARTY_EXTENSION_ROUTE_PREFIX;
use crate::infrastructure::user_data_dirs::DefaultUserWebDirs;
use crate::infrastructure::user_data_paths::is_user_data_asset_route;
use crate::presentation::web_resources::response_helpers::respond_plain_text;
use crate::presentation::web_resources::third_party_endpoint::handle_third_party_asset_web_request;
use crate::presentation::web_resources::thumbnail_endpoint::handle_thumbnail_web_request;
use crate::presentation::web_resources::user_data_endpoint::handle_user_data_asset_web_request;

const DEV_ALLOWED_METHODS: &str = "GET, HEAD, OPTIONS";

#[cfg(any(dev, debug_assertions))]
pub fn handle_dev_protocol_request<R: tauri::Runtime>(
    ctx: tauri::UriSchemeContext<'_, R>,
    request: tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<Cow<'static, [u8]>> {
    use tauri::Manager;

    let mut response = tauri::http::Response::new(Cow::Owned(Vec::new()));
    response
        .headers_mut()
        .insert(ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
    response.headers_mut().insert(
        ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static(DEV_ALLOWED_METHODS),
    );
    response
        .headers_mut()
        .insert(ACCESS_CONTROL_ALLOW_HEADERS, HeaderValue::from_static("*"));

    let path = request.uri().path();
    if path.starts_with(THIRD_PARTY_EXTENSION_ROUTE_PREFIX) {
        let dirs = ctx.app_handle().state::<ThirdPartyExtensionDirs>();
        handle_third_party_asset_web_request(
            &dirs.local_dir,
            &dirs.global_dir,
            &request,
            &mut response,
        );
        return response;
    }

    if path == "/thumbnail" {
        let dirs = ctx.app_handle().state::<DefaultUserWebDirs>();
        handle_thumbnail_web_request(&dirs, &request, &mut response);
        return response;
    }

    if is_user_data_asset_route(path) {
        let dirs = ctx.app_handle().state::<DefaultUserWebDirs>();
        handle_user_data_asset_web_request(&dirs, &request, &mut response);
        return response;
    }

    respond_plain_text(&mut response, StatusCode::NOT_FOUND, "Not Found");
    response
}
