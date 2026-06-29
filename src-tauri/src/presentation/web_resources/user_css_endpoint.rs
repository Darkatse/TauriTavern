use std::path::Path;

use crate::application::services::host_resource_service::contract::{
    HostResourceMethod, HostResourceRequest, HostResourceResponse, header, status,
};
const USER_CSS_ALLOWED_METHODS: &str = "GET, HEAD, OPTIONS";
const USER_CSS_CONTENT_TYPE: &str = "text/css; charset=utf-8";

pub(crate) fn serve_user_css(
    user_css_file: &Path,
    request: &HostResourceRequest<'_>,
) -> HostResourceResponse {
    match request.method {
        HostResourceMethod::Options => {
            return HostResourceResponse::no_content(USER_CSS_ALLOWED_METHODS);
        }
        HostResourceMethod::Get | HostResourceMethod::Head => {}
        _ => return HostResourceResponse::method_not_allowed(USER_CSS_ALLOWED_METHODS),
    }

    let bytes = match std::fs::read(user_css_file) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return HostResourceResponse::plain_text(status::NOT_FOUND, "User CSS not found");
        }
        Err(error) => {
            return HostResourceResponse::plain_text(
                status::INTERNAL_SERVER_ERROR,
                &format!("Failed to read user CSS: {}", error),
            );
        }
    };

    let content_length = bytes.len();
    let body = if request.method == HostResourceMethod::Head {
        Vec::new()
    } else {
        bytes
    };

    HostResourceResponse::bytes(status::OK, body, USER_CSS_CONTENT_TYPE)
        .with_header(header::CONTENT_LENGTH, content_length.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::services::host_resource_service::contract::{
        HostResourceHeaders, HostResourceMethod, HostResourceRequest,
    };

    fn request(method: HostResourceMethod) -> HostResourceRequest<'static> {
        HostResourceRequest::new(method, "/css/user.css", None, HostResourceHeaders::empty())
    }

    fn header<'a>(response: &'a HostResourceResponse, name: &str) -> Option<&'a str> {
        response
            .headers
            .iter()
            .find(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

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

        let response = serve_user_css(&css_file, &request(HostResourceMethod::Get));

        assert_eq!(response.status, status::OK);
        assert_eq!(response.body, b"body { color: red; }");
    }

    #[test]
    fn returns_not_found_when_data_root_user_css_is_missing() {
        let temp = TempDirGuard::new("user-css-endpoint-default");
        let css_file = temp.path.join("_css").join("user.css");

        let response = serve_user_css(&css_file, &request(HostResourceMethod::Get));

        assert_eq!(response.status, status::NOT_FOUND);
    }

    #[test]
    fn head_returns_no_body_with_css_length() {
        let temp = TempDirGuard::new("user-css-endpoint-head");
        let css_file = temp.path.join("_css").join("user.css");
        std::fs::create_dir_all(css_file.parent().expect("css parent")).expect("create css dir");
        std::fs::write(&css_file, b"body {}").expect("write css");

        let response = serve_user_css(&css_file, &request(HostResourceMethod::Head));

        assert_eq!(response.status, status::OK);
        assert!(response.body.is_empty());
        assert_eq!(header(&response, header::CONTENT_LENGTH), Some("7"));
    }

    #[test]
    fn rejects_non_read_methods() {
        let temp = TempDirGuard::new("user-css-endpoint-method");
        let css_file = temp.path.join("_css").join("user.css");

        let response = serve_user_css(&css_file, &request(HostResourceMethod::Other));

        assert_eq!(response.status, status::METHOD_NOT_ALLOWED);
    }
}
