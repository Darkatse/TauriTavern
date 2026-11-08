use std::collections::HashMap;

use serde::Deserialize;
use serde::de::IgnoredAny;
use serde_json::value::RawValue;
use tt_contracts::world_info::WorldInfoListItem;
use tt_domain::errors::DomainError;

/// A validated document borrowing its original representation.
pub(super) struct Document<'a> {
    bytes: &'a [u8],
}

impl<'a> Document<'a> {
    pub(super) fn parse(bytes: &'a [u8]) -> Result<Self, DomainError> {
        let text = std::str::from_utf8(bytes).map_err(|error| {
            DomainError::InvalidData(format!("Invalid world info UTF-8: {error}"))
        })?;
        let fields: HashMap<String, IgnoredAny> = serde_json::from_str(text).map_err(json_error)?;
        if !fields.contains_key("entries") {
            return Err(invalid_document());
        }
        Ok(Self { bytes })
    }

    pub(super) fn bytes(&self) -> &'a [u8] {
        self.bytes
    }
}

pub(super) fn replacement(bytes: &[u8]) -> Result<(String, Document<'_>), DomainError> {
    // JSON object semantics: the last occurrence of each field wins, even if an
    // earlier name or data value had the wrong type. Opaque values stay borrowed.
    let fields: HashMap<String, &RawValue> = serde_json::from_slice(bytes).map_err(json_error)?;
    let name = fields
        .get("name")
        .and_then(|value| serde_json::from_str::<String>(value.get()).ok())
        .filter(|name| !name.is_empty())
        .ok_or_else(|| DomainError::InvalidData("World file must have a name".into()))?;
    let data = fields.get("data").ok_or_else(invalid_document)?;
    Ok((name, Document::parse(data.get().as_bytes())?))
}

pub(super) fn list_item(file_id: &str, bytes: &[u8]) -> WorldInfoListItem {
    // Capturing only these fields leaves the others, entries included, undecoded.
    #[derive(Default, Deserialize)]
    struct Fields<'a> {
        #[serde(borrow)]
        name: Option<&'a RawValue>,
        #[serde(borrow)]
        extensions: Option<&'a RawValue>,
    }
    let fields: Fields = serde_json::from_slice(bytes).unwrap_or_default();
    let name = fields
        .name
        .and_then(|value| serde_json::from_str::<String>(value.get()).ok())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| file_id.to_owned());
    let extensions = fields
        .extensions
        .filter(|value| value.get().starts_with(['{', '[']))
        .map_or_else(
            || RawValue::from_string("{}".to_owned()).expect("`{}` is JSON"),
            RawValue::to_owned,
        );
    WorldInfoListItem {
        file_id: file_id.to_owned(),
        name,
        extensions,
    }
}

fn invalid_document() -> DomainError {
    DomainError::InvalidData("World info must be an object containing entries".into())
}

fn json_error(error: serde_json::Error) -> DomainError {
    DomainError::InvalidData(format!("Invalid world info JSON: {error}"))
}
