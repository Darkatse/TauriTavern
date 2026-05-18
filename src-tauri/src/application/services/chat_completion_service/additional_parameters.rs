use std::collections::HashMap;

use serde_json::{Map, Value};

use crate::application::errors::ApplicationError;

use super::custom_parameters;

#[derive(Debug, Clone, Default)]
pub(super) struct AdditionalParameters {
    include_body: String,
    exclude_body: String,
    include_headers: String,
}

impl AdditionalParameters {
    pub(super) fn from_payload(payload: &Map<String, Value>) -> Result<Self, ApplicationError> {
        Ok(Self {
            include_body: optional_string(payload, "custom_include_body")?,
            exclude_body: optional_string(payload, "custom_exclude_body")?,
            include_headers: optional_string(payload, "custom_include_headers")?,
        })
    }

    pub(super) fn from_status_headers(include_headers: &str) -> Self {
        Self {
            include_headers: include_headers.to_string(),
            ..Self::default()
        }
    }

    pub(super) fn headers(&self) -> Result<HashMap<String, String>, ApplicationError> {
        custom_parameters::parse_string_map(&self.include_headers)
    }

    pub(super) fn apply_body_overrides(
        &self,
        upstream_payload: &mut Value,
    ) -> Result<(), ApplicationError> {
        let Some(body) = upstream_payload.as_object_mut() else {
            return Err(ApplicationError::InternalError(
                "Chat completion upstream payload must be an object".to_string(),
            ));
        };

        if !self.include_body.trim().is_empty() {
            let include_map = custom_parameters::parse_object(&self.include_body)?;
            for (key, value) in include_map {
                body.insert(key, value);
            }
        }

        for key in custom_parameters::parse_key_list(&self.exclude_body)? {
            body.remove(&key);
        }

        Ok(())
    }
}

fn optional_string(payload: &Map<String, Value>, key: &str) -> Result<String, ApplicationError> {
    let Some(value) = payload.get(key) else {
        return Ok(String::new());
    };

    value.as_str().map(str::to_string).ok_or_else(|| {
        ApplicationError::ValidationError(format!(
            "Chat completion request field must be a string: {}",
            key
        ))
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::AdditionalParameters;

    #[test]
    fn final_body_overrides_apply_after_payload_construction() {
        let payload = json!({
            "custom_include_body": "temperature: 0.7\nmetadata: { feature: override }",
            "custom_exclude_body": "- stream"
        })
        .as_object()
        .cloned()
        .expect("payload must be an object");
        let parameters = AdditionalParameters::from_payload(&payload).expect("parameters parse");
        let mut upstream_payload = json!({
            "model": "gpt-4.1-mini",
            "temperature": 0.1,
            "stream": true
        });

        parameters
            .apply_body_overrides(&mut upstream_payload)
            .expect("overrides should apply");

        assert_eq!(upstream_payload["temperature"], json!(0.7));
        assert_eq!(upstream_payload["metadata"]["feature"], json!("override"));
        assert!(upstream_payload.get("stream").is_none());
    }

    #[test]
    fn non_string_payload_fields_fail_fast() {
        let payload = json!({
            "custom_include_body": { "temperature": 0.7 }
        })
        .as_object()
        .cloned()
        .expect("payload must be an object");

        let error =
            AdditionalParameters::from_payload(&payload).expect_err("field type should fail");

        assert!(error.to_string().contains("custom_include_body"));
    }
}
