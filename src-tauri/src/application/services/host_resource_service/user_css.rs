use super::contract::{
    HostResourceMethod, HostResourceRequest, HostResourceResponse, header, status,
};
use super::ports::{HostResourceAssetStore, HostResourceStoreError};

const USER_CSS_ALLOWED_METHODS: &str = "GET, HEAD, OPTIONS";
const USER_CSS_CONTENT_TYPE: &str = "text/css; charset=utf-8";

pub(super) fn serve_user_css(
    store: &dyn HostResourceAssetStore,
    request: &HostResourceRequest<'_>,
) -> HostResourceResponse {
    match request.method {
        HostResourceMethod::Options => {
            return HostResourceResponse::no_content(USER_CSS_ALLOWED_METHODS);
        }
        HostResourceMethod::Get | HostResourceMethod::Head => {}
        _ => return HostResourceResponse::method_not_allowed(USER_CSS_ALLOWED_METHODS),
    }

    let bytes = match store.read_user_css() {
        Ok(bytes) => bytes,
        Err(HostResourceStoreError::NotFound(_)) => {
            return HostResourceResponse::plain_text(status::NOT_FOUND, "User CSS not found");
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
    use crate::application::client_asset_paths::UserDataAssetKind;
    use crate::application::services::host_resource_service::contract::HostResourceHeaders;
    use crate::application::services::host_resource_service::ports::{
        HostResourceBinaryAsset, HostResourceFileStat, ThumbnailAssetRequest,
    };
    use crate::application::services::host_resource_service::range::ByteRange;
    use std::path::Path;

    struct Store {
        css: Result<Vec<u8>, HostResourceStoreError>,
    }

    impl HostResourceAssetStore for Store {
        fn read_user_css(&self) -> Result<Vec<u8>, HostResourceStoreError> {
            self.css.clone()
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
            _request: ThumbnailAssetRequest,
        ) -> Result<HostResourceBinaryAsset, HostResourceStoreError> {
            unreachable!()
        }
    }

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

    #[test]
    fn serves_user_css_and_head_keeps_length() {
        let store = Store {
            css: Ok(b"body {}".to_vec()),
        };

        let get = serve_user_css(&store, &request(HostResourceMethod::Get));
        let head = serve_user_css(&store, &request(HostResourceMethod::Head));

        assert_eq!(get.status, status::OK);
        assert_eq!(get.body, b"body {}");
        assert_eq!(head.status, status::OK);
        assert!(head.body.is_empty());
        assert_eq!(header(&head, header::CONTENT_LENGTH), Some("7"));
    }

    #[test]
    fn returns_not_found_when_user_css_is_missing() {
        let store = Store {
            css: Err(HostResourceStoreError::not_found("missing")),
        };

        let response = serve_user_css(&store, &request(HostResourceMethod::Get));

        assert_eq!(response.status, status::NOT_FOUND);
    }
}
