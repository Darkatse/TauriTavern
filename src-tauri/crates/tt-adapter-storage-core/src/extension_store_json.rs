//! The JSON entry rules shared by the extension stores.
use std::fmt;
use std::io::Read;

use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use tt_domain::json_document::JsonDocument;

pub(crate) fn validate(json: impl Read) -> serde_json::Result<()> {
    serde_json::from_reader::<_, JsonValue>(json).map(|_| ())
}

/// Two objects merge; otherwise the patch replaces the entry.
pub(crate) fn merge(entry: Option<&[u8]>, patch: &[u8]) -> serde_json::Result<Vec<u8>> {
    match entry {
        Some(entry) if is_object(entry) && is_object(patch) => {
            let mut merged = JsonDocument::parse(entry)?;
            merged.merge(JsonDocument::parse(patch)?)?;
            Ok(merged.to_vec())
        }
        _ => Ok(patch.to_vec()),
    }
}

fn is_object(json: &[u8]) -> bool {
    json.trim_ascii_start().first() == Some(&b'{')
}

/// Use serde's normal string, number and depth checks without retaining a Value tree.
/// IgnoredAny skips some of those checks; a large string can still occupy serde's scratch buffer.
struct JsonValue;

impl<'de> Deserialize<'de> for JsonValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(JsonVisitor)
    }
}

struct JsonVisitor;

impl<'de> Visitor<'de> for JsonVisitor {
    type Value = JsonValue;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_unit<E>(self) -> Result<JsonValue, E> {
        Ok(JsonValue)
    }

    fn visit_bool<E>(self, _: bool) -> Result<JsonValue, E> {
        Ok(JsonValue)
    }

    fn visit_i64<E>(self, _: i64) -> Result<JsonValue, E> {
        Ok(JsonValue)
    }

    fn visit_u64<E>(self, _: u64) -> Result<JsonValue, E> {
        Ok(JsonValue)
    }

    fn visit_f64<E>(self, _: f64) -> Result<JsonValue, E> {
        Ok(JsonValue)
    }

    fn visit_str<E>(self, _: &str) -> Result<JsonValue, E> {
        Ok(JsonValue)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<JsonValue, A::Error> {
        while sequence.next_element::<JsonValue>()?.is_some() {}
        Ok(JsonValue)
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<JsonValue, A::Error> {
        while map.next_entry::<JsonValue, JsonValue>()?.is_some() {}
        Ok(JsonValue)
    }
}
