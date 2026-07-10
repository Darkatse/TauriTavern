mod range;

mod css_compat;
mod response;
mod route_classifier;
mod third_party;
mod thumbnail;
mod user_css;
mod user_data;

#[cfg(test)]
mod test_support;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use http::header::{HeaderName, HeaderValue};
use http::{Method, Request, Response, StatusCode};
use route_classifier::{HostResourceRoute, classify_host_resource_route};
use tt_ports::host_resource::HostResourceAssetStore;

static NEXT_TRACE_SEQUENCE: AtomicU64 = AtomicU64::new(0);
const TAURITAVERN_TRACE_ID: HeaderName = HeaderName::from_static("x-tauritavern-trace-id");

pub type HostResourceResponse = Response<Vec<u8>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostResourceDeliveryCapabilities {
    supports_not_modified: bool,
    webview_reapplies_range_semantics: bool,
}

impl HostResourceDeliveryCapabilities {
    pub const fn new(supports_not_modified: bool, webview_reapplies_range_semantics: bool) -> Self {
        Self {
            supports_not_modified,
            webview_reapplies_range_semantics,
        }
    }

    const fn supports_not_modified(self) -> bool {
        self.supports_not_modified
    }

    const fn webview_reapplies_range_semantics(self) -> bool {
        self.webview_reapplies_range_semantics
    }
}

pub struct HostResourceService {
    avatar_persona_original_images_enabled: AtomicBool,
    store: Arc<dyn HostResourceAssetStore>,
}

impl HostResourceService {
    pub fn new<S>(avatar_persona_original_images_enabled: bool, store: Arc<S>) -> Self
    where
        S: HostResourceAssetStore + 'static,
    {
        Self {
            avatar_persona_original_images_enabled: AtomicBool::new(
                avatar_persona_original_images_enabled,
            ),
            store,
        }
    }

    pub fn try_serve(
        &self,
        request: &Request<Vec<u8>>,
        delivery: HostResourceDeliveryCapabilities,
    ) -> Option<HostResourceResponse> {
        let response = match classify_host_resource_route(request)? {
            HostResourceRoute::UserCss => Some(user_css::serve_user_css(
                self.store.as_ref(),
                request,
                delivery,
            )),
            HostResourceRoute::ThirdPartyAsset => Some(third_party::serve_third_party_asset(
                self.store.as_ref(),
                request,
                delivery,
            )),
            HostResourceRoute::Thumbnail => Some(thumbnail::serve_thumbnail(
                self.store.as_ref(),
                self.avatar_persona_original_images_enabled
                    .load(Ordering::Relaxed),
                request,
                delivery,
            )),
            HostResourceRoute::UserDataAsset => Some(user_data::serve_user_data_asset(
                self.store.as_ref(),
                request,
                delivery,
            )),
        }?;

        Some(finalize_response(request, response))
    }

    pub fn set_avatar_persona_original_images_enabled(&self, enabled: bool) {
        self.avatar_persona_original_images_enabled
            .store(enabled, Ordering::Relaxed);
    }

    pub fn serve(
        &self,
        request: &Request<Vec<u8>>,
        delivery: HostResourceDeliveryCapabilities,
    ) -> HostResourceResponse {
        self.try_serve(request, delivery).unwrap_or_else(|| {
            finalize_response(request, response::error(StatusCode::NOT_FOUND, "Not Found"))
        })
    }
}

fn finalize_response(
    request: &Request<Vec<u8>>,
    mut response: HostResourceResponse,
) -> HostResourceResponse {
    if request.method() == Method::HEAD {
        response.body_mut().clear();
    }
    response.headers_mut().insert(
        TAURITAVERN_TRACE_ID,
        HeaderValue::from_str(&next_trace_id()).expect("controlled trace id"),
    );
    response
}

fn next_trace_id() -> String {
    let sequence = NEXT_TRACE_SEQUENCE.fetch_add(1, Ordering::Relaxed) + 1;
    format!("hr-{sequence}")
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::sync::atomic::AtomicUsize;

    use http::{Method, StatusCode};
    use tt_ports::host_resource::{
        HostResourceSourceRequest, HostResourceStoreError, OpenedHostResource,
        ThumbnailAssetRequest, ThumbnailKind, ThumbnailSelection,
    };

    use super::*;
    use crate::services::host_resource_service::test_support;

    #[derive(Default)]
    struct Store {
        thumbnail_requests: Mutex<Vec<ThumbnailAssetRequest>>,
    }

    impl HostResourceAssetStore for Store {
        fn open(
            &self,
            request: HostResourceSourceRequest<'_>,
        ) -> Result<OpenedHostResource, HostResourceStoreError> {
            let (bytes, content_type) = match request {
                HostResourceSourceRequest::UserCss => (b"css".as_slice(), "text/css"),
                HostResourceSourceRequest::ThirdParty { .. } => {
                    (b"third".as_slice(), "application/javascript")
                }
                HostResourceSourceRequest::UserData { .. } => (b"data".as_slice(), "image/png"),
                HostResourceSourceRequest::Thumbnail(request) => {
                    self.thumbnail_requests
                        .lock()
                        .expect("lock")
                        .push(request.clone());
                    (b"thumbnail".as_slice(), "image/jpeg")
                }
            };
            Ok(test_support::opened(
                bytes,
                content_type,
                Arc::new(AtomicUsize::new(0)),
            ))
        }
    }

    #[test]
    fn facade_dispatches_known_routes_and_ignores_frontend_assets() {
        let service = HostResourceService::new(false, Arc::new(Store::default()));
        let delivery = HostResourceDeliveryCapabilities::new(true, false);
        let user_css = test_support::request(Method::GET, "/css/user.css");
        let third_party =
            test_support::request(Method::GET, "/scripts/extensions/third-party/mobile/app.js");
        let user_data = test_support::request(Method::GET, "/backgrounds/a.png");
        let frontend = test_support::request(Method::GET, "/index.html");

        let user_css_response = service.try_serve(&user_css, delivery).expect("served");
        let third_party_response = service.try_serve(&third_party, delivery).expect("served");
        let user_data_response = service.try_serve(&user_data, delivery).expect("served");

        assert_eq!(user_css_response.body(), b"css");
        assert_eq!(third_party_response.body(), b"third");
        assert_eq!(user_data_response.body(), b"data");
        assert!(
            user_css_response.headers()[&TAURITAVERN_TRACE_ID]
                .to_str()
                .ok()
                .is_some_and(|value| value.starts_with("hr-"))
        );
        assert_ne!(
            user_css_response.headers()[&TAURITAVERN_TRACE_ID],
            third_party_response.headers()[&TAURITAVERN_TRACE_ID]
        );
        assert!(service.try_serve(&frontend, delivery).is_none());

        let fallback = service.serve(&frontend, delivery);
        assert_eq!(fallback.status(), StatusCode::NOT_FOUND);
        assert!(fallback.headers().contains_key(&TAURITAVERN_TRACE_ID));
    }

    #[test]
    fn runtime_thumbnail_setting_updates_subsequent_requests() {
        let store = Arc::new(Store::default());
        let service = HostResourceService::new(false, Arc::clone(&store));
        let avatar = Request::builder()
            .method(Method::GET)
            .uri("/thumbnail?type=avatar&file=a.png")
            .body(Vec::new())
            .expect("request");
        let delivery = HostResourceDeliveryCapabilities::new(true, false);

        service.try_serve(&avatar, delivery).expect("served");
        service.set_avatar_persona_original_images_enabled(true);
        service.try_serve(&avatar, delivery).expect("served");

        assert_eq!(
            store.thumbnail_requests.lock().expect("lock").as_slice(),
            &[
                ThumbnailAssetRequest {
                    kind: ThumbnailKind::Avatar,
                    file: "a.png".to_string(),
                    selection: ThumbnailSelection::PreferGenerated,
                },
                ThumbnailAssetRequest {
                    kind: ThumbnailKind::Avatar,
                    file: "a.png".to_string(),
                    selection: ThumbnailSelection::Original,
                },
            ]
        );
    }

    #[test]
    fn head_never_returns_an_error_body() {
        let service = HostResourceService::new(false, Arc::new(Store::default()));
        let delivery = HostResourceDeliveryCapabilities::new(true, false);
        let invalid_thumbnail = test_support::request(Method::HEAD, "/thumbnail");
        let unknown = test_support::request(Method::HEAD, "/index.html");

        let invalid_thumbnail = service
            .try_serve(&invalid_thumbnail, delivery)
            .expect("handled thumbnail route");
        let unknown = service.serve(&unknown, delivery);

        assert_eq!(invalid_thumbnail.status(), StatusCode::BAD_REQUEST);
        assert!(invalid_thumbnail.body().is_empty());
        assert_eq!(
            invalid_thumbnail.headers()[http::header::CONTENT_LENGTH],
            "22"
        );
        assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
        assert!(unknown.body().is_empty());
        assert_eq!(unknown.headers()[http::header::CONTENT_LENGTH], "9");
    }
}
