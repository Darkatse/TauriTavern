use std::borrow::Cow;

use tauri::http::StatusCode;

use crate::domain::errors::DomainError;
use crate::infrastructure::persistence::thumbnail_cache::read_thumbnail_or_original_sync;
use crate::infrastructure::thumbnails::{avatar_thumbnail_config, background_thumbnail_config};
use crate::infrastructure::user_data_dirs::DefaultUserWebDirs;
use crate::presentation::web_resources::response_helpers::{
    respond_bytes, respond_method_not_allowed, respond_no_content, respond_plain_text,
};

const THUMBNAIL_ROUTE_PATH: &str = "/thumbnail";
const THUMBNAIL_ALLOWED_METHODS: &str = "GET, HEAD, OPTIONS";

pub fn handle_thumbnail_web_request(
    user_dirs: &DefaultUserWebDirs,
    request: &tauri::http::Request<Vec<u8>>,
    response: &mut tauri::http::Response<Cow<'static, [u8]>>,
) {
    if request.uri().path() != THUMBNAIL_ROUTE_PATH {
        return;
    }

    handle_thumbnail_route_request(user_dirs, request, response);
}

fn handle_thumbnail_route_request(
    user_dirs: &DefaultUserWebDirs,
    request: &tauri::http::Request<Vec<u8>>,
    response: &mut tauri::http::Response<Cow<'static, [u8]>>,
) {
    use tauri::http::Method;

    match request.method() {
        &Method::OPTIONS => {
            respond_no_content(response, THUMBNAIL_ALLOWED_METHODS);
            return;
        }
        &Method::GET | &Method::HEAD => {}
        _ => {
            respond_method_not_allowed(response, THUMBNAIL_ALLOWED_METHODS);
            return;
        }
    }

    let query = request.uri().query().unwrap_or("");
    let (thumbnail_type, file) = match parse_thumbnail_query(query) {
        Ok(value) => value,
        Err(error) => {
            respond_plain_text(response, error.status_code(), error.message());
            return;
        }
    };

    let (original_dir, thumbnail_dir, config) = match thumbnail_type.as_str() {
        "avatar" => (
            user_dirs.characters_dir.as_path(),
            user_dirs.thumbnails_avatar_dir.as_path(),
            avatar_thumbnail_config(),
        ),
        "persona" => (
            user_dirs.avatars_dir.as_path(),
            user_dirs.thumbnails_persona_dir.as_path(),
            avatar_thumbnail_config(),
        ),
        "bg" => (
            user_dirs.backgrounds_dir.as_path(),
            user_dirs.thumbnails_bg_dir.as_path(),
            background_thumbnail_config(),
        ),
        _ => {
            respond_plain_text(response, StatusCode::BAD_REQUEST, "Invalid thumbnail type");
            return;
        }
    };

    let original_path = original_dir.join(&file);
    let thumbnail_path = thumbnail_dir.join(&file);

    let asset = match read_thumbnail_or_original_sync(&original_path, &thumbnail_path, config) {
        Ok(value) => value,
        Err(DomainError::NotFound(_)) => {
            respond_plain_text(response, StatusCode::NOT_FOUND, "Not Found");
            tracing::debug!("Thumbnail 404: type={} file={}", thumbnail_type, file);
            return;
        }
        Err(error) => {
            respond_plain_text(
                response,
                StatusCode::INTERNAL_SERVER_ERROR,
                &error.to_string(),
            );
            return;
        }
    };

    if request.method() == Method::HEAD {
        respond_bytes(response, StatusCode::OK, Vec::new(), &asset.mime_type);
        return;
    }

    respond_bytes(response, StatusCode::OK, asset.bytes, &asset.mime_type);
    tracing::debug!("Thumbnail hit: type={} file={}", thumbnail_type, file);
}

fn decode_query_component(value: &str) -> Result<String, ()> {
    let normalized = value.replace('+', " ");
    percent_encoding::percent_decode_str(&normalized)
        .decode_utf8()
        .map(|value| value.into_owned())
        .map_err(|_| ())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ThumbnailQueryError {
    InvalidQuery,
    MissingType,
    MissingFile,
    ForbiddenFile,
}

impl ThumbnailQueryError {
    fn status_code(self) -> StatusCode {
        match self {
            Self::ForbiddenFile => StatusCode::FORBIDDEN,
            _ => StatusCode::BAD_REQUEST,
        }
    }

    fn message(self) -> &'static str {
        match self {
            Self::InvalidQuery => "Invalid thumbnail query",
            Self::MissingType => "Missing thumbnail type",
            Self::MissingFile => "Missing thumbnail file",
            Self::ForbiddenFile => "Forbidden thumbnail file",
        }
    }
}

fn parse_thumbnail_query(query: &str) -> Result<(String, String), ThumbnailQueryError> {
    let mut thumbnail_type = None;
    let mut file = None;

    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }

        let (raw_key, raw_value) = match pair.split_once('=') {
            Some((key, value)) => (key, value),
            None => (pair, ""),
        };

        let key = decode_query_component(raw_key).map_err(|_| ThumbnailQueryError::InvalidQuery)?;
        let value =
            decode_query_component(raw_value).map_err(|_| ThumbnailQueryError::InvalidQuery)?;

        match key.as_str() {
            "type" => thumbnail_type = Some(value),
            "file" => file = Some(value),
            _ => {}
        }
    }

    let thumbnail_type = thumbnail_type.ok_or(ThumbnailQueryError::MissingType)?;
    let file = file.ok_or(ThumbnailQueryError::MissingFile)?;

    let normalized_type = thumbnail_type.trim().to_ascii_lowercase();
    let normalized_file = file.trim().to_string();

    if normalized_type.is_empty() {
        return Err(ThumbnailQueryError::MissingType);
    }

    if normalized_file.is_empty() {
        return Err(ThumbnailQueryError::MissingFile);
    }

    if !crate::infrastructure::request_path::validate_path_segment(&normalized_file) {
        return Err(ThumbnailQueryError::ForbiddenFile);
    }

    Ok((normalized_type, normalized_file))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tauri::http::header::CONTENT_TYPE;

    fn dirs(root: &PathBuf) -> DefaultUserWebDirs {
        DefaultUserWebDirs {
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

    struct TempDirGuard {
        path: PathBuf,
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
    fn rejects_methods_outside_endpoint_contract() {
        let temp = TempDirGuard::new("thumbnail-endpoint-method-gate");
        let request = tauri::http::Request::builder()
            .method("POST")
            .uri("/thumbnail?type=avatar&file=a.png")
            .body(Vec::new())
            .expect("request");
        let mut response = tauri::http::Response::new(Cow::Owned(Vec::new()));

        handle_thumbnail_web_request(&dirs(&temp.path), &request, &mut response);

        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    }

    #[test]
    fn returns_404_for_missing_thumbnail_source() {
        let temp = TempDirGuard::new("thumbnail-endpoint-404");
        std::fs::create_dir_all(temp.path.join("characters")).expect("create characters dir");

        let request = tauri::http::Request::builder()
            .method("GET")
            .uri("/thumbnail?type=avatar&file=missing.png")
            .body(Vec::new())
            .expect("request");
        let mut response = tauri::http::Response::new(Cow::Owned(Vec::new()));

        handle_thumbnail_web_request(&dirs(&temp.path), &request, &mut response);

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn falls_back_to_original_when_thumbnail_missing() {
        let temp = TempDirGuard::new("thumbnail-endpoint-fallback-original");
        std::fs::create_dir_all(temp.path.join("characters")).expect("create characters dir");
        std::fs::write(temp.path.join("characters").join("a.png"), b"original")
            .expect("write original");

        let request = tauri::http::Request::builder()
            .method("GET")
            .uri("/thumbnail?type=avatar&file=a.png")
            .body(Vec::new())
            .expect("request");
        let mut response = tauri::http::Response::new(Cow::Owned(Vec::new()));

        handle_thumbnail_web_request(&dirs(&temp.path), &request, &mut response);

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(CONTENT_TYPE),
            Some(&tauri::http::header::HeaderValue::from_static("image/png"))
        );
        assert_eq!(response.body().as_ref(), b"original");
    }

    #[test]
    fn serves_cached_thumbnail_when_available() {
        let temp = TempDirGuard::new("thumbnail-endpoint-cached");
        std::fs::create_dir_all(temp.path.join("characters")).expect("create characters dir");
        std::fs::create_dir_all(temp.path.join("thumbnails").join("avatar"))
            .expect("create thumbnail dir");
        std::fs::write(temp.path.join("characters").join("a.png"), b"original")
            .expect("write original");
        std::fs::write(
            temp.path.join("thumbnails").join("avatar").join("a.png"),
            b"thumb",
        )
        .expect("write thumbnail");

        let request = tauri::http::Request::builder()
            .method("GET")
            .uri("/thumbnail?type=avatar&file=a.png")
            .body(Vec::new())
            .expect("request");
        let mut response = tauri::http::Response::new(Cow::Owned(Vec::new()));

        handle_thumbnail_web_request(&dirs(&temp.path), &request, &mut response);

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(CONTENT_TYPE),
            Some(&tauri::http::header::HeaderValue::from_static("image/jpeg"))
        );
        assert_eq!(response.body().as_ref(), b"thumb");
    }

    #[test]
    fn serves_background_thumbnails() {
        let temp = TempDirGuard::new("thumbnail-endpoint-bg");
        std::fs::create_dir_all(temp.path.join("backgrounds")).expect("create backgrounds dir");
        std::fs::write(temp.path.join("backgrounds").join("a.png"), b"original")
            .expect("write original");

        let request = tauri::http::Request::builder()
            .method("GET")
            .uri("/thumbnail?type=bg&file=a.png")
            .body(Vec::new())
            .expect("request");
        let mut response = tauri::http::Response::new(Cow::Owned(Vec::new()));

        handle_thumbnail_web_request(&dirs(&temp.path), &request, &mut response);

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.body().as_ref(), b"original");
    }
}
