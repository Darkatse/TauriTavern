use std::borrow::Cow;
use std::path::Path;

use tauri::http::header::{CONTENT_LENGTH, HeaderValue};
use tauri::http::{Method, StatusCode};

use crate::presentation::web_resources::response_helpers::{
    respond_bytes, respond_method_not_allowed, respond_no_content, respond_plain_text,
};

const USER_CSS_ROUTE: &str = "/css/user.css";
const USER_CSS_ALLOWED_METHODS: &str = "GET, HEAD, OPTIONS";
const USER_CSS_CONTENT_TYPE: &str = "text/css; charset=utf-8";

pub fn is_user_css_route(path: &str) -> bool {
    path == USER_CSS_ROUTE
}

pub fn handle_user_css_web_request(
    user_css_file: &Path,
    request: &tauri::http::Request<Vec<u8>>,
    response: &mut tauri::http::Response<Cow<'static, [u8]>>,
) {
    if !is_user_css_route(request.uri().path()) {
        return;
    }

    match request.method() {
        &Method::OPTIONS => {
            respond_no_content(response, USER_CSS_ALLOWED_METHODS);
            return;
        }
        &Method::GET | &Method::HEAD => {}
        _ => {
            respond_method_not_allowed(response, USER_CSS_ALLOWED_METHODS);
            return;
        }
    }

    let bytes = match std::fs::read(user_css_file) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            respond_plain_text(response, StatusCode::NOT_FOUND, "User CSS not found");
            return;
        }
        Err(error) => {
            respond_plain_text(
                response,
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("Failed to read user CSS: {}", error),
            );
            return;
        }
    };

    let content_length = bytes.len();
    let body = if request.method() == Method::HEAD {
        Vec::new()
    } else {
        bytes
    };

    respond_bytes(response, StatusCode::OK, body, USER_CSS_CONTENT_TYPE);
    response.headers_mut().insert(
        CONTENT_LENGTH,
        HeaderValue::from_str(&content_length.to_string()).expect("Invalid Content-Length"),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDirGuard {
        path: std::path::PathBuf,
    }

    impl TempDirGuard {
        fn new(test_name: &str) -> Self {
            let mut path = std::env::temp_dir();
            path.push(format!("tauritavern-{test_name}-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&path).expect("create temp dir");
            Self { path }
        }
    }

    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn serves_data_root_user_css_when_present() {
        let temp = TempDirGuard::new("user-css-endpoint-custom");
        let css_file = temp.path.join("_css").join("user.css");
        std::fs::create_dir_all(css_file.parent().expect("css parent")).expect("create css dir");
        std::fs::write(&css_file, b"body { color: red; }").expect("write css");

        let request = tauri::http::Request::builder()
            .method("GET")
            .uri("/css/user.css")
            .body(Vec::new())
            .expect("request");
        let mut response = tauri::http::Response::new(Cow::Owned(Vec::new()));

        handle_user_css_web_request(&css_file, &request, &mut response);

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.body().as_ref(), b"body { color: red; }");
    }

    #[test]
    fn returns_not_found_when_data_root_user_css_is_missing() {
        let temp = TempDirGuard::new("user-css-endpoint-default");
        let css_file = temp.path.join("_css").join("user.css");

        let request = tauri::http::Request::builder()
            .method("GET")
            .uri("/css/user.css")
            .body(Vec::new())
            .expect("request");
        let mut response = tauri::http::Response::new(Cow::Owned(Vec::new()));

        handle_user_css_web_request(&css_file, &request, &mut response);

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn head_returns_no_body_with_css_length() {
        let temp = TempDirGuard::new("user-css-endpoint-head");
        let css_file = temp.path.join("_css").join("user.css");
        std::fs::create_dir_all(css_file.parent().expect("css parent")).expect("create css dir");
        std::fs::write(&css_file, b"body {}").expect("write css");

        let request = tauri::http::Request::builder()
            .method("HEAD")
            .uri("/css/user.css")
            .body(Vec::new())
            .expect("request");
        let mut response = tauri::http::Response::new(Cow::Owned(Vec::new()));

        handle_user_css_web_request(&css_file, &request, &mut response);

        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.body().is_empty());
        assert_eq!(
            response
                .headers()
                .get(CONTENT_LENGTH)
                .map(|value| value.to_str().unwrap_or("")),
            Some("7")
        );
    }

    #[test]
    fn rejects_non_read_methods() {
        let temp = TempDirGuard::new("user-css-endpoint-method");
        let css_file = temp.path.join("_css").join("user.css");

        let request = tauri::http::Request::builder()
            .method("POST")
            .uri("/css/user.css")
            .body(Vec::new())
            .expect("request");
        let mut response = tauri::http::Response::new(Cow::Owned(Vec::new()));

        handle_user_css_web_request(&css_file, &request, &mut response);

        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    }
}
