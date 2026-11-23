//! What the log keeps of a request. Recording and the request preview both start here, so a
//! previewed request reads exactly like the logged one.

use std::borrow::Cow;

use serde_json::Value;
use tt_contracts::observability::{
    ChatCompletionRequestPreviewDto, ExchangeViewDto, RequestKindDto,
};
use tt_ports::repositories::chat_completion_repository::ChatCompletionSource;

use super::readable::{
    extract_model, format_endpoint, pretty_json, project_request_view, wire_log_payload,
};

pub(super) struct LoggedRequest<'a> {
    pub source: String,
    pub endpoint: String,
    pub model: Option<String>,
    pub payload: Cow<'a, Value>,
}

impl<'a> LoggedRequest<'a> {
    pub(super) fn new(
        source: String,
        base_url: &str,
        endpoint_path: &str,
        payload: &'a Value,
    ) -> Self {
        let payload = wire_log_payload(payload);
        Self {
            source,
            endpoint: format_endpoint(base_url, endpoint_path),
            model: extract_model(&payload),
            payload,
        }
    }

    pub(super) fn raw(&self) -> String {
        pretty_json(&self.payload)
    }
}

pub fn preview_chat_completion_request(
    source: ChatCompletionSource,
    base_url: &str,
    endpoint_path: &str,
    payload: &Value,
) -> ChatCompletionRequestPreviewDto {
    let request = LoggedRequest::new(source.key().to_string(), base_url, endpoint_path, payload);
    let request_raw = request.raw();
    ChatCompletionRequestPreviewDto {
        source: request.source,
        model: request.model,
        endpoint: request.endpoint,
        request_raw,
        view: ExchangeViewDto {
            request: project_request_view(RequestKindDto::Chat, &request.payload),
            response: None,
        },
    }
}
