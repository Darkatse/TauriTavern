use mime_guess::from_path;

use crate::application::services::host_resource_service::contract::{
    HostResourceMethod, HostResourceRequest, HostResourceResponse, status,
};
use crate::application::services::host_resource_service::path_guard::validate_path_segment;
use crate::application::services::host_resource_service::policy::HostResourceRuntimePolicy;
use crate::application::services::host_resource_service::roots::HostResourceRoots;
use crate::domain::errors::DomainError;
use crate::infrastructure::persistence::thumbnail_cache::{
    ThumbnailAsset, read_thumbnail_or_original_sync,
};
use crate::infrastructure::thumbnails::{avatar_thumbnail_config, background_thumbnail_config};

const THUMBNAIL_ALLOWED_METHODS: &str = "GET, HEAD, OPTIONS";

pub(crate) fn serve_thumbnail(
    roots: &HostResourceRoots,
    policy: &HostResourceRuntimePolicy,
    request: &HostResourceRequest<'_>,
) -> HostResourceResponse {
    match request.method {
        HostResourceMethod::Options => {
            return HostResourceResponse::no_content(THUMBNAIL_ALLOWED_METHODS);
        }
        HostResourceMethod::Get | HostResourceMethod::Head => {}
        _ => return HostResourceResponse::method_not_allowed(THUMBNAIL_ALLOWED_METHODS),
    }

    let query = request.query.unwrap_or("");
    let (thumbnail_type, file) = match parse_thumbnail_query(query) {
        Ok(value) => value,
        Err(error) => {
            return HostResourceResponse::plain_text(error.status_code(), error.message());
        }
    };

    let (original_dir, thumbnail_dir, config) = match thumbnail_type.as_str() {
        "avatar" => (
            roots.characters_dir.as_path(),
            roots.thumbnails_avatar_dir.as_path(),
            avatar_thumbnail_config(),
        ),
        "persona" => (
            roots.avatars_dir.as_path(),
            roots.thumbnails_persona_dir.as_path(),
            avatar_thumbnail_config(),
        ),
        "bg" => (
            roots.backgrounds_dir.as_path(),
            roots.thumbnails_bg_dir.as_path(),
            background_thumbnail_config(),
        ),
        _ => {
            return HostResourceResponse::plain_text(status::BAD_REQUEST, "Invalid thumbnail type");
        }
    };

    let original_path = original_dir.join(&file);
    let thumbnail_path = thumbnail_dir.join(&file);

    // NOTE: This mirrors SillyTavern's `thumbnails.enabled` behavior, but scoped
    // to avatars/personas only. Some themes expect full-size avatar images.
    let use_thumbnails = match thumbnail_type.as_str() {
        "avatar" | "persona" => !policy.avatar_persona_original_images_enabled(),
        _ => true,
    };

    let asset = match read_thumbnail_asset(&original_path, &thumbnail_path, config, use_thumbnails)
    {
        Ok(value) => value,
        Err(DomainError::NotFound(_)) => {
            tracing::debug!("Thumbnail 404: type={} file={}", thumbnail_type, file);
            return HostResourceResponse::plain_text(status::NOT_FOUND, "Not Found");
        }
        Err(error) => {
            return HostResourceResponse::plain_text(
                status::INTERNAL_SERVER_ERROR,
                &error.to_string(),
            );
        }
    };

    if request.method == HostResourceMethod::Head {
        return HostResourceResponse::bytes(status::OK, Vec::new(), &asset.mime_type);
    }

    tracing::debug!("Thumbnail hit: type={} file={}", thumbnail_type, file);
    HostResourceResponse::bytes(status::OK, asset.bytes, &asset.mime_type)
}

fn read_thumbnail_asset(
    original_path: &std::path::Path,
    thumbnail_path: &std::path::Path,
    config: crate::infrastructure::persistence::thumbnail_cache::ThumbnailConfig,
    use_thumbnails: bool,
) -> Result<ThumbnailAsset, DomainError> {
    if use_thumbnails {
        return read_thumbnail_or_original_sync(original_path, thumbnail_path, config);
    }

    let metadata = std::fs::metadata(original_path).map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => DomainError::NotFound(format!(
            "Source image not found: {}",
            original_path.display()
        )),
        _ => DomainError::InternalError(format!(
            "Failed to read source image metadata '{}': {}",
            original_path.display(),
            error
        )),
    })?;

    if !metadata.is_file() {
        return Err(DomainError::NotFound(format!(
            "Source image not found: {}",
            original_path.display()
        )));
    }

    let bytes = std::fs::read(original_path).map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => DomainError::NotFound(format!(
            "Source image not found: {}",
            original_path.display()
        )),
        _ => DomainError::InternalError(format!(
            "Failed to read original image '{}': {}",
            original_path.display(),
            error
        )),
    })?;

    let mime_type = from_path(original_path)
        .first_or_octet_stream()
        .essence_str()
        .to_string();

    Ok(ThumbnailAsset { bytes, mime_type })
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
    fn status_code(self) -> u16 {
        match self {
            Self::ForbiddenFile => status::FORBIDDEN,
            _ => status::BAD_REQUEST,
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

    if normalized_type.is_empty() {
        return Err(ThumbnailQueryError::MissingType);
    }

    if file.is_empty() {
        return Err(ThumbnailQueryError::MissingFile);
    }

    if !validate_path_segment(&file) {
        return Err(ThumbnailQueryError::ForbiddenFile);
    }

    Ok((normalized_type, file))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::services::host_resource_service::contract::{
        HostResourceHeaders, header,
    };
    use std::path::PathBuf;

    fn roots(root: &PathBuf) -> HostResourceRoots {
        HostResourceRoots {
            user_css_file: root.join("_css/user.css"),
            local_extensions_dir: root.join("extensions/local"),
            global_extensions_dir: root.join("extensions/global"),
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

    fn request(method: HostResourceMethod, uri: &'static str) -> HostResourceRequest<'static> {
        let (path, query) = uri
            .split_once('?')
            .map_or((uri, None), |(path, query)| (path, Some(query)));
        HostResourceRequest::new(method, path, query, HostResourceHeaders::empty())
    }

    fn header<'a>(response: &'a HostResourceResponse, name: &str) -> Option<&'a str> {
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
    fn rejects_methods_outside_endpoint_contract() {
        let temp = TempDirGuard::new("thumbnail-endpoint-method-gate");
        let policy = HostResourceRuntimePolicy::new(false);
        let response = serve_thumbnail(
            &roots(&temp.path),
            &policy,
            &request(
                HostResourceMethod::Other,
                "/thumbnail?type=avatar&file=a.png",
            ),
        );

        assert_eq!(response.status, status::METHOD_NOT_ALLOWED);
    }

    #[test]
    fn returns_404_for_missing_thumbnail_source() {
        let temp = TempDirGuard::new("thumbnail-endpoint-404");
        let policy = HostResourceRuntimePolicy::new(false);
        std::fs::create_dir_all(temp.path.join("characters")).expect("create characters dir");

        let response = serve_thumbnail(
            &roots(&temp.path),
            &policy,
            &request(
                HostResourceMethod::Get,
                "/thumbnail?type=avatar&file=missing.png",
            ),
        );

        assert_eq!(response.status, status::NOT_FOUND);
    }

    #[test]
    fn falls_back_to_original_when_thumbnail_missing() {
        let temp = TempDirGuard::new("thumbnail-endpoint-fallback-original");
        let policy = HostResourceRuntimePolicy::new(false);
        std::fs::create_dir_all(temp.path.join("characters")).expect("create characters dir");
        std::fs::write(temp.path.join("characters").join("a.png"), b"original")
            .expect("write original");

        let response = serve_thumbnail(
            &roots(&temp.path),
            &policy,
            &request(HostResourceMethod::Get, "/thumbnail?type=avatar&file=a.png"),
        );

        assert_eq!(response.status, status::OK);
        assert_eq!(header(&response, header::CONTENT_TYPE), Some("image/png"));
        assert_eq!(response.body, b"original");
    }

    #[test]
    fn serves_cached_thumbnail_when_available() {
        let temp = TempDirGuard::new("thumbnail-endpoint-cached");
        let policy = HostResourceRuntimePolicy::new(false);
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

        let response = serve_thumbnail(
            &roots(&temp.path),
            &policy,
            &request(HostResourceMethod::Get, "/thumbnail?type=avatar&file=a.png"),
        );

        assert_eq!(response.status, status::OK);
        assert_eq!(header(&response, header::CONTENT_TYPE), Some("image/jpeg"));
        assert_eq!(response.body, b"thumb");
    }

    #[test]
    fn serves_original_avatar_when_original_images_enabled_even_if_cached_thumbnail_exists() {
        let temp = TempDirGuard::new("thumbnail-endpoint-disabled-original");
        let policy = HostResourceRuntimePolicy::new(true);
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

        let response = serve_thumbnail(
            &roots(&temp.path),
            &policy,
            &request(HostResourceMethod::Get, "/thumbnail?type=avatar&file=a.png"),
        );

        assert_eq!(response.status, status::OK);
        assert_eq!(header(&response, header::CONTENT_TYPE), Some("image/png"));
        assert_eq!(response.body, b"original");
    }

    #[test]
    fn serves_background_thumbnails() {
        let temp = TempDirGuard::new("thumbnail-endpoint-bg");
        let policy = HostResourceRuntimePolicy::new(true);
        std::fs::create_dir_all(temp.path.join("backgrounds")).expect("create backgrounds dir");
        std::fs::create_dir_all(temp.path.join("thumbnails").join("bg"))
            .expect("create thumbnail dir");
        std::fs::write(temp.path.join("backgrounds").join("a.png"), b"original")
            .expect("write original");
        std::fs::write(
            temp.path.join("thumbnails").join("bg").join("a.png"),
            b"thumb",
        )
        .expect("write thumbnail");

        let response = serve_thumbnail(
            &roots(&temp.path),
            &policy,
            &request(HostResourceMethod::Get, "/thumbnail?type=bg&file=a.png"),
        );

        assert_eq!(response.status, status::OK);
        assert_eq!(header(&response, header::CONTENT_TYPE), Some("image/jpeg"));
        assert_eq!(response.body, b"thumb");
    }

    #[test]
    fn serves_thumbnail_for_exact_file_query_segment() {
        let temp = TempDirGuard::new("thumbnail-endpoint-exact-file");
        let policy = HostResourceRuntimePolicy::new(true);
        std::fs::create_dir_all(temp.path.join("backgrounds")).expect("create backgrounds dir");
        std::fs::create_dir_all(temp.path.join("thumbnails").join("bg"))
            .expect("create thumbnail dir");
        std::fs::write(
            temp.path.join("backgrounds").join(" space.png"),
            b"original",
        )
        .expect("write original");
        std::fs::write(
            temp.path.join("thumbnails").join("bg").join(" space.png"),
            b"thumb",
        )
        .expect("write thumbnail");

        let response = serve_thumbnail(
            &roots(&temp.path),
            &policy,
            &request(
                HostResourceMethod::Get,
                "/thumbnail?type=bg&file=%20space.png",
            ),
        );

        assert_eq!(response.status, status::OK);
        assert_eq!(response.body, b"thumb");
    }

    #[test]
    fn parses_legacy_c1_thumbnail_file() {
        let (thumbnail_type, file) = parse_thumbnail_query("type=bg&file=%C3%A3%C2%80%C2%90.png")
            .expect("parse thumbnail query");

        assert_eq!(thumbnail_type, "bg");
        assert_eq!(file, "ã\u{80}\u{90}.png");
    }

    #[test]
    fn rejects_c0_control_thumbnail_file() {
        let error = parse_thumbnail_query("type=bg&file=bad%1F.png")
            .expect_err("C0 control should be forbidden");

        assert_eq!(error, ThumbnailQueryError::ForbiddenFile);
    }
}
