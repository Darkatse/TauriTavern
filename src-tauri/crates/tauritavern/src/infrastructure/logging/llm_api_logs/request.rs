use std::borrow::Cow;

use serde_json::Value;

use super::readable::{extract_model, format_endpoint, pretty_json, wire_log_payload};

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
