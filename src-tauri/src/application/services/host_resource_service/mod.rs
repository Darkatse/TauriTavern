pub mod contract;
pub mod ports;
pub mod range;

mod css_compat;
mod route_classifier;
mod third_party;
mod thumbnail;
mod user_css;
mod user_data;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::domain::errors::DomainError;
use contract::{HostResourceRequest, HostResourceResponse, header};
use ports::{HostResourceAssetStore, HostResourceBinaryAsset};
use route_classifier::{HostResourceRoute, classify_host_resource_route};
use user_data::UserDataAssetRequestPolicy;

static NEXT_TRACE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(crate) struct HostResourceService {
    avatar_persona_original_images_enabled: AtomicBool,
    store: Arc<dyn HostResourceAssetStore>,
    user_data_policy: UserDataAssetRequestPolicy,
}

impl HostResourceService {
    pub(crate) fn new<S>(avatar_persona_original_images_enabled: bool, store: Arc<S>) -> Self
    where
        S: HostResourceAssetStore + 'static,
    {
        Self {
            avatar_persona_original_images_enabled: AtomicBool::new(
                avatar_persona_original_images_enabled,
            ),
            store,
            user_data_policy: UserDataAssetRequestPolicy::for_current_platform(),
        }
    }

    pub(crate) fn try_serve(
        &self,
        request: &HostResourceRequest<'_>,
    ) -> Option<HostResourceResponse> {
        let response = match classify_host_resource_route(request)? {
            HostResourceRoute::UserCss => {
                Some(user_css::serve_user_css(self.store.as_ref(), request))
            }
            HostResourceRoute::ThirdPartyAsset => Some(third_party::serve_third_party_asset(
                self.store.as_ref(),
                request,
            )),
            HostResourceRoute::Thumbnail => Some(thumbnail::serve_thumbnail(
                self.store.as_ref(),
                self.avatar_persona_original_images_enabled
                    .load(Ordering::Relaxed),
                request,
            )),
            HostResourceRoute::UserDataAsset => Some(user_data::serve_user_data_asset(
                self.store.as_ref(),
                request,
                self.user_data_policy,
            )),
        }?;

        Some(with_trace_id(response))
    }

    pub(crate) async fn read_thumbnail_asset_for_command(
        &self,
        thumbnail_type: &str,
        file: &str,
    ) -> Result<HostResourceBinaryAsset, DomainError> {
        thumbnail::read_thumbnail_asset_for_command(Arc::clone(&self.store), thumbnail_type, file)
            .await
    }

    pub(crate) fn set_avatar_persona_original_images_enabled(&self, enabled: bool) {
        self.avatar_persona_original_images_enabled
            .store(enabled, Ordering::Relaxed);
    }

    #[cfg(any(dev, debug_assertions))]
    pub(crate) fn serve_dev_resource(
        &self,
        request: &HostResourceRequest<'_>,
    ) -> HostResourceResponse {
        self.try_serve(request).unwrap_or_else(|| {
            with_trace_id(HostResourceResponse::plain_text(
                contract::status::NOT_FOUND,
                "Not Found",
            ))
        })
    }
}

fn with_trace_id(response: HostResourceResponse) -> HostResourceResponse {
    response.with_header(header::TAURITAVERN_TRACE_ID, next_trace_id())
}

fn next_trace_id() -> String {
    let sequence = NEXT_TRACE_SEQUENCE.fetch_add(1, Ordering::Relaxed) + 1;
    format!("hr-{sequence}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::client_asset_paths::UserDataAssetKind;
    use crate::application::services::host_resource_service::contract::{
        HostResourceHeaders, HostResourceMethod, header,
    };
    use crate::application::services::host_resource_service::ports::{
        HostResourceBinaryAsset, HostResourceFileStat, HostResourceStoreError,
        ThumbnailAssetRequest, ThumbnailKind,
    };
    use crate::application::services::host_resource_service::range::ByteRange;
    use std::path::Path;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Store {
        thumbnail_requests: Mutex<Vec<ThumbnailAssetRequest>>,
    }

    impl HostResourceAssetStore for Store {
        fn read_user_css(&self) -> Result<Vec<u8>, HostResourceStoreError> {
            Ok(b"css".to_vec())
        }

        fn stat_third_party_asset(
            &self,
            _extension_folder: &str,
            _relative_path: &Path,
        ) -> Result<HostResourceFileStat, HostResourceStoreError> {
            Ok(HostResourceFileStat {
                len: 5,
                mime_type: "application/javascript".to_string(),
            })
        }

        fn read_third_party_asset(
            &self,
            _extension_folder: &str,
            _relative_path: &Path,
            _max_len: Option<u64>,
        ) -> Result<HostResourceBinaryAsset, HostResourceStoreError> {
            Ok(HostResourceBinaryAsset {
                bytes: b"third".to_vec(),
                mime_type: "application/javascript".to_string(),
            })
        }

        fn stat_user_data_asset(
            &self,
            _kind: UserDataAssetKind,
            _relative_path: &Path,
        ) -> Result<HostResourceFileStat, HostResourceStoreError> {
            Ok(HostResourceFileStat {
                len: 4,
                mime_type: "image/png".to_string(),
            })
        }

        fn read_user_data_asset(
            &self,
            _kind: UserDataAssetKind,
            _relative_path: &Path,
        ) -> Result<Vec<u8>, HostResourceStoreError> {
            Ok(b"data".to_vec())
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
            self.thumbnail_requests.lock().expect("lock").push(request);
            Ok(HostResourceBinaryAsset {
                bytes: b"thumbnail".to_vec(),
                mime_type: "image/jpeg".to_string(),
            })
        }
    }

    fn header_value<'a>(response: &'a HostResourceResponse, name: &str) -> Option<&'a str> {
        response
            .headers
            .iter()
            .find(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    #[test]
    fn facade_dispatches_known_routes_and_ignores_frontend_assets() {
        let service = HostResourceService::new(false, Arc::new(Store::default()));
        let user_css = HostResourceRequest::new(
            HostResourceMethod::Get,
            "/css/user.css",
            None,
            HostResourceHeaders::empty(),
        );
        let third_party = HostResourceRequest::new(
            HostResourceMethod::Get,
            "/scripts/extensions/third-party/mobile/app.js",
            None,
            HostResourceHeaders::empty(),
        );
        let user_data = HostResourceRequest::new(
            HostResourceMethod::Get,
            "/backgrounds/a.png",
            None,
            HostResourceHeaders::empty(),
        );
        let frontend = HostResourceRequest::new(
            HostResourceMethod::Get,
            "/index.html",
            None,
            HostResourceHeaders::empty(),
        );

        let user_css_response = service.try_serve(&user_css).expect("served");
        let third_party_response = service.try_serve(&third_party).expect("served");
        let user_data_response = service.try_serve(&user_data).expect("served");

        assert_eq!(user_css_response.body, b"css");
        assert_eq!(third_party_response.body, b"third");
        assert_eq!(user_data_response.body, b"data");
        assert!(
            header_value(&user_css_response, header::TAURITAVERN_TRACE_ID)
                .is_some_and(|value| value.starts_with("hr-"))
        );
        assert_ne!(
            header_value(&user_css_response, header::TAURITAVERN_TRACE_ID),
            header_value(&third_party_response, header::TAURITAVERN_TRACE_ID)
        );
        assert!(service.try_serve(&frontend).is_none());

        let dev_fallback = service.serve_dev_resource(&frontend);
        assert_eq!(dev_fallback.status, contract::status::NOT_FOUND);
        assert!(
            header_value(&dev_fallback, header::TAURITAVERN_TRACE_ID)
                .is_some_and(|value| value.starts_with("hr-"))
        );
    }

    #[test]
    fn runtime_thumbnail_setting_updates_subsequent_requests() {
        let store = Arc::new(Store::default());
        let service = HostResourceService::new(false, Arc::clone(&store));
        let avatar = HostResourceRequest::new(
            HostResourceMethod::Get,
            "/thumbnail",
            Some("type=avatar&file=a.png"),
            HostResourceHeaders::empty(),
        );

        service.try_serve(&avatar).expect("served");
        service.set_avatar_persona_original_images_enabled(true);
        service.try_serve(&avatar).expect("served");

        assert_eq!(
            store.thumbnail_requests.lock().expect("lock").as_slice(),
            &[
                ThumbnailAssetRequest {
                    kind: ThumbnailKind::Avatar,
                    file: "a.png".to_string(),
                    use_thumbnails: true,
                },
                ThumbnailAssetRequest {
                    kind: ThumbnailKind::Avatar,
                    file: "a.png".to_string(),
                    use_thumbnails: false,
                },
            ]
        );
    }
}
