use super::contract::{HostResourceMethod, HostResourceRequest, HostResourceResponse, status};
use super::path_guard::validate_path_segment;
use super::policy::HostResourceRuntimePolicy;
use super::ports::{
    HostResourceAssetStore, HostResourceStoreError, ThumbnailAssetRequest, ThumbnailKind,
};

const THUMBNAIL_ALLOWED_METHODS: &str = "GET, HEAD, OPTIONS";

pub(super) fn serve_thumbnail(
    store: &dyn HostResourceAssetStore,
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

    let kind = match thumbnail_type.as_str() {
        "avatar" => ThumbnailKind::Avatar,
        "persona" => ThumbnailKind::Persona,
        "bg" => ThumbnailKind::Background,
        _ => {
            return HostResourceResponse::plain_text(status::BAD_REQUEST, "Invalid thumbnail type");
        }
    };

    let use_thumbnails = match kind {
        ThumbnailKind::Avatar | ThumbnailKind::Persona => {
            !policy.avatar_persona_original_images_enabled()
        }
        ThumbnailKind::Background => true,
    };

    let asset = match store.read_thumbnail_asset(ThumbnailAssetRequest {
        kind,
        file: file.clone(),
        use_thumbnails,
    }) {
        Ok(asset) => asset,
        Err(HostResourceStoreError::NotFound(_)) => {
            tracing::debug!("Thumbnail 404: type={} file={}", thumbnail_type, file);
            return HostResourceResponse::plain_text(status::NOT_FOUND, "Not Found");
        }
        Err(HostResourceStoreError::Forbidden(message)) => {
            return HostResourceResponse::plain_text(status::FORBIDDEN, &message);
        }
        Err(HostResourceStoreError::Internal(message)) => {
            return HostResourceResponse::plain_text(status::INTERNAL_SERVER_ERROR, &message);
        }
        Err(HostResourceStoreError::PayloadTooLarge { .. }) => {
            return HostResourceResponse::plain_text(
                status::PAYLOAD_TOO_LARGE,
                "Host resource is too large to load.",
            );
        }
    };

    if request.method == HostResourceMethod::Head {
        return HostResourceResponse::bytes(status::OK, Vec::new(), &asset.mime_type);
    }

    tracing::debug!("Thumbnail hit: type={} file={}", thumbnail_type, file);
    HostResourceResponse::bytes(status::OK, asset.bytes, &asset.mime_type)
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
    use crate::application::services::host_resource_service::contract::HostResourceHeaders;
    use crate::application::services::host_resource_service::ports::{
        HostResourceBinaryAsset, HostResourceFileStat,
    };
    use crate::application::services::host_resource_service::range::ByteRange;
    use crate::application::services::host_resource_service::routes::UserDataAssetKind;
    use std::path::Path;
    use std::sync::Mutex;

    struct Store {
        requests: Mutex<Vec<ThumbnailAssetRequest>>,
    }

    impl HostResourceAssetStore for Store {
        fn read_user_css(&self) -> Result<Vec<u8>, HostResourceStoreError> {
            unreachable!()
        }

        fn stat_third_party_asset(
            &self,
            _extension_folder: &str,
            _relative_path: &Path,
        ) -> Result<HostResourceFileStat, HostResourceStoreError> {
            unreachable!()
        }

        fn read_third_party_asset(
            &self,
            _extension_folder: &str,
            _relative_path: &Path,
            _max_len: Option<u64>,
        ) -> Result<HostResourceBinaryAsset, HostResourceStoreError> {
            unreachable!()
        }

        fn stat_user_data_asset(
            &self,
            _kind: UserDataAssetKind,
            _relative_path: &Path,
        ) -> Result<HostResourceFileStat, HostResourceStoreError> {
            unreachable!()
        }

        fn read_user_data_asset(
            &self,
            _kind: UserDataAssetKind,
            _relative_path: &Path,
        ) -> Result<Vec<u8>, HostResourceStoreError> {
            unreachable!()
        }

        fn read_user_data_asset_range(
            &self,
            _kind: UserDataAssetKind,
            _relative_path: &Path,
            _range: ByteRange,
        ) -> Result<Vec<u8>, HostResourceStoreError> {
            unreachable!()
        }

        fn read_thumbnail_asset(
            &self,
            request: ThumbnailAssetRequest,
        ) -> Result<HostResourceBinaryAsset, HostResourceStoreError> {
            self.requests.lock().expect("lock").push(request);
            Ok(HostResourceBinaryAsset {
                bytes: b"thumbnail".to_vec(),
                mime_type: "image/jpeg".to_string(),
            })
        }
    }

    fn request(method: HostResourceMethod, uri: &'static str) -> HostResourceRequest<'static> {
        let (path, query) = uri
            .split_once('?')
            .map_or((uri, None), |(path, query)| (path, Some(query)));
        HostResourceRequest::new(method, path, query, HostResourceHeaders::empty())
    }

    #[test]
    fn avatar_original_policy_disables_thumbnail_cache() {
        let store = Store {
            requests: Mutex::new(Vec::new()),
        };
        let policy = HostResourceRuntimePolicy::new(true);

        let response = serve_thumbnail(
            &store,
            &policy,
            &request(HostResourceMethod::Get, "/thumbnail?type=avatar&file=a.png"),
        );

        assert_eq!(response.status, status::OK);
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
        let store = Store {
            requests: Mutex::new(Vec::new()),
        };
        let policy = HostResourceRuntimePolicy::new(false);

        let response = serve_thumbnail(
            &store,
            &policy,
            &request(
                HostResourceMethod::Get,
                "/thumbnail?type=bg&file=nested%2Fbad.png",
            ),
        );

        assert_eq!(response.status, status::FORBIDDEN);
        assert!(store.requests.lock().expect("lock").is_empty());
    }
}
