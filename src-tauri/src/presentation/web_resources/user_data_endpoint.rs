use std::borrow::Cow;
use std::io::{Read, Seek};

use tauri::http::StatusCode;
use tauri::http::header::{ACCEPT_RANGES, CONTENT_LENGTH, CONTENT_RANGE, HeaderValue, RANGE};

use crate::infrastructure::user_data_dirs::DefaultUserWebDirs;
use crate::infrastructure::user_data_paths::{
    UserDataAssetKind, UserDataPathError, is_user_data_asset_route,
    parse_user_data_asset_request_path,
};
use crate::presentation::web_resources::byte_range::{RangeHeaderError, parse_single_range_header};
use crate::presentation::web_resources::response_helpers::{
    respond_bytes, respond_method_not_allowed, respond_no_content, respond_plain_text,
};

const USER_DATA_ALLOWED_METHODS: &str = "GET, HEAD, OPTIONS";

#[derive(Clone, Copy)]
struct UserDataAssetRequestPolicy {
    android_webview_reapplies_range_semantics: bool,
}

impl UserDataAssetRequestPolicy {
    const fn for_current_platform() -> Self {
        Self {
            android_webview_reapplies_range_semantics: cfg!(target_os = "android"),
        }
    }
}

pub fn handle_user_data_asset_web_request(
    user_dirs: &DefaultUserWebDirs,
    request: &tauri::http::Request<Vec<u8>>,
    response: &mut tauri::http::Response<Cow<'static, [u8]>>,
) {
    handle_user_data_asset_web_request_with_policy(
        user_dirs,
        request,
        response,
        UserDataAssetRequestPolicy::for_current_platform(),
    );
}

fn handle_user_data_asset_web_request_with_policy(
    user_dirs: &DefaultUserWebDirs,
    request: &tauri::http::Request<Vec<u8>>,
    response: &mut tauri::http::Response<Cow<'static, [u8]>>,
    policy: UserDataAssetRequestPolicy,
) {
    let request_path = request.uri().path();
    if !is_user_data_asset_route(request_path) {
        return;
    }

    handle_user_data_asset_route_request_with_policy(user_dirs, request, response, policy);
}

fn handle_user_data_asset_route_request_with_policy(
    user_dirs: &DefaultUserWebDirs,
    request: &tauri::http::Request<Vec<u8>>,
    response: &mut tauri::http::Response<Cow<'static, [u8]>>,
    policy: UserDataAssetRequestPolicy,
) {
    use tauri::http::Method;

    match request.method() {
        &Method::OPTIONS => {
            respond_no_content(response, USER_DATA_ALLOWED_METHODS);
            return;
        }
        &Method::GET | &Method::HEAD => {}
        _ => {
            respond_method_not_allowed(response, USER_DATA_ALLOWED_METHODS);
            return;
        }
    }

    let request_path = request.uri().path();
    let parsed = match parse_user_data_asset_request_path(request_path) {
        Ok(Some(value)) => value,
        Ok(None) => return,
        Err(UserDataPathError::MissingAssetPath) => {
            respond_plain_text(response, StatusCode::NOT_FOUND, "Not Found");
            return;
        }
        Err(UserDataPathError::InvalidPath) => {
            respond_plain_text(response, StatusCode::BAD_REQUEST, "Invalid asset path");
            return;
        }
    };

    let base_dir = match parsed.kind {
        UserDataAssetKind::Character => user_dirs.characters_dir.as_path(),
        UserDataAssetKind::Persona => user_dirs.avatars_dir.as_path(),
        UserDataAssetKind::Background => user_dirs.backgrounds_dir.as_path(),
        UserDataAssetKind::Asset => user_dirs.assets_dir.as_path(),
        UserDataAssetKind::UserImage => user_dirs.user_images_dir.as_path(),
        UserDataAssetKind::UserFile => user_dirs.user_files_dir.as_path(),
    };
    let asset_path = base_dir.join(&parsed.relative_path);

    let mime_type = mime_guess::from_path(&asset_path)
        .first_or_octet_stream()
        .essence_str()
        .to_string();

    let metadata = match std::fs::metadata(&asset_path) {
        Ok(value) => value,
        Err(error) => {
            let status = match error.kind() {
                std::io::ErrorKind::NotFound => StatusCode::NOT_FOUND,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            respond_plain_text(
                response,
                status,
                &format!("Failed to stat user data asset: {}", error),
            );
            return;
        }
    };

    if !metadata.is_file() {
        respond_plain_text(response, StatusCode::NOT_FOUND, "Not Found");
        return;
    }

    response
        .headers_mut()
        .insert(ACCEPT_RANGES, HeaderValue::from_static("bytes"));

    if request.method() == Method::HEAD {
        respond_bytes(response, StatusCode::OK, Vec::new(), &mime_type);
        response.headers_mut().insert(
            CONTENT_LENGTH,
            HeaderValue::from_str(&metadata.len().to_string()).expect("Invalid Content-Length"),
        );
        return;
    }

    // Android WebView re-applies request range semantics to intercepted responses.
    // If we serve already-sliced bytes, non-zero ranges can become unsatisfiable and yield 416.
    //
    // However, the media pipeline still expects a 206 + Content-Range when it requests a range.
    // Workaround: return correct range headers but provide the full file bytes so WebView can
    // apply the range itself (skip `range.start` bytes in the returned stream).
    //
    // See docs/CurrentState/MediaAssetContract.md.
    let is_android_background_video = policy.android_webview_reapplies_range_semantics
        && parsed.kind == UserDataAssetKind::Background
        && mime_type.starts_with("video/");

    if let Some(range_header) = request.headers().get(RANGE) {
        let header_value = match range_header.to_str() {
            Ok(value) => value,
            Err(_) => {
                respond_plain_text(
                    response,
                    StatusCode::RANGE_NOT_SATISFIABLE,
                    "Invalid Range header",
                );
                response.headers_mut().insert(
                    CONTENT_RANGE,
                    HeaderValue::from_str(&format!("bytes */{}", metadata.len()))
                        .expect("Invalid Content-Range"),
                );
                return;
            }
        };

        let range = match parse_single_range_header(header_value, metadata.len()) {
            Ok(value) => value,
            Err(RangeHeaderError::Invalid) => {
                respond_plain_text(
                    response,
                    StatusCode::RANGE_NOT_SATISFIABLE,
                    "Invalid Range header",
                );
                response.headers_mut().insert(
                    CONTENT_RANGE,
                    HeaderValue::from_str(&format!("bytes */{}", metadata.len()))
                        .expect("Invalid Content-Range"),
                );
                return;
            }
            Err(RangeHeaderError::Unsatisfiable) => {
                respond_plain_text(
                    response,
                    StatusCode::RANGE_NOT_SATISFIABLE,
                    "Range not satisfiable",
                );
                response.headers_mut().insert(
                    CONTENT_RANGE,
                    HeaderValue::from_str(&format!("bytes */{}", metadata.len()))
                        .expect("Invalid Content-Range"),
                );
                return;
            }
        };

        if is_android_background_video && range.start != 0 {
            match std::fs::read(&asset_path) {
                Ok(bytes) => {
                    respond_bytes(response, StatusCode::PARTIAL_CONTENT, bytes, &mime_type);
                    response.headers_mut().insert(
                        CONTENT_RANGE,
                        HeaderValue::from_str(&format!(
                            "bytes {}-{}/{}",
                            range.start,
                            range.end,
                            metadata.len()
                        ))
                        .expect("Invalid Content-Range"),
                    );
                    response.headers_mut().insert(
                        CONTENT_LENGTH,
                        HeaderValue::from_str(&range.len().to_string())
                            .expect("Invalid Content-Length"),
                    );
                    tracing::debug!(
                        "User data asset Android video range workaround hit: {}",
                        parsed.relative_path_display
                    );
                }
                Err(error) => {
                    let status = match error.kind() {
                        std::io::ErrorKind::NotFound => StatusCode::NOT_FOUND,
                        _ => StatusCode::INTERNAL_SERVER_ERROR,
                    };
                    respond_plain_text(
                        response,
                        status,
                        &format!("Failed to read user data asset: {}", error),
                    );
                }
            }
            return;
        } else {
            let range_len = match usize::try_from(range.len()) {
                Ok(value) => value,
                Err(_) => {
                    respond_plain_text(
                        response,
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Range is too large to serve",
                    );
                    return;
                }
            };

            let mut file = match std::fs::File::open(&asset_path) {
                Ok(value) => value,
                Err(error) => {
                    respond_plain_text(
                        response,
                        StatusCode::INTERNAL_SERVER_ERROR,
                        &format!("Failed to open user data asset: {}", error),
                    );
                    return;
                }
            };

            if let Err(error) = file.seek(std::io::SeekFrom::Start(range.start)) {
                respond_plain_text(
                    response,
                    StatusCode::INTERNAL_SERVER_ERROR,
                    &format!("Failed to seek user data asset: {}", error),
                );
                return;
            }

            let mut bytes = vec![0u8; range_len];
            if let Err(error) = file.read_exact(&mut bytes) {
                respond_plain_text(
                    response,
                    StatusCode::INTERNAL_SERVER_ERROR,
                    &format!("Failed to read user data asset range: {}", error),
                );
                return;
            }

            respond_bytes(response, StatusCode::PARTIAL_CONTENT, bytes, &mime_type);
            response.headers_mut().insert(
                CONTENT_RANGE,
                HeaderValue::from_str(&format!(
                    "bytes {}-{}/{}",
                    range.start,
                    range.end,
                    metadata.len()
                ))
                .expect("Invalid Content-Range"),
            );
            response.headers_mut().insert(
                CONTENT_LENGTH,
                HeaderValue::from_str(&range.len().to_string()).expect("Invalid Content-Length"),
            );

            tracing::debug!(
                "User data asset range hit: {:?}/{}",
                parsed.kind,
                parsed.relative_path_display
            );
            return;
        }
    }

    match std::fs::read(&asset_path) {
        Ok(bytes) => {
            respond_bytes(response, StatusCode::OK, bytes, &mime_type);
            response.headers_mut().insert(
                CONTENT_LENGTH,
                HeaderValue::from_str(&metadata.len().to_string()).expect("Invalid Content-Length"),
            );
            tracing::debug!(
                "User data asset hit: {:?}/{}",
                parsed.kind,
                parsed.relative_path_display
            );
        }
        Err(error) => {
            let status = match error.kind() {
                std::io::ErrorKind::NotFound => StatusCode::NOT_FOUND,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            respond_plain_text(
                response,
                status,
                &format!("Failed to read user data asset: {}", error),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tauri::http::header::{CONTENT_LENGTH, CONTENT_RANGE};

    fn dirs(root: &PathBuf) -> DefaultUserWebDirs {
        DefaultUserWebDirs {
            characters_dir: root.join("characters"),
            avatars_dir: root.join("User Avatars"),
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
    fn serves_character_assets() {
        let temp = TempDirGuard::new("user-data-endpoint-characters");
        std::fs::create_dir_all(temp.path.join("characters")).expect("create characters dir");
        std::fs::write(temp.path.join("characters").join("a.png"), b"ok").expect("write asset");

        let request = tauri::http::Request::builder()
            .method("GET")
            .uri("/characters/a.png")
            .body(Vec::new())
            .expect("request");
        let mut response = tauri::http::Response::new(Cow::Owned(Vec::new()));

        handle_user_data_asset_web_request(&dirs(&temp.path), &request, &mut response);

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.body().as_ref(), b"ok");
    }

    #[test]
    fn serves_nested_user_files_assets() {
        let temp = TempDirGuard::new("user-data-endpoint-user-files");
        let files_dir = temp.path.join("user/files").join("nested");
        std::fs::create_dir_all(&files_dir).expect("create user files dir");
        std::fs::write(files_dir.join("a.txt"), b"ok").expect("write asset");

        let request = tauri::http::Request::builder()
            .method("GET")
            .uri("/user/files/nested/a.txt")
            .body(Vec::new())
            .expect("request");
        let mut response = tauri::http::Response::new(Cow::Owned(Vec::new()));

        handle_user_data_asset_web_request(&dirs(&temp.path), &request, &mut response);

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.body().as_ref(), b"ok");
    }

    #[test]
    fn serves_background_assets_with_single_range() {
        let temp = TempDirGuard::new("user-data-endpoint-background-range");
        std::fs::create_dir_all(temp.path.join("backgrounds")).expect("create backgrounds dir");
        std::fs::write(temp.path.join("backgrounds").join("a.bin"), b"abcd").expect("write asset");

        let request = tauri::http::Request::builder()
            .method("GET")
            .uri("/backgrounds/a.bin")
            .header("range", "bytes=1-2")
            .body(Vec::new())
            .expect("request");
        let mut response = tauri::http::Response::new(Cow::Owned(Vec::new()));

        handle_user_data_asset_web_request(&dirs(&temp.path), &request, &mut response);

        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(response.body().as_ref(), b"bc");
        assert_eq!(
            response
                .headers()
                .get(CONTENT_RANGE)
                .map(|value| value.to_str().unwrap_or("")),
            Some("bytes 1-2/4")
        );
    }

    #[test]
    fn serves_background_assets_with_suffix_range() {
        let temp = TempDirGuard::new("user-data-endpoint-background-range-suffix");
        std::fs::create_dir_all(temp.path.join("backgrounds")).expect("create backgrounds dir");
        std::fs::write(temp.path.join("backgrounds").join("a.bin"), b"abcd").expect("write asset");

        let request = tauri::http::Request::builder()
            .method("GET")
            .uri("/backgrounds/a.bin")
            .header("range", "bytes=-1")
            .body(Vec::new())
            .expect("request");
        let mut response = tauri::http::Response::new(Cow::Owned(Vec::new()));

        handle_user_data_asset_web_request(&dirs(&temp.path), &request, &mut response);

        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(response.body().as_ref(), b"d");
        assert_eq!(
            response
                .headers()
                .get(CONTENT_RANGE)
                .map(|value| value.to_str().unwrap_or("")),
            Some("bytes 3-3/4")
        );
    }

    #[test]
    fn returns_416_for_unsatisfiable_range() {
        let temp = TempDirGuard::new("user-data-endpoint-background-range-unsatisfiable");
        std::fs::create_dir_all(temp.path.join("backgrounds")).expect("create backgrounds dir");
        std::fs::write(temp.path.join("backgrounds").join("a.bin"), b"abcd").expect("write asset");

        let request = tauri::http::Request::builder()
            .method("GET")
            .uri("/backgrounds/a.bin")
            .header("range", "bytes=10-11")
            .body(Vec::new())
            .expect("request");
        let mut response = tauri::http::Response::new(Cow::Owned(Vec::new()));

        handle_user_data_asset_web_request(&dirs(&temp.path), &request, &mut response);

        assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE);
        assert_eq!(
            response
                .headers()
                .get(CONTENT_RANGE)
                .map(|value| value.to_str().unwrap_or("")),
            Some("bytes */4")
        );
    }

    #[test]
    fn serves_background_video_assets_with_single_range_on_non_android() {
        let temp = TempDirGuard::new("user-data-endpoint-background-video-range-non-android");
        std::fs::create_dir_all(temp.path.join("backgrounds")).expect("create backgrounds dir");
        std::fs::write(temp.path.join("backgrounds").join("a.mp4"), b"abcd").expect("write asset");

        let request = tauri::http::Request::builder()
            .method("GET")
            .uri("/backgrounds/a.mp4")
            .header("range", "bytes=1-2")
            .body(Vec::new())
            .expect("request");
        let mut response = tauri::http::Response::new(Cow::Owned(Vec::new()));

        handle_user_data_asset_web_request_with_policy(
            &dirs(&temp.path),
            &request,
            &mut response,
            UserDataAssetRequestPolicy {
                android_webview_reapplies_range_semantics: false,
            },
        );

        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(response.body().as_ref(), b"bc");
        assert_eq!(
            response
                .headers()
                .get(CONTENT_RANGE)
                .map(|value| value.to_str().unwrap_or("")),
            Some("bytes 1-2/4")
        );
        assert_eq!(
            response
                .headers()
                .get(CONTENT_LENGTH)
                .map(|value| value.to_str().unwrap_or("")),
            Some("2")
        );
    }

    #[test]
    fn serves_background_video_assets_with_android_range_workaround() {
        let temp = TempDirGuard::new("user-data-endpoint-background-video-range-android");
        std::fs::create_dir_all(temp.path.join("backgrounds")).expect("create backgrounds dir");
        std::fs::write(temp.path.join("backgrounds").join("a.mp4"), b"abcd").expect("write asset");

        let request = tauri::http::Request::builder()
            .method("GET")
            .uri("/backgrounds/a.mp4")
            .header("range", "bytes=1-2")
            .body(Vec::new())
            .expect("request");
        let mut response = tauri::http::Response::new(Cow::Owned(Vec::new()));

        handle_user_data_asset_web_request_with_policy(
            &dirs(&temp.path),
            &request,
            &mut response,
            UserDataAssetRequestPolicy {
                android_webview_reapplies_range_semantics: true,
            },
        );

        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(response.body().as_ref(), b"abcd");
        assert_eq!(
            response
                .headers()
                .get(CONTENT_RANGE)
                .map(|value| value.to_str().unwrap_or("")),
            Some("bytes 1-2/4")
        );
        assert_eq!(
            response
                .headers()
                .get(CONTENT_LENGTH)
                .map(|value| value.to_str().unwrap_or("")),
            Some("2")
        );
    }
}
