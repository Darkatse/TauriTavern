use std::io::{Read, Seek};

use crate::application::services::host_resource_service::contract::{
    HostResourceMethod, HostResourceRequest, HostResourceResponse, header, status,
};
use crate::application::services::host_resource_service::range::{
    RangeHeaderError, parse_single_range_header,
};
use crate::application::services::host_resource_service::roots::HostResourceRoots;
use crate::application::services::host_resource_service::routes::{
    UserDataAssetKind, UserDataPathError, parse_user_data_asset_request_path,
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

pub(crate) fn serve_user_data_asset(
    roots: &HostResourceRoots,
    request: &HostResourceRequest<'_>,
) -> HostResourceResponse {
    serve_user_data_asset_with_policy(
        roots,
        request,
        UserDataAssetRequestPolicy::for_current_platform(),
    )
}

fn serve_user_data_asset_with_policy(
    roots: &HostResourceRoots,
    request: &HostResourceRequest<'_>,
    policy: UserDataAssetRequestPolicy,
) -> HostResourceResponse {
    match request.method {
        HostResourceMethod::Options => {
            return HostResourceResponse::no_content(USER_DATA_ALLOWED_METHODS);
        }
        HostResourceMethod::Get | HostResourceMethod::Head => {}
        _ => return HostResourceResponse::method_not_allowed(USER_DATA_ALLOWED_METHODS),
    }

    let parsed = match parse_user_data_asset_request_path(request.path) {
        Ok(Some(value)) => value,
        Ok(None) => return HostResourceResponse::plain_text(status::NOT_FOUND, "Not Found"),
        Err(UserDataPathError::MissingAssetPath) => {
            return HostResourceResponse::plain_text(status::NOT_FOUND, "Not Found");
        }
        Err(UserDataPathError::InvalidPath) => {
            return HostResourceResponse::plain_text(status::BAD_REQUEST, "Invalid asset path");
        }
    };

    let base_dir = match parsed.kind {
        UserDataAssetKind::Character => roots.characters_dir.as_path(),
        UserDataAssetKind::Persona => roots.avatars_dir.as_path(),
        UserDataAssetKind::Background => roots.backgrounds_dir.as_path(),
        UserDataAssetKind::Asset => roots.assets_dir.as_path(),
        UserDataAssetKind::UserImage => roots.user_images_dir.as_path(),
        UserDataAssetKind::UserFile => roots.user_files_dir.as_path(),
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
                std::io::ErrorKind::NotFound => status::NOT_FOUND,
                _ => status::INTERNAL_SERVER_ERROR,
            };
            return HostResourceResponse::plain_text(
                status,
                &format!("Failed to stat user data asset: {}", error),
            );
        }
    };

    if !metadata.is_file() {
        return HostResourceResponse::plain_text(status::NOT_FOUND, "Not Found");
    }

    if request.method == HostResourceMethod::Head {
        return with_accept_ranges(
            HostResourceResponse::bytes(status::OK, Vec::new(), &mime_type)
                .with_header(header::CONTENT_LENGTH, metadata.len().to_string()),
        );
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

    if let Some(range_header) = request.headers.get(header::RANGE) {
        let header_value = match std::str::from_utf8(range_header) {
            Ok(value) => value,
            Err(_) => {
                return range_not_satisfiable("Invalid Range header", metadata.len());
            }
        };

        let range = match parse_single_range_header(header_value, metadata.len()) {
            Ok(value) => value,
            Err(RangeHeaderError::Invalid) => {
                return range_not_satisfiable("Invalid Range header", metadata.len());
            }
            Err(RangeHeaderError::Unsatisfiable) => {
                return range_not_satisfiable("Range not satisfiable", metadata.len());
            }
        };

        if is_android_background_video && range.start != 0 {
            match std::fs::read(&asset_path) {
                Ok(bytes) => {
                    let response =
                        HostResourceResponse::bytes(status::PARTIAL_CONTENT, bytes, &mime_type)
                            .with_header(
                                header::CONTENT_RANGE,
                                format!("bytes {}-{}/{}", range.start, range.end, metadata.len()),
                            )
                            .with_header(header::CONTENT_LENGTH, range.len().to_string());
                    tracing::debug!(
                        "User data asset Android video range workaround hit: {}",
                        parsed.relative_path_display
                    );
                    return with_accept_ranges(response);
                }
                Err(error) => {
                    let status = match error.kind() {
                        std::io::ErrorKind::NotFound => status::NOT_FOUND,
                        _ => status::INTERNAL_SERVER_ERROR,
                    };
                    return with_accept_ranges(HostResourceResponse::plain_text(
                        status,
                        &format!("Failed to read user data asset: {}", error),
                    ));
                }
            }
        } else {
            let range_len = match usize::try_from(range.len()) {
                Ok(value) => value,
                Err(_) => {
                    return with_accept_ranges(HostResourceResponse::plain_text(
                        status::INTERNAL_SERVER_ERROR,
                        "Range is too large to serve",
                    ));
                }
            };

            let mut file = match std::fs::File::open(&asset_path) {
                Ok(value) => value,
                Err(error) => {
                    return with_accept_ranges(HostResourceResponse::plain_text(
                        status::INTERNAL_SERVER_ERROR,
                        &format!("Failed to open user data asset: {}", error),
                    ));
                }
            };

            if let Err(error) = file.seek(std::io::SeekFrom::Start(range.start)) {
                return with_accept_ranges(HostResourceResponse::plain_text(
                    status::INTERNAL_SERVER_ERROR,
                    &format!("Failed to seek user data asset: {}", error),
                ));
            }

            let mut bytes = vec![0u8; range_len];
            if let Err(error) = file.read_exact(&mut bytes) {
                return with_accept_ranges(HostResourceResponse::plain_text(
                    status::INTERNAL_SERVER_ERROR,
                    &format!("Failed to read user data asset range: {}", error),
                ));
            }

            let response = HostResourceResponse::bytes(status::PARTIAL_CONTENT, bytes, &mime_type)
                .with_header(
                    header::CONTENT_RANGE,
                    format!("bytes {}-{}/{}", range.start, range.end, metadata.len()),
                )
                .with_header(header::CONTENT_LENGTH, range.len().to_string());

            tracing::debug!(
                "User data asset range hit: {:?}/{}",
                parsed.kind,
                parsed.relative_path_display
            );
            return with_accept_ranges(response);
        }
    }

    match std::fs::read(&asset_path) {
        Ok(bytes) => {
            tracing::debug!(
                "User data asset hit: {:?}/{}",
                parsed.kind,
                parsed.relative_path_display
            );
            with_accept_ranges(
                HostResourceResponse::bytes(status::OK, bytes, &mime_type)
                    .with_header(header::CONTENT_LENGTH, metadata.len().to_string()),
            )
        }
        Err(error) => {
            let status = match error.kind() {
                std::io::ErrorKind::NotFound => status::NOT_FOUND,
                _ => status::INTERNAL_SERVER_ERROR,
            };
            with_accept_ranges(HostResourceResponse::plain_text(
                status,
                &format!("Failed to read user data asset: {}", error),
            ))
        }
    }
}

fn with_accept_ranges(response: HostResourceResponse) -> HostResourceResponse {
    response.with_header(header::ACCEPT_RANGES, "bytes")
}

fn range_not_satisfiable(message: &str, total_size: u64) -> HostResourceResponse {
    with_accept_ranges(
        HostResourceResponse::plain_text(status::RANGE_NOT_SATISFIABLE, message)
            .with_header(header::CONTENT_RANGE, format!("bytes */{}", total_size)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::services::host_resource_service::contract::{
        HostResourceHeader, HostResourceHeaders,
    };
    use std::path::PathBuf;

    fn roots(root: &PathBuf) -> HostResourceRoots {
        HostResourceRoots {
            user_css_file: root.join("_css/user.css"),
            local_extensions_dir: root.join("extensions/local"),
            global_extensions_dir: root.join("extensions/global"),
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

    fn request(method: HostResourceMethod, uri: &'static str) -> HostResourceRequest<'static> {
        request_with_headers(method, uri, &[])
    }

    fn request_with_headers<'a>(
        method: HostResourceMethod,
        uri: &'static str,
        headers: &'a [HostResourceHeader<'a>],
    ) -> HostResourceRequest<'a> {
        let (path, query) = uri
            .split_once('?')
            .map_or((uri, None), |(path, query)| (path, Some(query)));
        HostResourceRequest::new(method, path, query, HostResourceHeaders::new(headers))
    }

    fn response_header<'a>(response: &'a HostResourceResponse, name: &str) -> Option<&'a str> {
        response
            .headers
            .iter()
            .find(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
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

        let response = serve_user_data_asset(
            &roots(&temp.path),
            &request(HostResourceMethod::Get, "/characters/a.png"),
        );

        assert_eq!(response.status, status::OK);
        assert_eq!(response.body, b"ok");
    }

    #[test]
    fn serves_legacy_c1_background_asset() {
        let temp = TempDirGuard::new("user-data-endpoint-background-c1");
        std::fs::create_dir_all(temp.path.join("backgrounds")).expect("create backgrounds dir");
        std::fs::write(
            temp.path.join("backgrounds").join("ã\u{80}\u{90}.png"),
            b"ok",
        )
        .expect("write asset");

        let response = serve_user_data_asset(
            &roots(&temp.path),
            &request(
                HostResourceMethod::Get,
                "/backgrounds/%C3%A3%C2%80%C2%90.png",
            ),
        );

        assert_eq!(response.status, status::OK);
        assert_eq!(response.body, b"ok");
    }

    #[test]
    fn rejects_c0_control_background_asset_path() {
        let temp = TempDirGuard::new("user-data-endpoint-background-c0");
        let response = serve_user_data_asset(
            &roots(&temp.path),
            &request(HostResourceMethod::Get, "/backgrounds/bad%1F.png"),
        );

        assert_eq!(response.status, status::BAD_REQUEST);
    }

    #[test]
    fn serves_nested_user_files_assets() {
        let temp = TempDirGuard::new("user-data-endpoint-user-files");
        let files_dir = temp.path.join("user/files").join("nested");
        std::fs::create_dir_all(&files_dir).expect("create user files dir");
        std::fs::write(files_dir.join("a.txt"), b"ok").expect("write asset");

        let response = serve_user_data_asset(
            &roots(&temp.path),
            &request(HostResourceMethod::Get, "/user/files/nested/a.txt"),
        );

        assert_eq!(response.status, status::OK);
        assert_eq!(response.body, b"ok");
    }

    #[test]
    fn serves_background_assets_with_single_range() {
        let temp = TempDirGuard::new("user-data-endpoint-background-range");
        std::fs::create_dir_all(temp.path.join("backgrounds")).expect("create backgrounds dir");
        std::fs::write(temp.path.join("backgrounds").join("a.bin"), b"abcd").expect("write asset");

        let headers = [HostResourceHeader {
            name: header::RANGE,
            value: b"bytes=1-2",
        }];
        let request = request_with_headers(HostResourceMethod::Get, "/backgrounds/a.bin", &headers);

        let response = serve_user_data_asset(&roots(&temp.path), &request);

        assert_eq!(response.status, status::PARTIAL_CONTENT);
        assert_eq!(response.body, b"bc");
        assert_eq!(
            response_header(&response, header::CONTENT_RANGE),
            Some("bytes 1-2/4")
        );
    }

    #[test]
    fn serves_background_assets_with_suffix_range() {
        let temp = TempDirGuard::new("user-data-endpoint-background-range-suffix");
        std::fs::create_dir_all(temp.path.join("backgrounds")).expect("create backgrounds dir");
        std::fs::write(temp.path.join("backgrounds").join("a.bin"), b"abcd").expect("write asset");

        let headers = [HostResourceHeader {
            name: header::RANGE,
            value: b"bytes=-1",
        }];
        let request = request_with_headers(HostResourceMethod::Get, "/backgrounds/a.bin", &headers);

        let response = serve_user_data_asset(&roots(&temp.path), &request);

        assert_eq!(response.status, status::PARTIAL_CONTENT);
        assert_eq!(response.body, b"d");
        assert_eq!(
            response_header(&response, header::CONTENT_RANGE),
            Some("bytes 3-3/4")
        );
    }

    #[test]
    fn returns_416_for_unsatisfiable_range() {
        let temp = TempDirGuard::new("user-data-endpoint-background-range-unsatisfiable");
        std::fs::create_dir_all(temp.path.join("backgrounds")).expect("create backgrounds dir");
        std::fs::write(temp.path.join("backgrounds").join("a.bin"), b"abcd").expect("write asset");

        let headers = [HostResourceHeader {
            name: header::RANGE,
            value: b"bytes=10-11",
        }];
        let request = request_with_headers(HostResourceMethod::Get, "/backgrounds/a.bin", &headers);

        let response = serve_user_data_asset(&roots(&temp.path), &request);

        assert_eq!(response.status, status::RANGE_NOT_SATISFIABLE);
        assert_eq!(
            response_header(&response, header::CONTENT_RANGE),
            Some("bytes */4")
        );
    }

    #[test]
    fn serves_background_video_assets_with_single_range_on_non_android() {
        let temp = TempDirGuard::new("user-data-endpoint-background-video-range-non-android");
        std::fs::create_dir_all(temp.path.join("backgrounds")).expect("create backgrounds dir");
        std::fs::write(temp.path.join("backgrounds").join("a.mp4"), b"abcd").expect("write asset");

        let headers = [HostResourceHeader {
            name: header::RANGE,
            value: b"bytes=1-2",
        }];
        let request = request_with_headers(HostResourceMethod::Get, "/backgrounds/a.mp4", &headers);

        let response = serve_user_data_asset_with_policy(
            &roots(&temp.path),
            &request,
            UserDataAssetRequestPolicy {
                android_webview_reapplies_range_semantics: false,
            },
        );

        assert_eq!(response.status, status::PARTIAL_CONTENT);
        assert_eq!(response.body, b"bc");
        assert_eq!(
            response_header(&response, header::CONTENT_RANGE),
            Some("bytes 1-2/4")
        );
        assert_eq!(
            response_header(&response, header::CONTENT_LENGTH),
            Some("2")
        );
    }

    #[test]
    fn serves_background_video_assets_with_android_range_workaround() {
        let temp = TempDirGuard::new("user-data-endpoint-background-video-range-android");
        std::fs::create_dir_all(temp.path.join("backgrounds")).expect("create backgrounds dir");
        std::fs::write(temp.path.join("backgrounds").join("a.mp4"), b"abcd").expect("write asset");

        let headers = [HostResourceHeader {
            name: header::RANGE,
            value: b"bytes=1-2",
        }];
        let request = request_with_headers(HostResourceMethod::Get, "/backgrounds/a.mp4", &headers);

        let response = serve_user_data_asset_with_policy(
            &roots(&temp.path),
            &request,
            UserDataAssetRequestPolicy {
                android_webview_reapplies_range_semantics: true,
            },
        );

        assert_eq!(response.status, status::PARTIAL_CONTENT);
        assert_eq!(response.body, b"abcd");
        assert_eq!(
            response_header(&response, header::CONTENT_RANGE),
            Some("bytes 1-2/4")
        );
        assert_eq!(
            response_header(&response, header::CONTENT_LENGTH),
            Some("2")
        );
    }
}
