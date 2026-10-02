use std::fmt;
use std::fs::File;
use std::io::{self, BufReader, BufWriter, Write};
use std::path::Path;

use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::Value;
use tt_domain::errors::DomainError;
use tt_domain::json_merge::merge_json_value;

use super::io_error;

fn json_error(path: &Path, error: serde_json::Error) -> DomainError {
    DomainError::InvalidData(format!(
        "Invalid extension store JSON {}: {error}",
        path.display()
    ))
}

fn reader(path: &Path) -> io::Result<BufReader<File>> {
    File::open(path).map(BufReader::new)
}

pub(super) fn validate(source: &Path, target: &Path) -> Result<(), DomainError> {
    let reader = reader(source).map_err(|error| io_error("open JSON", source, error))?;
    serde_json::from_reader::<_, JsonValue>(reader)
        .map(|_| ())
        .map_err(|error| json_error(target, error))
}

pub(super) fn merge(target: &Path, patch: &Path, output: &Path) -> Result<(), DomainError> {
    let patch_reader = reader(patch).map_err(|error| io_error("open JSON patch", patch, error))?;
    let patch = serde_json::from_reader(patch_reader).map_err(|error| {
        DomainError::InvalidData(format!(
            "Invalid extension store JSON patch for {}: {error}",
            target.display()
        ))
    })?;
    let mut current = match reader(target) {
        Ok(reader) => serde_json::from_reader(reader).map_err(|error| json_error(target, error))?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => Value::Null,
        Err(error) => return Err(io_error("open JSON", target, error)),
    };
    merge_json_value(&mut current, patch);
    let file = File::options()
        .write(true)
        .create_new(true)
        .open(output)
        .map_err(|error| io_error("create merged JSON", output, error))?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer(&mut writer, &current).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to write merged extension store JSON {}: {error}",
            output.display()
        ))
    })?;
    writer
        .flush()
        .map_err(|error| io_error("flush merged JSON", output, error))
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
