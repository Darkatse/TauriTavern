use std::borrow::Cow;

use tauri::http::StatusCode;

use crate::infrastructure::user_data_dirs::DefaultUserWebDirs;
use crate::infrastructure::user_data_paths::{
    UserDataAssetKind, UserDataPathError, is_user_data_asset_route,
    parse_user_data_asset_request_path,
};
use crate::presentation::web_resources::response_helpers::{
    respond_bytes, respond_method_not_allowed, respond_no_content, respond_plain_text,
};

const USER_DATA_ALLOWED_METHODS: &str = "GET, HEAD, OPTIONS";

pub fn handle_user_data_asset_web_request(
    user_dirs: &DefaultUserWebDirs,
    request: &tauri::http::Request<Vec<u8>>,
    response: &mut tauri::http::Response<Cow<'static, [u8]>>,
) {
    let request_path = request.uri().path();
    if !is_user_data_asset_route(request_path) {
        return;
    }

    handle_user_data_asset_route_request(user_dirs, request, response);
}

fn handle_user_data_asset_route_request(
    user_dirs: &DefaultUserWebDirs,
    request: &tauri::http::Request<Vec<u8>>,
    response: &mut tauri::http::Response<Cow<'static, [u8]>>,
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

    if request.method() == Method::HEAD {
        respond_bytes(response, StatusCode::OK, Vec::new(), &mime_type);
        return;
    }

    match std::fs::read(&asset_path) {
        Ok(bytes) => {
            respond_bytes(response, StatusCode::OK, bytes, &mime_type);
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
        std::fs::write(temp.path.join("characters").join("a.png"), b"ok")
            .expect("write asset");

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
}
