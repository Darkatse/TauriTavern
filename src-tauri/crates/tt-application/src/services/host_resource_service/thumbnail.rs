use std::sync::Arc;

use http::{Method, Request, StatusCode};
use tt_domain::errors::DomainError;
use tt_domain::models::filename::sanitize_filename;

use super::response::{self, RepresentationMetadata, RetrievalDecision};
use super::{HostResourceBinaryAsset, HostResourceDeliveryCapabilities, HostResourceResponse};
use crate::client_asset_paths::validate_path_segment;
use tt_ports::host_resource::{
    HostResourceAssetStore, HostResourceSourceRequest, HostResourceStoreError,
    ThumbnailAssetRequest, ThumbnailKind,
};

const THUMBNAIL_ALLOWED_METHODS: &str = "GET, HEAD, OPTIONS";

pub(super) fn serve_thumbnail(
    store: &dyn HostResourceAssetStore,
    avatar_persona_original_images_enabled: bool,
    request: &Request<Vec<u8>>,
    delivery: HostResourceDeliveryCapabilities,
) -> HostResourceResponse {
    match *request.method() {
        Method::OPTIONS => {
            return response::no_content(THUMBNAIL_ALLOWED_METHODS);
        }
        Method::GET | Method::HEAD => {}
        _ => return response::method_not_allowed(THUMBNAIL_ALLOWED_METHODS),
    }

    let query = request.uri().query().unwrap_or("");
    let (thumbnail_type, file) = match parse_thumbnail_query(query) {
        Ok(value) => value,
        Err(error) => {
            return response::error(error.status_code(), error.message());
        }
    };

    let kind = match parse_thumbnail_kind(&thumbnail_type) {
        Some(kind) => kind,
        None => {
            return response::error(StatusCode::BAD_REQUEST, "Invalid thumbnail type");
        }
    };

    let use_thumbnails = match kind {
        ThumbnailKind::Avatar | ThumbnailKind::Persona => !avatar_persona_original_images_enabled,
        ThumbnailKind::Background => true,
    };

    let opened = match store.open(HostResourceSourceRequest::Thumbnail(
        &ThumbnailAssetRequest {
            kind,
            file: file.clone(),
            use_thumbnails,
        },
    )) {
        Ok(opened) => opened,
        Err(HostResourceStoreError::NotFound(_)) => {
            tracing::debug!("Thumbnail 404: type={} file={}", thumbnail_type, file);
            return response::error(StatusCode::NOT_FOUND, "Not Found");
        }
        Err(error) => return response::store_error(error, "Not Found"),
    };
    let metadata = match RepresentationMetadata::raw(&opened.metadata, None) {
        Ok(metadata) => metadata,
        Err(error) => return response::store_error(error, "Not Found"),
    };

    match response::decide_retrieval(request, &metadata, delivery) {
        RetrievalDecision::NotModified => return response::not_modified(&metadata),
        RetrievalDecision::Head => return response::head(&metadata),
        RetrievalDecision::Full | RetrievalDecision::Continue => {}
    }

    let bytes = match opened.read(None) {
        Ok(bytes) => bytes,
        Err(error) => return response::store_error(error, "Not Found"),
    };
    tracing::debug!("Thumbnail hit: type={} file={}", thumbnail_type, file);
    response::ok(&metadata, bytes)
}

pub(super) async fn read_thumbnail_asset_for_command(
    store: Arc<dyn HostResourceAssetStore>,
    thumbnail_type: &str,
    file: &str,
) -> Result<HostResourceBinaryAsset, DomainError> {
    let kind = parse_thumbnail_kind(thumbnail_type)
        .ok_or_else(|| DomainError::InvalidData("Invalid thumbnail type".to_string()))?;
    let file = sanitize_command_thumbnail_filename(kind, file)?;

    tokio::task::spawn_blocking(move || {
        let opened = store
            .open(HostResourceSourceRequest::Thumbnail(
                &ThumbnailAssetRequest {
                    kind,
                    file,
                    use_thumbnails: true,
                },
            ))
            .map_err(domain_error_from_store_error)?;
        let mime_type = opened.metadata.content_type.clone();
        let bytes = opened.read(None).map_err(domain_error_from_store_error)?;
        Ok(HostResourceBinaryAsset { bytes, mime_type })
    })
    .await
    .map_err(|error| DomainError::InternalError(format!("Thumbnail worker failed: {error}")))?
}

fn parse_thumbnail_kind(value: &str) -> Option<ThumbnailKind> {
    match value.trim().to_ascii_lowercase().as_str() {
        "bg" => Some(ThumbnailKind::Background),
        "avatar" => Some(ThumbnailKind::Avatar),
        "persona" => Some(ThumbnailKind::Persona),
        _ => None,
    }
}

fn sanitize_command_thumbnail_filename(
    kind: ThumbnailKind,
    filename: &str,
) -> Result<String, DomainError> {
    let sanitized = match kind {
        ThumbnailKind::Background => sanitize_filename(filename),
        ThumbnailKind::Avatar | ThumbnailKind::Persona => filename
            .chars()
            .map(|character| match character {
                '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
                _ if character.is_control() => '_',
                _ => character,
            })
            .collect::<String>()
            .trim()
            .trim_end_matches(['.', ' '])
            .to_string(),
    };

    if sanitized.is_empty() {
        let message = match kind {
            ThumbnailKind::Background => "Invalid background filename",
            ThumbnailKind::Avatar | ThumbnailKind::Persona => "Invalid thumbnail file name",
        };
        return Err(DomainError::InvalidData(message.to_string()));
    }

    Ok(sanitized)
}

fn domain_error_from_store_error(error: HostResourceStoreError) -> DomainError {
    match error {
        HostResourceStoreError::NotFound(message) => DomainError::NotFound(message),
        HostResourceStoreError::Forbidden(message) | HostResourceStoreError::Internal(message) => {
            DomainError::InternalError(message)
        }
    }
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
    use std::sync::atomic::AtomicUsize;
    use std::sync::{Arc, Mutex};

    use tt_ports::host_resource::{
        HostResourceSourceRequest, HostResourceStoreError, OpenedHostResource,
    };

    use super::*;
    use crate::services::host_resource_service::test_support;

    struct Store {
        requests: Mutex<Vec<ThumbnailAssetRequest>>,
        reads: Arc<AtomicUsize>,
    }

    impl HostResourceAssetStore for Store {
        fn open(
            &self,
            request: HostResourceSourceRequest<'_>,
        ) -> Result<OpenedHostResource, HostResourceStoreError> {
            let HostResourceSourceRequest::Thumbnail(request) = request else {
                unreachable!()
            };
            self.requests.lock().expect("lock").push(request.clone());
            Ok(test_support::opened(
                b"thumbnail",
                "image/jpeg",
                Arc::clone(&self.reads),
            ))
        }
    }

    fn store() -> Store {
        Store {
            requests: Mutex::new(Vec::new()),
            reads: Arc::new(AtomicUsize::new(0)),
        }
    }

    #[test]
    fn avatar_original_policy_disables_thumbnail_cache() {
        let store = store();

        let response = serve_thumbnail(
            &store,
            true,
            &test_support::request(Method::GET, "/thumbnail?type=avatar&file=a.png"),
            HostResourceDeliveryCapabilities::new(true, false),
        );

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            store.requests.lock().expect("lock").as_slice(),
            &[ThumbnailAssetRequest {
                kind: ThumbnailKind::Avatar,
                file: "a.png".to_string(),
                use_thumbnails: false,
            }]
        );
    }

    #[test]
    fn rejects_path_like_thumbnail_files() {
        let store = store();

        let response = serve_thumbnail(
            &store,
            false,
            &test_support::request(Method::GET, "/thumbnail?type=bg&file=nested%2Fbad.png"),
            HostResourceDeliveryCapabilities::new(true, false),
        );

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert!(store.requests.lock().expect("lock").is_empty());
    }

    #[test]
    fn endpoint_ignores_animated_query_parameter() {
        let store = store();

        let response = serve_thumbnail(
            &store,
            false,
            &test_support::request(Method::GET, "/thumbnail?type=bg&file=a.png&animated=true"),
            HostResourceDeliveryCapabilities::new(true, false),
        );

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            store.requests.lock().expect("lock").as_slice(),
            &[ThumbnailAssetRequest {
                kind: ThumbnailKind::Background,
                file: "a.png".to_string(),
                use_thumbnails: true,
            }]
        );
    }

    #[test]
    fn thumbnail_head_does_not_read_body() {
        let store = store();

        let response = serve_thumbnail(
            &store,
            false,
            &test_support::request(Method::HEAD, "/thumbnail?type=avatar&file=a.png"),
            HostResourceDeliveryCapabilities::new(true, false),
        );

        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.body().is_empty());
        assert_eq!(store.reads.load(std::sync::atomic::Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn command_thumbnail_always_uses_thumbnail_cache_and_sanitizes_file() {
        let store = Arc::new(store());
        let service = crate::services::host_resource_service::HostResourceService::new(
            true,
            Arc::clone(&store),
        );

        let asset = service
            .read_thumbnail_asset_for_command(" Avatar ", " bad:name?.png. ")
            .await
            .expect("asset");

        assert_eq!(asset.bytes, b"thumbnail".to_vec());
        assert_eq!(
            store.requests.lock().expect("lock").as_slice(),
            &[ThumbnailAssetRequest {
                kind: ThumbnailKind::Avatar,
                file: "bad_name_.png".to_string(),
                use_thumbnails: true,
            }]
        );
    }

    #[tokio::test]
    async fn command_thumbnail_rejects_empty_sanitized_file() {
        let store = Arc::new(store());
        let service = crate::services::host_resource_service::HostResourceService::new(
            false,
            Arc::clone(&store),
        );

        let error = service
            .read_thumbnail_asset_for_command("bg", " ... ")
            .await
            .expect_err("invalid file");

        assert!(
            matches!(error, DomainError::InvalidData(message) if message == "Invalid background filename")
        );
        assert!(store.requests.lock().expect("lock").is_empty());
    }

    #[tokio::test]
    async fn command_background_thumbnail_uses_background_filename_sanitizer() {
        let store = Arc::new(store());
        let service = crate::services::host_resource_service::HostResourceService::new(
            false,
            Arc::clone(&store),
        );

        service
            .read_thumbnail_asset_for_command("bg", "..\\bad:*name?.png")
            .await
            .expect("asset");

        assert_eq!(
            store.requests.lock().expect("lock").as_slice(),
            &[ThumbnailAssetRequest {
                kind: ThumbnailKind::Background,
                file: "..badname.png".to_string(),
                use_thumbnails: true,
            }]
        );
    }
}
