//! What the log keeps of a request. Recording and the request preview both start here, so a
//! previewed request reads exactly like the logged one.

use std::borrow::Cow;

use serde_json::Value;
use tt_contracts::observability::{
    ChatCompletionRequestPreviewDto, ExchangeViewDto, RequestDescriptionDto, RequestKindDto,
    RequestViewDto,
};
use tt_ports::repositories::chat_completion_repository::ChatCompletionSource;

use super::readable::{
    extract_model, format_endpoint, pretty_json, project_request_view, render_request_readable,
    wire_log_payload,
};

pub(super) struct LoggedRequest<'a> {
    pub source: String,
    pub endpoint: String,
    pub model: Option<String>,
    pub body: Cow<'a, Value>,
    /// Absent when request construction fails.
    pub http: Option<RequestDescriptionDto>,
}

impl<'a> LoggedRequest<'a> {
    pub(super) fn new(
        source: String,
        base_url: &str,
        endpoint_path: &str,
        payload: &Value,
        body: Cow<'a, Value>,
        http: Option<RequestDescriptionDto>,
    ) -> Self {
        Self {
            source,
            endpoint: format_endpoint(base_url, endpoint_path),
            model: extract_model(payload),
            body: wire_log_payload(body),
            http,
        }
    }

    pub(super) fn raw(&self) -> String {
        pretty_json(&self.body)
    }
}

pub(super) fn request_view(
    kind: RequestKindDto,
    payload: &Value,
    http: Option<RequestDescriptionDto>,
) -> RequestViewDto {
    RequestViewDto {
        http,
        ..project_request_view(kind, payload)
    }
}

pub fn preview_chat_completion_request(
    source: ChatCompletionSource,
    base_url: &str,
    endpoint_path: &str,
    payload: &Value,
    body: &Value,
    http: RequestDescriptionDto,
) -> ChatCompletionRequestPreviewDto {
    let request = LoggedRequest::new(
        source.key().to_string(),
        base_url,
        endpoint_path,
        payload,
        Cow::Borrowed(body),
        Some(http),
    );
    let request_raw = request.raw();
    let view = request_view(RequestKindDto::Chat, &request.body, request.http);
    ChatCompletionRequestPreviewDto {
        source: request.source,
        model: request.model,
        endpoint: request.endpoint,
        request_raw,
        request_readable: render_request_readable(&view),
        view: ExchangeViewDto {
            request: view,
            response: None,
        },
    }
}
