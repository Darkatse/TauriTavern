use std::borrow::Cow;

use tauri::http::header::{
    ACCESS_CONTROL_ALLOW_HEADERS, ACCESS_CONTROL_ALLOW_METHODS, ACCESS_CONTROL_ALLOW_ORIGIN,
    ACCESS_CONTROL_EXPOSE_HEADERS, CACHE_CONTROL, CONTENT_TYPE, HeaderValue,
};

use crate::presentation::web_resources::tauri_resource_adapter::{
    apply_host_resource_response, serve_dev_protocol_resource_from_app,
};

const DEV_ALLOWED_METHODS: &str = "GET, HEAD, OPTIONS";

fn dev_protocol_response(body: Cow<'static, [u8]>) -> tauri::http::Response<Cow<'static, [u8]>> {
    let mut response = tauri::http::Response::new(body);
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
    response
        .headers_mut()
        .insert(ACCESS_CONTROL_EXPOSE_HEADERS, HeaderValue::from_static("*"));
    response
}

#[cfg(any(dev, debug_assertions))]
pub fn handle_dev_protocol_request<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    request: tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<Cow<'static, [u8]>> {
    let mut response = dev_protocol_response(Cow::Owned(Vec::new()));
    let host_response = serve_dev_protocol_resource_from_app(app_handle, &request);
    apply_host_resource_response(&mut response, host_response);
    response
}

#[cfg(any(dev, debug_assertions))]
pub fn dev_protocol_task_error_response() -> tauri::http::Response<Cow<'static, [u8]>> {
    let mut response = dev_protocol_response(Cow::Borrowed(b"Internal Server Error"));
    *response.status_mut() = tauri::http::StatusCode::INTERNAL_SERVER_ERROR;
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    response
}
