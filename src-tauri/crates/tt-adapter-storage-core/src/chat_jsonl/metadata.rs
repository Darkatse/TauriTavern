use serde::Deserialize;
use serde::de::{IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde_json::{Value, value::RawValue};

use super::validate_integrity;

/// Header metadata interpreted by identity and directory queries. Unknown
/// extension fields are consumed without constructing their object trees.
#[derive(Debug, Default)]
pub(crate) struct HeaderMetadata {
    chat_id: Option<Box<RawValue>>,
    pub(crate) integrity: Option<String>,
}

impl HeaderMetadata {
    // Identity readers do not interpret chat_id_hash. In particular, they must
    // not acquire the directory query's JSON number representation constraints.
    pub(crate) fn chat_id(&self) -> serde_json::Result<Option<String>> {
        let Some(raw) = self.chat_id.as_ref() else {
            return Ok(None);
        };
        if !matches!(
            raw.get().as_bytes().first(),
            Some(b'"' | b'-' | b'0'..=b'9')
        ) {
            return Ok(None);
        }
        let value: Value = serde_json::from_str(raw.get())?;
        Ok(value
            .as_u64()
            .map(|n| n.to_string())
            .or_else(|| value.as_i64().map(|n| n.to_string()))
            .or_else(|| value.as_str().map(str::to_owned)))
    }
}

impl<'de> Deserialize<'de> for HeaderMetadata {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        HeaderMetadataFields::deserialize(deserializer)?
            .finish()
            .map_err(serde::de::Error::custom)
    }
}

/// Validation waits until the containing header has selected its final
/// chat_metadata field. An earlier duplicate may contain an invalid integrity.
#[derive(Debug, Default)]
pub(crate) struct HeaderMetadataFields {
    chat_id: Option<Box<RawValue>>,
    integrity: Option<Value>,
}

impl HeaderMetadataFields {
    pub(crate) fn finish(self) -> Result<HeaderMetadata, &'static str> {
        let integrity = self
            .integrity
            .as_ref()
            .map(validate_integrity)
            .transpose()?
            .map(str::to_owned);
        Ok(HeaderMetadata {
            chat_id: self.chat_id,
            integrity,
        })
    }
}

impl<'de> Deserialize<'de> for HeaderMetadataFields {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(Self::default())
    }
}

impl<'de> Visitor<'de> for HeaderMetadataFields {
    type Value = Self;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("chat metadata")
    }

    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self, M::Error> {
        #[derive(Deserialize)]
        #[serde(field_identifier, rename_all = "snake_case")]
        enum Field {
            ChatIdHash,
            Integrity,
            #[serde(other)]
            Other,
        }

        let mut chat_id = None;
        let mut integrity = None;
        while let Some(field) = map.next_key()? {
            match field {
                Field::ChatIdHash => chat_id = Some(map.next_value()?),
                Field::Integrity => integrity = Some(map.next_value::<Value>()?),
                Field::Other => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(Self { chat_id, integrity })
    }

    fn visit_seq<S: SeqAccess<'de>>(self, mut seq: S) -> Result<Self, S::Error> {
        while seq.next_element::<IgnoredAny>()?.is_some() {}
        Ok(self)
    }

    fn visit_unit<E: serde::de::Error>(self) -> Result<Self, E> {
        Ok(self)
    }
    fn visit_bool<E: serde::de::Error>(self, _: bool) -> Result<Self, E> {
        Ok(self)
    }
    fn visit_i64<E: serde::de::Error>(self, _: i64) -> Result<Self, E> {
        Ok(self)
    }
    fn visit_u64<E: serde::de::Error>(self, _: u64) -> Result<Self, E> {
        Ok(self)
    }
    fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<Self, E> {
        Ok(self)
    }
    fn visit_str<E: serde::de::Error>(self, _: &str) -> Result<Self, E> {
        Ok(self)
    }
}
