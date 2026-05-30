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

    pub(super) fn ensure_body_overrides_do_not_touch(
        &self,
        protected_keys: &[&str],
    ) -> Result<(), ApplicationError> {
        if protected_keys.is_empty() {
            return Ok(());
        }

        if !self.include_body.trim().is_empty() {
            for key in custom_parameters::parse_object(&self.include_body)?.keys() {
                if protected_keys.contains(&key.as_str()) {
                    return Err(protected_body_override_error(key));
                }
            }
        }

        for key in custom_parameters::parse_key_list(&self.exclude_body)? {
            if protected_keys.contains(&key.as_str()) {
                return Err(protected_body_override_error(&key));
            }
        }

        Ok(())
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

    // Treat JSON null the same as a missing field. Frontend presets and
    // third-party extensions (e.g. per-chat API routers) can persist `null`
    // into `custom_include_body` / `custom_exclude_body` / `custom_include_headers`
    // when a slot was cleared, and our `/api/backends/chat-completions/generate`
    // route forwards the body verbatim. Without this guard the upstream value
    // surfaces as a confusing "must be a string" validation error even though
    // the field is semantically absent, mirroring serde's `Option<String>`
    // behaviour for nullable fields.
    if value.is_null() {
        return Ok(String::new());
    }

    value.as_str().map(str::to_string).ok_or_else(|| {
        ApplicationError::ValidationError(format!(
            "Chat completion request field must be a string: {}",
            key
        ))
    })
}

fn protected_body_override_error(key: &str) -> ApplicationError {
    ApplicationError::ValidationError(format!(
        "Chat completion body override cannot modify protected field: {key}"
    ))
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

    #[test]
    fn null_payload_fields_are_treated_as_missing() {
        // Stale presets and third-party extensions sometimes persist a literal
        // `null` for these slots; the HTTP boundary should accept that the same
        // way a missing field would be accepted, instead of bubbling up a
        // confusing "must be a string" validation error to the user.
        let payload = json!({
            "custom_include_body": null,
            "custom_exclude_body": null,
            "custom_include_headers": null,
        })
        .as_object()
        .cloned()
        .expect("payload must be an object");

        let parameters = AdditionalParameters::from_payload(&payload)
            .expect("null fields should be tolerated, not rejected as non-string");

        // Empty defaults should remain after construction.
        parameters
            .ensure_body_overrides_do_not_touch(&["messages", "tools"])
            .expect("absent overrides should never trip the protected-field guard");

        let headers = parameters.headers().expect("headers should parse cleanly");
        assert!(
            headers.is_empty(),
            "null include_headers must not produce any header entries",
        );
    }

    #[test]
    fn protected_body_overrides_reject_include_keys() {
        let payload = json!({
            "custom_include_body": "{\"messages\":[{\"role\":\"user\",\"content\":\"override\"}]}"
        })
        .as_object()
        .cloned()
        .expect("payload must be an object");
        let parameters = AdditionalParameters::from_payload(&payload).expect("parameters parse");

        let error = parameters
            .ensure_body_overrides_do_not_touch(&["messages"])
            .expect_err("protected include key should fail");

        assert!(error.to_string().contains("protected field: messages"));
    }

    #[test]
    fn protected_body_overrides_reject_exclude_keys() {
        let payload = json!({
            "custom_exclude_body": "[\"tools\"]"
        })
        .as_object()
        .cloned()
        .expect("payload must be an object");
        let parameters = AdditionalParameters::from_payload(&payload).expect("parameters parse");

        let error = parameters
            .ensure_body_overrides_do_not_touch(&["tools"])
            .expect_err("protected exclude key should fail");

        assert!(error.to_string().contains("protected field: tools"));
    }

    #[test]
    fn protected_body_overrides_allow_unrelated_keys() {
        let payload = json!({
            "custom_include_body": "{ \"metadata\": { \"feature\": \"test\" } }",
            "custom_exclude_body": "[\"temperature\"]"
        })
        .as_object()
        .cloned()
        .expect("payload must be an object");
        let parameters = AdditionalParameters::from_payload(&payload).expect("parameters parse");

        parameters
            .ensure_body_overrides_do_not_touch(&["messages", "tools"])
            .expect("unrelated overrides should pass");
    }
}
