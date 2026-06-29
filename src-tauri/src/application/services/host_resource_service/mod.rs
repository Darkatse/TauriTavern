pub mod contract;
pub mod css_compat;
pub mod path_guard;
pub mod policy;
pub mod ports;
pub mod range;
pub mod route_classifier;
pub mod routes;

mod third_party;
mod thumbnail;
mod user_css;
mod user_data;

use std::sync::Arc;

use crate::domain::errors::DomainError;
use contract::{HostResourceRequest, HostResourceResponse};
use policy::HostResourceRuntimePolicy;
use ports::{HostResourceAssetStore, HostResourceBinaryAsset};
use route_classifier::{HostResourceRoute, classify_host_resource_route};
use user_data::UserDataAssetRequestPolicy;

pub(crate) struct HostResourceService {
    policy: Arc<HostResourceRuntimePolicy>,
    store: Arc<dyn HostResourceAssetStore>,
    user_data_policy: UserDataAssetRequestPolicy,
}

impl HostResourceService {
    pub(crate) fn new<S>(policy: Arc<HostResourceRuntimePolicy>, store: Arc<S>) -> Self
    where
        S: HostResourceAssetStore + 'static,
    {
        Self {
            policy,
            store,
            user_data_policy: UserDataAssetRequestPolicy::for_current_platform(),
        }
    }

    pub(crate) fn try_serve(
        &self,
        request: &HostResourceRequest<'_>,
    ) -> Option<HostResourceResponse> {
        match classify_host_resource_route(request)? {
            HostResourceRoute::UserCss => {
                Some(user_css::serve_user_css(self.store.as_ref(), request))
            }
            HostResourceRoute::ThirdPartyAsset => Some(third_party::serve_third_party_asset(
                self.store.as_ref(),
                request,
            )),
            HostResourceRoute::Thumbnail => Some(thumbnail::serve_thumbnail(
                self.store.as_ref(),
                self.policy.as_ref(),
                request,
            )),
            HostResourceRoute::UserDataAsset => Some(user_data::serve_user_data_asset(
                self.store.as_ref(),
                request,
                self.user_data_policy,
            )),
        }
    }

    pub(crate) async fn read_thumbnail_asset_for_command(
        &self,
        thumbnail_type: &str,
        file: &str,
    ) -> Result<HostResourceBinaryAsset, DomainError> {
        thumbnail::read_thumbnail_asset_for_command(Arc::clone(&self.store), thumbnail_type, file)
            .await
    }

    #[cfg(any(dev, debug_assertions))]
    pub(crate) fn serve_dev_resource(
        &self,
        request: &HostResourceRequest<'_>,
    ) -> HostResourceResponse {
        self.try_serve(request).unwrap_or_else(|| {
            HostResourceResponse::plain_text(contract::status::NOT_FOUND, "Not Found")
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::services::host_resource_service::contract::{
        HostResourceHeaders, HostResourceMethod,
    };
    use crate::application::services::host_resource_service::ports::{
        HostResourceBinaryAsset, HostResourceFileStat, HostResourceStoreError,
        ThumbnailAssetRequest,
    };
    use crate::application::services::host_resource_service::range::ByteRange;
    use crate::application::services::host_resource_service::routes::UserDataAssetKind;
    use std::path::Path;

    struct Store;

    impl HostResourceAssetStore for Store {
        fn read_user_css(&self) -> Result<Vec<u8>, HostResourceStoreError> {
            Ok(b"css".to_vec())
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

    #[test]
    fn facade_dispatches_known_routes_and_ignores_frontend_assets() {
        let service = HostResourceService::new(
            Arc::new(HostResourceRuntimePolicy::new(false)),
            Arc::new(Store),
        );
        let user_css = HostResourceRequest::new(
            HostResourceMethod::Get,
            "/css/user.css",
            None,
            HostResourceHeaders::empty(),
        );
        let frontend = HostResourceRequest::new(
            HostResourceMethod::Get,
            "/index.html",
            None,
            HostResourceHeaders::empty(),
        );

        assert_eq!(service.try_serve(&user_css).expect("served").body, b"css");
        assert!(service.try_serve(&frontend).is_none());
    }
}
