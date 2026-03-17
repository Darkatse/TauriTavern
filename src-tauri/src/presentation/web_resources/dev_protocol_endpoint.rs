use std::borrow::Cow;

use tauri::http::header::{
    ACCESS_CONTROL_ALLOW_HEADERS, ACCESS_CONTROL_ALLOW_METHODS, ACCESS_CONTROL_ALLOW_ORIGIN,
    HeaderValue,
};

use crate::presentation::web_resources::dev_resource_dispatch::dispatch_dev_web_resource_request;

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

    dispatch_dev_web_resource_request(&ctx.app_handle(), &request, &mut response);
    response
}
