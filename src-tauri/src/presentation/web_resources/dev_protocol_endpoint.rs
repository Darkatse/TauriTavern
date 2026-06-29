use std::borrow::Cow;

use tauri::http::header::{
    ACCESS_CONTROL_ALLOW_HEADERS, ACCESS_CONTROL_ALLOW_METHODS, ACCESS_CONTROL_ALLOW_ORIGIN,
    HeaderValue,
};

use crate::presentation::web_resources::tauri_resource_adapter::{
    apply_host_resource_response, serve_dev_web_resource_from_app,
};

const DEV_ALLOWED_METHODS: &str = "GET, HEAD, OPTIONS";

#[cfg(any(dev, debug_assertions))]
pub fn handle_dev_protocol_request<R: tauri::Runtime>(
    ctx: tauri::UriSchemeContext<'_, R>,
    request: tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<Cow<'static, [u8]>> {
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

    let host_response = serve_dev_web_resource_from_app(&ctx.app_handle(), &request);
    apply_host_resource_response(&mut response, host_response);
    response
}
