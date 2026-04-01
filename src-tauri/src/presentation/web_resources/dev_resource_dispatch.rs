use std::borrow::Cow;

use tauri::http::StatusCode;

use crate::infrastructure::third_party_assets::ThirdPartyExtensionDirs;
use crate::infrastructure::third_party_paths::THIRD_PARTY_EXTENSION_ROUTE_PREFIX;
use crate::infrastructure::user_data_dirs::DefaultUserWebDirs;
use crate::infrastructure::user_data_paths::is_user_data_asset_route;
use crate::presentation::web_resources::response_helpers::respond_plain_text;
use crate::presentation::web_resources::third_party_endpoint::handle_third_party_asset_web_request;
use crate::presentation::web_resources::thumbnail_endpoint::{
    handle_thumbnail_web_request, ThumbnailEndpointPolicy,
};
use crate::presentation::web_resources::user_data_endpoint::handle_user_data_asset_web_request;

pub fn dispatch_dev_web_resource_request<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    request: &tauri::http::Request<Vec<u8>>,
    response: &mut tauri::http::Response<Cow<'static, [u8]>>,
) {
    use tauri::Manager;

    let path = request.uri().path();
    if path.starts_with(THIRD_PARTY_EXTENSION_ROUTE_PREFIX) {
        let dirs = app_handle.state::<ThirdPartyExtensionDirs>();
        handle_third_party_asset_web_request(&dirs.local_dir, &dirs.global_dir, request, response);
        return;
    }

    if path == "/thumbnail" {
        let dirs = app_handle.state::<DefaultUserWebDirs>();
        let policy = app_handle.state::<std::sync::Arc<ThumbnailEndpointPolicy>>();
        handle_thumbnail_web_request(&dirs, policy.inner().as_ref(), request, response);
        return;
    }

    if is_user_data_asset_route(path) {
        let dirs = app_handle.state::<DefaultUserWebDirs>();
        handle_user_data_asset_web_request(&dirs, request, response);
        return;
    }

    respond_plain_text(response, StatusCode::NOT_FOUND, "Not Found");
}
