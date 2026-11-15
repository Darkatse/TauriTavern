use http::{Method, Request, StatusCode};

use super::response::{self, RepresentationMetadata, RetrievalDecision};
use super::{HostResourceDeliveryCapabilities, HostResourceResponse};
use crate::client_asset_paths::validate_path_segment;
use tt_ports::host_resource::{
    HostResourceAssetStore, HostResourceSourceRequest, HostResourceStoreError,
    ThumbnailAssetRequest, ThumbnailKind, ThumbnailSelection,
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

    let ThumbnailQuery {
        thumbnail_type,
        file,
        static_preview,
        list,
    } = match parse_thumbnail_query(request.uri().query().unwrap_or("")) {
        Ok(query) => query,
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

    if static_preview && kind != ThumbnailKind::Background {
        return response::error(
            StatusCode::BAD_REQUEST,
            "Static previews are only supported for backgrounds",
        );
    }

    let selection = match kind {
        ThumbnailKind::Avatar | ThumbnailKind::Persona
            if avatar_persona_original_images_enabled && !list =>
        {
            ThumbnailSelection::Original
        }
        ThumbnailKind::Background if static_preview => ThumbnailSelection::RequireGenerated,
        ThumbnailKind::Avatar | ThumbnailKind::Persona | ThumbnailKind::Background => {
            ThumbnailSelection::PreferGenerated
        }
    };

    let opened = match store.open(HostResourceSourceRequest::Thumbnail(
        &ThumbnailAssetRequest {
            kind,
            file: file.clone(),
            selection,
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

    let body = match response::body(opened, None, delivery) {
        Ok(body) => body,
        Err(error) => return response::store_error(error, "Not Found"),
    };
    tracing::debug!("Thumbnail hit: type={} file={}", thumbnail_type, file);
    response::ok(&metadata, body)
}

fn parse_thumbnail_kind(value: &str) -> Option<ThumbnailKind> {
    match value.trim().to_ascii_lowercase().as_str() {
        "bg" => Some(ThumbnailKind::Background),
        "avatar" => Some(ThumbnailKind::Avatar),
        "persona" => Some(ThumbnailKind::Persona),
        _ => None,
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

struct ThumbnailQuery {
    thumbnail_type: String,
    file: String,
    static_preview: bool,
    list: bool,
}

fn parse_thumbnail_flag(value: &str) -> Result<bool, ThumbnailQueryError> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(ThumbnailQueryError::InvalidQuery),
    }
}

fn parse_thumbnail_query(query: &str) -> Result<ThumbnailQuery, ThumbnailQueryError> {
    let mut thumbnail_type = None;
    let mut file = None;
    let mut static_preview = false;
    let mut list = false;

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
            "static" => static_preview = parse_thumbnail_flag(&value)?,
            "list" => list = parse_thumbnail_flag(&value)?,
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

    Ok(ThumbnailQuery {
        thumbnail_type: normalized_type,
        file,
        static_preview,
        list,
    })
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
    fn original_images_setting_spares_list_thumbnails() {
        let store = store();

        for uri in [
            "/thumbnail?type=avatar&file=a.png",
            "/thumbnail?type=avatar&list=true&file=a.png",
        ] {
            let response = serve_thumbnail(
                &store,
                true,
                &test_support::request(Method::GET, uri),
                HostResourceDeliveryCapabilities::new(true),
            );
            assert_eq!(response.status(), StatusCode::OK);
        }

        assert_eq!(
            store.requests.lock().expect("lock").as_slice(),
            &[
                ThumbnailAssetRequest {
                    kind: ThumbnailKind::Avatar,
                    file: "a.png".to_string(),
                    selection: ThumbnailSelection::Original,
                },
                ThumbnailAssetRequest {
                    kind: ThumbnailKind::Avatar,
                    file: "a.png".to_string(),
                    selection: ThumbnailSelection::PreferGenerated,
                },
            ]
        );
    }

    #[test]
    fn rejects_path_like_thumbnail_files() {
        let store = store();

        let response = serve_thumbnail(
            &store,
            false,
            &test_support::request(Method::GET, "/thumbnail?type=bg&file=nested%2Fbad.png"),
            HostResourceDeliveryCapabilities::new(true),
        );

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert!(store.requests.lock().expect("lock").is_empty());
    }

    #[test]
    fn static_background_requires_a_generated_preview() {
        let store = store();

        let response = serve_thumbnail(
            &store,
            false,
            &test_support::request(Method::GET, "/thumbnail?type=bg&file=a.gif&static=true"),
            HostResourceDeliveryCapabilities::new(true),
        );

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            store.requests.lock().expect("lock").as_slice(),
            &[ThumbnailAssetRequest {
                kind: ThumbnailKind::Background,
                file: "a.gif".to_string(),
                selection: ThumbnailSelection::RequireGenerated,
            }]
        );
    }

    #[test]
    fn rejects_static_preview_for_non_background_assets() {
        let store = store();

        let response = serve_thumbnail(
            &store,
            false,
            &test_support::request(
                Method::GET,
                "/thumbnail?type=persona&file=a.gif&static=true",
            ),
            HostResourceDeliveryCapabilities::new(true),
        );

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(store.requests.lock().expect("lock").is_empty());
    }

    #[test]
    fn thumbnail_head_does_not_read_body() {
        let store = store();

        let response = serve_thumbnail(
            &store,
            false,
            &test_support::request(Method::HEAD, "/thumbnail?type=avatar&file=a.png"),
            HostResourceDeliveryCapabilities::new(true),
        );

        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.body().is_empty());
        assert_eq!(store.reads.load(std::sync::atomic::Ordering::Relaxed), 0);
    }
}
