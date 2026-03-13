use std::borrow::Cow;

use tauri::http::StatusCode;
use tauri::http::header::{ALLOW, CACHE_CONTROL, CONTENT_TYPE, HeaderValue};

pub(crate) fn respond_no_content(
    response: &mut tauri::http::Response<Cow<'static, [u8]>>,
    allowed_methods: &'static str,
) {
    *response.status_mut() = StatusCode::NO_CONTENT;
    set_allowed_methods_header(response, allowed_methods);
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    *response.body_mut() = Cow::Owned(Vec::new());
}

pub(crate) fn respond_plain_text(
    response: &mut tauri::http::Response<Cow<'static, [u8]>>,
    status: StatusCode,
    message: &str,
) {
    *response.status_mut() = status;
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    *response.body_mut() = Cow::Owned(message.as_bytes().to_vec());
}

pub(crate) fn respond_method_not_allowed(
    response: &mut tauri::http::Response<Cow<'static, [u8]>>,
    allowed_methods: &'static str,
) {
    respond_plain_text(response, StatusCode::METHOD_NOT_ALLOWED, "Method not allowed");
    set_allowed_methods_header(response, allowed_methods);
}

pub(crate) fn set_allowed_methods_header(
    response: &mut tauri::http::Response<Cow<'static, [u8]>>,
    allowed_methods: &'static str,
) {
    response
        .headers_mut()
        .insert(ALLOW, HeaderValue::from_static(allowed_methods));
}

pub(crate) fn respond_bytes(
    response: &mut tauri::http::Response<Cow<'static, [u8]>>,
    status: StatusCode,
    bytes: Vec<u8>,
    content_type: &str,
) {
    *response.status_mut() = status;
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_str(content_type).expect("Invalid Content-Type"),
    );
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    *response.body_mut() = Cow::Owned(bytes);
}

