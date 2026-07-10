use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use headers::{Date, ETag, HeaderMapExt, IfModifiedSince, IfNoneMatch, IfRange, LastModified};
use http::header::{
    ACCEPT_RANGES, ALLOW, CACHE_CONTROL, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, IF_NONE_MATCH,
};
use http::{HeaderValue, Method, Request, Response, StatusCode};
use sha2::{Digest, Sha256};
use tt_ports::host_resource::{HostResourceSourceMetadata, HostResourceStoreError};

use super::HostResourceDeliveryCapabilities;

const PRIVATE_REVALIDATE: HeaderValue = HeaderValue::from_static("private, no-cache");
const NO_STORE: HeaderValue = HeaderValue::from_static("no-store");
const HTTP_DATE_YEAR_10000_SECS: u64 = 253_402_300_800;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HostResourceCachePolicy {
    NoStore,
    PrivateRevalidate,
}

impl HostResourceCachePolicy {
    const fn header_value(self) -> HeaderValue {
        match self {
            Self::NoStore => NO_STORE,
            Self::PrivateRevalidate => PRIVATE_REVALIDATE,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct RepresentationMetadata {
    content_type: HeaderValue,
    content_length: Option<u64>,
    etag: ETag,
    last_modified: Option<SystemTime>,
    response_date: SystemTime,
}

impl RepresentationMetadata {
    pub(super) fn raw(
        source: &HostResourceSourceMetadata,
        content_type: Option<&str>,
    ) -> Result<Self, HostResourceStoreError> {
        Self::new(
            source,
            b"raw",
            content_type.unwrap_or(&source.content_type),
            Some(source.content_length),
            true,
        )
    }

    pub(super) fn derived(
        source: &HostResourceSourceMetadata,
        variant: &[u8],
    ) -> Result<Self, HostResourceStoreError> {
        Self::new(source, variant, &source.content_type, None, false)
    }

    fn new(
        source: &HostResourceSourceMetadata,
        variant: &[u8],
        content_type: &str,
        content_length: Option<u64>,
        include_last_modified: bool,
    ) -> Result<Self, HostResourceStoreError> {
        let content_type = HeaderValue::from_str(content_type).map_err(|error| {
            HostResourceStoreError::internal(format!("Invalid host resource content type: {error}"))
        })?;
        let etag = representation_etag(source, variant, &content_type)?;
        let response_date = SystemTime::now();
        let last_modified = include_last_modified
            .then_some(source.last_modified.min(response_date))
            .filter(|value| {
                value
                    .duration_since(UNIX_EPOCH)
                    .is_ok_and(|duration| duration.as_secs() < HTTP_DATE_YEAR_10000_SECS)
            });

        Ok(Self {
            content_type,
            content_length,
            etag,
            last_modified,
            response_date,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RetrievalDecision {
    NotModified,
    Head,
    Full,
    Continue,
}

pub(super) fn decide_retrieval(
    request: &Request<Vec<u8>>,
    metadata: &RepresentationMetadata,
    delivery: HostResourceDeliveryCapabilities,
) -> RetrievalDecision {
    if request_is_not_modified(request, metadata) {
        if delivery.supports_not_modified() {
            return RetrievalDecision::NotModified;
        }
        return if request.method() == Method::HEAD {
            RetrievalDecision::Head
        } else {
            RetrievalDecision::Full
        };
    }

    if request.method() == Method::HEAD {
        RetrievalDecision::Head
    } else {
        RetrievalDecision::Continue
    }
}

pub(super) fn if_range_allows_range(
    request: &Request<Vec<u8>>,
    metadata: &RepresentationMetadata,
) -> bool {
    if !request.headers().contains_key(http::header::IF_RANGE) {
        return true;
    }
    let Some(if_range) = request.headers().typed_get::<IfRange>() else {
        return true;
    };

    !if_range.is_modified(Some(&metadata.etag), None)
}

fn request_is_not_modified(request: &Request<Vec<u8>>, metadata: &RepresentationMetadata) -> bool {
    if request.headers().contains_key(IF_NONE_MATCH) {
        return request
            .headers()
            .typed_get::<IfNoneMatch>()
            .is_some_and(|condition| !condition.precondition_passes(&metadata.etag));
    }

    let Some(last_modified) = metadata.last_modified else {
        return false;
    };
    request
        .headers()
        .typed_get::<IfModifiedSince>()
        .is_some_and(|condition| !condition.is_modified(last_modified))
}

fn representation_etag(
    source: &HostResourceSourceMetadata,
    variant: &[u8],
    content_type: &HeaderValue,
) -> Result<ETag, HostResourceStoreError> {
    let mut hasher = Sha256::new();
    hasher.update(b"host-representation-v1\0");
    hasher.update(source.revision.as_bytes());
    hasher.update([0]);
    hasher.update(variant);
    hasher.update([0]);
    hasher.update(content_type.as_bytes());
    let opaque = URL_SAFE_NO_PAD.encode(hasher.finalize());
    format!("W/\"{opaque}\"").parse().map_err(|error| {
        HostResourceStoreError::internal(format!("Failed to build host resource ETag: {error}"))
    })
}

pub(super) fn not_modified(metadata: &RepresentationMetadata) -> Response<Vec<u8>> {
    representation_response(StatusCode::NOT_MODIFIED, metadata, Vec::new(), None, false)
}

pub(super) fn head(metadata: &RepresentationMetadata) -> Response<Vec<u8>> {
    representation_response(
        StatusCode::OK,
        metadata,
        Vec::new(),
        metadata.content_length,
        true,
    )
}

pub(super) fn ok(metadata: &RepresentationMetadata, body: Vec<u8>) -> Response<Vec<u8>> {
    let content_length = body.len() as u64;
    representation_response(StatusCode::OK, metadata, body, Some(content_length), true)
}

pub(super) fn partial(
    metadata: &RepresentationMetadata,
    body: Vec<u8>,
    range_value: String,
    declared_length: u64,
) -> Response<Vec<u8>> {
    let mut response = representation_response(
        StatusCode::PARTIAL_CONTENT,
        metadata,
        body,
        Some(declared_length),
        true,
    );
    insert_header_value(response.headers_mut(), CONTENT_RANGE, range_value);
    response
}

fn representation_response(
    status: StatusCode,
    metadata: &RepresentationMetadata,
    body: Vec<u8>,
    content_length: Option<u64>,
    include_content_headers: bool,
) -> Response<Vec<u8>> {
    let mut response = Response::new(body);
    *response.status_mut() = status;
    response.headers_mut().insert(
        CACHE_CONTROL,
        HostResourceCachePolicy::PrivateRevalidate.header_value(),
    );
    response
        .headers_mut()
        .typed_insert(Date::from(metadata.response_date));
    response.headers_mut().typed_insert(metadata.etag.clone());
    if let Some(last_modified) = metadata.last_modified {
        response
            .headers_mut()
            .typed_insert(LastModified::from(last_modified));
    }

    if include_content_headers {
        response
            .headers_mut()
            .insert(CONTENT_TYPE, metadata.content_type.clone());
        if let Some(content_length) = content_length {
            insert_header_value(response.headers_mut(), CONTENT_LENGTH, content_length);
        }
    }
    response
}

pub(super) fn no_content(allowed_methods: &'static str) -> Response<Vec<u8>> {
    let mut response = base_response(StatusCode::NO_CONTENT, Vec::new());
    response
        .headers_mut()
        .insert(ALLOW, HeaderValue::from_static(allowed_methods));
    response
}

pub(super) fn method_not_allowed(allowed_methods: &'static str) -> Response<Vec<u8>> {
    let mut response = error(StatusCode::METHOD_NOT_ALLOWED, "Method not allowed");
    response
        .headers_mut()
        .insert(ALLOW, HeaderValue::from_static(allowed_methods));
    response
}

pub(super) fn error(status: StatusCode, message: &str) -> Response<Vec<u8>> {
    let body = message.as_bytes().to_vec();
    let mut response = base_response(status, body);
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    insert_header_value(response.headers_mut(), CONTENT_LENGTH, message.len());
    response
}

fn base_response(status: StatusCode, body: Vec<u8>) -> Response<Vec<u8>> {
    let mut response = Response::new(body);
    *response.status_mut() = status;
    response.headers_mut().insert(
        CACHE_CONTROL,
        HostResourceCachePolicy::NoStore.header_value(),
    );
    response
        .headers_mut()
        .typed_insert(Date::from(SystemTime::now()));
    response
}

pub(super) fn store_error(
    error_value: HostResourceStoreError,
    not_found_message: &'static str,
) -> Response<Vec<u8>> {
    match error_value {
        HostResourceStoreError::NotFound(_) => error(StatusCode::NOT_FOUND, not_found_message),
        HostResourceStoreError::Forbidden(message) => error(StatusCode::FORBIDDEN, &message),
        HostResourceStoreError::Internal(message) => {
            error(StatusCode::INTERNAL_SERVER_ERROR, &message)
        }
    }
}

pub(super) fn with_accept_ranges(mut response: Response<Vec<u8>>) -> Response<Vec<u8>> {
    response
        .headers_mut()
        .insert(ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    response
}

pub(super) fn range_not_satisfiable(message: &str, total_size: u64) -> Response<Vec<u8>> {
    let mut response = error(StatusCode::RANGE_NOT_SATISFIABLE, message);
    insert_header_value(
        response.headers_mut(),
        CONTENT_RANGE,
        format!("bytes */{total_size}"),
    );
    with_accept_ranges(response)
}

fn insert_header_value(
    headers: &mut http::HeaderMap,
    name: http::header::HeaderName,
    value: impl ToString,
) {
    headers.insert(
        name,
        HeaderValue::from_str(&value.to_string()).expect("controlled HTTP header value"),
    );
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use http::header::{CACHE_CONTROL, ETAG, LAST_MODIFIED};
    use tt_ports::host_resource::HostResourceSourceRevision;

    use super::*;

    fn source() -> HostResourceSourceMetadata {
        HostResourceSourceMetadata {
            content_type: "image/png".to_string(),
            content_length: 4,
            last_modified: SystemTime::now() - Duration::from_secs(60),
            revision: HostResourceSourceRevision::new(b"revision".to_vec()),
        }
    }

    fn request(headers: &[(&str, &str)]) -> Request<Vec<u8>> {
        let mut builder = Request::builder()
            .method(Method::GET)
            .uri("/characters/a.png");
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        builder.body(Vec::new()).expect("request")
    }

    #[test]
    fn matching_weak_etag_respects_delivery_capability() {
        let metadata = RepresentationMetadata::raw(&source(), None).expect("metadata");
        let first = ok(&metadata, b"data".to_vec());
        let etag = first.headers()[ETAG].to_str().expect("etag");
        let request = request(&[("if-none-match", etag)]);

        assert_eq!(
            decide_retrieval(
                &request,
                &metadata,
                HostResourceDeliveryCapabilities::new(true, false)
            ),
            RetrievalDecision::NotModified
        );
        assert_eq!(
            decide_retrieval(
                &request,
                &metadata,
                HostResourceDeliveryCapabilities::new(false, false)
            ),
            RetrievalDecision::Full
        );
    }

    #[test]
    fn wildcard_and_tag_list_use_weak_if_none_match_comparison() {
        let metadata = RepresentationMetadata::raw(&source(), None).expect("metadata");
        let response = ok(&metadata, b"data".to_vec());
        let etag = response.headers()[ETAG].to_str().expect("etag");
        let list = format!("\"other\", {etag}");

        for condition in ["*", list.as_str()] {
            assert_eq!(
                decide_retrieval(
                    &request(&[("if-none-match", condition)]),
                    &metadata,
                    HostResourceDeliveryCapabilities::new(true, false)
                ),
                RetrievalDecision::NotModified
            );
        }
    }

    #[test]
    fn last_modified_round_trips_through_if_modified_since() {
        let metadata = RepresentationMetadata::raw(&source(), None).expect("metadata");
        let response = ok(&metadata, b"data".to_vec());
        let last_modified = response.headers()[LAST_MODIFIED]
            .to_str()
            .expect("last modified");

        assert_eq!(
            decide_retrieval(
                &request(&[("if-modified-since", last_modified)]),
                &metadata,
                HostResourceDeliveryCapabilities::new(true, false)
            ),
            RetrievalDecision::NotModified
        );
    }

    #[test]
    fn pre_epoch_source_omits_unrepresentable_last_modified() {
        let mut source = source();
        source.last_modified = UNIX_EPOCH - Duration::from_secs(1);
        let metadata = RepresentationMetadata::raw(&source, None).expect("metadata");
        let response = ok(&metadata, b"data".to_vec());

        assert!(!response.headers().contains_key(LAST_MODIFIED));
    }

    #[test]
    fn malformed_if_none_match_suppresses_if_modified_since() {
        let metadata = RepresentationMetadata::raw(&source(), None).expect("metadata");
        let mut request = request(&[("if-none-match", "invalid")]);
        request
            .headers_mut()
            .typed_insert(IfModifiedSince::from(SystemTime::now()));

        assert_eq!(
            decide_retrieval(
                &request,
                &metadata,
                HostResourceDeliveryCapabilities::new(true, false)
            ),
            RetrievalDecision::Continue
        );
    }

    #[test]
    fn not_modified_response_has_only_cache_metadata() {
        let metadata = RepresentationMetadata::raw(&source(), None).expect("metadata");
        let response = not_modified(&metadata);

        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
        assert!(response.body().is_empty());
        assert_eq!(response.headers()[CACHE_CONTROL], "private, no-cache");
        assert!(response.headers().contains_key(ETAG));
        assert!(!response.headers().contains_key(CONTENT_TYPE));
        assert!(!response.headers().contains_key(CONTENT_LENGTH));
        assert!(!response.headers().contains_key(CONTENT_RANGE));
    }
}
