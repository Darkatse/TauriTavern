//! Pass user documents across IPC as text or bytes: `Value` sorts object keys.
//! Edit only backend-owned fields; never write a user document back from a typed projection.
//! Unopened values retain their text. Existing keys keep their relative order; new keys append.
use std::collections::HashMap;
use std::fmt;

use indexmap::IndexMap;
use serde::de::{DeserializeOwned, Deserializer};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_json::value::{RawValue, to_raw_value};

const UNSET_SENTINEL: &str = "__@@UNSET@@__";

/// Raw JSON can preserve lone surrogate escapes that Rust strings cannot represent.
#[derive(Debug, Default, Clone, Serialize)]
#[serde(transparent)]
pub struct JsonDocument {
    members: IndexMap<String, Node>,
}

impl JsonDocument {
    pub fn parse(json: &[u8]) -> serde_json::Result<Self> {
        serde_json::from_slice(json).map(|members| Self { members })
    }

    pub fn from_serialize<T: Serialize + ?Sized>(value: &T) -> serde_json::Result<Self> {
        Self::parse(&serde_json::to_vec(value)?)
    }

    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }

    /// Reads without changing the stored text, including along array-index paths.
    pub fn get<T: DeserializeOwned>(&self, path: &[&str]) -> serde_json::Result<Option<T>> {
        let Some((key, rest)) = path.split_first() else {
            return serde_json::from_slice(&self.to_vec()).map(Some);
        };
        match self.members.get(*key) {
            Some(Node::Text(text)) => decode_at(text, rest),
            Some(Node::Object(object)) => object.get(rest),
            None => Ok(None),
        }
    }

    pub fn object_mut(&mut self, path: &[&str]) -> serde_json::Result<Option<&mut Self>> {
        let mut object = self;
        for key in path {
            let Some(node) = object.members.get_mut(*key) else {
                return Ok(None);
            };
            let Some(next) = node.as_object_mut()? else {
                return Ok(None);
            };
            object = next;
        }
        Ok(Some(object))
    }

    /// Missing or non-object ancestors are replaced with empty objects.
    pub fn object_entry(&mut self, path: &[&str]) -> serde_json::Result<&mut Self> {
        let mut object = self;
        for key in path {
            let node = object
                .members
                .entry((*key).to_owned())
                .or_insert_with(|| Node::Object(Self::default()));
            if !node.is_object() {
                *node = Node::Object(Self::default());
            }
            object = node.as_object_mut()?.expect("an object member");
        }
        Ok(object)
    }

    pub fn set(&mut self, key: &str, member: impl Into<Member>) {
        self.members.insert(key.to_owned(), member.into().0);
    }

    pub fn remove(&mut self, key: &str) -> Option<Member> {
        self.members.shift_remove(key).map(Member)
    }

    /// Only object pairs merge recursively; arrays and scalars replace their targets.
    pub fn merge(&mut self, update: Self) -> serde_json::Result<()> {
        self.merge_members(update, false)
    }

    /// Only the literal unset marker emitted by `JSON.stringify` deletes an object member.
    pub fn merge_with_unset(&mut self, update: Self) -> serde_json::Result<()> {
        self.merge_members(update, true)
    }

    fn merge_members(&mut self, update: Self, unset: bool) -> serde_json::Result<()> {
        for (key, mut incoming) in update.members {
            if unset && incoming.is_unset() {
                self.members.shift_remove(&key);
                continue;
            }
            if incoming.is_object()
                && let Some(current) = self.members.get_mut(&key)
                && let Some(current) = current.as_object_mut()?
            {
                current.merge_members(incoming.into_document()?, unset)?;
                continue;
            }
            if unset {
                incoming.prune_unset()?;
            }
            self.members.insert(key, incoming);
        }
        Ok(())
    }

    pub fn to_vec(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("document members are valid JSON")
    }

    /// Uses four-space indentation without normalizing number spellings or string escapes.
    pub fn to_pretty_vec(&self) -> Vec<u8> {
        indent(&self.to_vec())
    }
}

impl fmt::Display for JsonDocument {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let json = serde_json::to_string(self).map_err(|_| fmt::Error)?;
        formatter.write_str(&json)
    }
}

#[derive(Debug)]
pub struct Member(Node);

impl From<Box<RawValue>> for Member {
    fn from(text: Box<RawValue>) -> Self {
        Self(Node::Text(text))
    }
}

impl From<JsonDocument> for Member {
    fn from(document: JsonDocument) -> Self {
        Self(Node::Object(document))
    }
}

impl From<&Value> for Member {
    fn from(value: &Value) -> Self {
        let text = to_raw_value(value).expect("a JSON value serializes");
        Self(Node::Text(text))
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
enum Node {
    Text(Box<RawValue>),
    Object(JsonDocument),
}

impl Node {
    fn is_object(&self) -> bool {
        match self {
            Node::Text(text) => is_object(text),
            Node::Object(_) => true,
        }
    }

    fn as_object_mut(&mut self) -> serde_json::Result<Option<&mut JsonDocument>> {
        if let Node::Text(text) = self {
            if !is_object(text) {
                return Ok(None);
            }
            *self = Node::Object(JsonDocument::parse(text.get().as_bytes())?);
        }
        Ok(match self {
            Node::Object(object) => Some(object),
            Node::Text(_) => None,
        })
    }

    fn into_document(self) -> serde_json::Result<JsonDocument> {
        match self {
            Node::Text(text) => JsonDocument::parse(text.get().as_bytes()),
            Node::Object(object) => Ok(object),
        }
    }

    fn is_unset(&self) -> bool {
        let Node::Text(text) = self else {
            return false;
        };
        let quoted = text.get().strip_prefix('"');
        quoted.and_then(|rest| rest.strip_suffix('"')) == Some(UNSET_SENTINEL)
    }

    fn prune_unset(&mut self) -> serde_json::Result<()> {
        if matches!(self, Node::Text(text) if !text.get().contains(UNSET_SENTINEL)) {
            return Ok(());
        }
        let Some(object) = self.as_object_mut()? else {
            return Ok(());
        };
        object.members.retain(|_, member| !member.is_unset());
        object.members.values_mut().try_for_each(Node::prune_unset)
    }
}

impl<'de> Deserialize<'de> for Node {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Box::<RawValue>::deserialize(deserializer).map(Node::Text)
    }
}

fn is_object(text: &RawValue) -> bool {
    text.get().starts_with('{')
}

fn decode_at<T: DeserializeOwned>(text: &RawValue, path: &[&str]) -> serde_json::Result<Option<T>> {
    let Some((segment, rest)) = path.split_first() else {
        return serde_json::from_str(text.get()).map(Some);
    };
    match child(text, segment)? {
        Some(text) => decode_at(text, rest),
        None => Ok(None),
    }
}

fn child<'a>(text: &'a RawValue, segment: &str) -> serde_json::Result<Option<&'a RawValue>> {
    let json = text.get();
    match json.as_bytes().first() {
        Some(b'{') => {
            let mut members: HashMap<String, &RawValue> = serde_json::from_str(json)?;
            Ok(members.remove(segment))
        }
        Some(b'[') => {
            let Ok(index) = segment.parse::<usize>() else {
                return Ok(None);
            };
            let elements: Vec<&RawValue> = serde_json::from_str(json)?;
            Ok(elements.get(index).copied())
        }
        _ => Ok(None),
    }
}

fn indent(json: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(json.len() + json.len() / 4);
    let mut depth = 0;
    let mut bytes = json.iter().copied().peekable();
    while let Some(byte) = bytes.next() {
        match byte {
            b'"' => {
                output.push(byte);
                while let Some(byte) = bytes.next() {
                    output.push(byte);
                    match byte {
                        b'"' => break,
                        b'\\' => output.extend(bytes.next()),
                        _ => {}
                    }
                }
            }
            b'{' | b'[' => {
                output.push(byte);
                while bytes.next_if(is_whitespace).is_some() {}
                match bytes.next_if(|next| matches!(next, b'}' | b']')) {
                    Some(close) => output.push(close),
                    None => {
                        depth += 1;
                        new_line(&mut output, depth);
                    }
                }
            }
            b'}' | b']' => {
                depth -= 1;
                new_line(&mut output, depth);
                output.push(byte);
            }
            b',' => {
                output.push(byte);
                new_line(&mut output, depth);
            }
            b':' => output.extend_from_slice(b": "),
            byte if is_whitespace(&byte) => {}
            _ => output.push(byte),
        }
    }
    output
}

fn is_whitespace(byte: &u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r')
}

fn new_line(output: &mut Vec<u8>, depth: usize) {
    output.push(b'\n');
    output.resize(output.len() + depth * 4, b' ');
}

#[cfg(test)]
mod tests {
    use serde_json::value::RawValue;
    use serde_json::{Value, json};

    use super::JsonDocument;

    fn document(json: &str) -> JsonDocument {
        JsonDocument::parse(json.as_bytes()).expect("a JSON object")
    }

    #[test]
    fn members_left_alone_keep_their_text_and_place() {
        let mut settings =
            document(r#"{"zeta":{"b": 1E3,"a":[ 1.0 ,-0]},"alpha":"é\/","mid":{"y":1,"x":2}}"#);

        let mid = settings.object_mut(&["mid"]).unwrap().unwrap();
        mid.set("x", &json!(3));

        assert_eq!(
            settings.to_string(),
            r#"{"zeta":{"b": 1E3,"a":[ 1.0 ,-0]},"alpha":"é\/","mid":{"y":1,"x":3}}"#
        );
    }

    #[test]
    fn set_replaces_in_place_or_appends_and_remove_keeps_the_rest_in_order() {
        let mut card = document(r#"{"c":1,"a":2,"b":3}"#);

        card.set("a", &json!("x"));
        card.set("z", &json!(true));
        card.remove("c");

        assert_eq!(card.to_string(), r#"{"a":"x","b":3,"z":true}"#);
    }

    #[test]
    fn received_text_and_documents_are_set_as_they_are() {
        let mut header = document(r#"{"user_name":"u","chat_metadata":{"old":1},"last":true}"#);
        let metadata: Box<RawValue> =
            serde_json::from_str(r#"{"zeta": 1E3,"alpha":{"y":1,"x":2}}"#).unwrap();

        header.set("chat_metadata", metadata);
        header.set("extensions", document(r#"{"b":1,"a":2}"#));

        assert_eq!(
            header.to_string(),
            r#"{"user_name":"u","chat_metadata":{"zeta": 1E3,"alpha":{"y":1,"x":2}},"last":true,"extensions":{"b":1,"a":2}}"#
        );
    }

    #[test]
    fn get_follows_object_keys_and_array_indices() {
        let mut card = document(r#"{"data":{"tags":["pilot",{"since": 2}],"0":"a key"}}"#);
        let first_tag = ["data", "tags", "0"];

        assert_eq!(card.get(&first_tag).unwrap(), Some(json!("pilot")));
        assert_eq!(card.get(&["data", "tags", "1", "since"]).unwrap(), Some(2));
        assert_eq!(card.get(&["data", "0"]).unwrap(), Some(json!("a key")));
        assert_eq!(card.get::<Value>(&["data", "tags", "2"]).unwrap(), None);
        assert_eq!(card.get::<Value>(&["data", "tags", "first"]).unwrap(), None);

        card.object_mut(&["data"]).unwrap();
        assert_eq!(card.get(&first_tag).unwrap(), Some(json!("pilot")));
    }

    #[test]
    fn merge_keeps_existing_keys_in_place_and_appends_new_ones() {
        let mut card = document(r#"{"zeta":{"b":1,"a":2},"list":[1],"alpha":1}"#);

        card.merge(document(
            r#"{"new":{"y":1,"x":2},"zeta":{"c":3,"a":{"k":1}},"list":{"now":"object"},"alpha":[2]}"#,
        ))
        .unwrap();

        assert_eq!(
            card.to_string(),
            r#"{"zeta":{"b":1,"a":{"k":1},"c":3},"list":{"now":"object"},"alpha":[2],"new":{"y":1,"x":2}}"#
        );
    }

    #[test]
    fn merge_with_unset_removes_marked_members() {
        let mut card = document(r#"{"keep":1,"drop":2,"nested":{"x":1,"y":2},"replaced":1}"#);

        card.merge_with_unset(document(
            r#"{"drop":"__@@UNSET@@__","nested":{"x":"__@@UNSET@@__"},"replaced":{"a":"__@@UNSET@@__","b":{"c":"__@@UNSET@@__","d":1}},"added":{"e":"__@@UNSET@@__"}}"#,
        ))
        .unwrap();

        assert_eq!(
            card.to_string(),
            r#"{"keep":1,"nested":{"y":2},"replaced":{"b":{"d":1}},"added":{}}"#
        );
    }

    #[test]
    fn pretty_text_is_laid_out_like_json_stringify() {
        let preset = document(r#"{"a":{},"b":[ ],"c":[1,{"d":"x{y}[z],\": "}],"e":{"f":null}}"#);

        assert_eq!(
            String::from_utf8(preset.to_pretty_vec()).unwrap(),
            r#"{
    "a": {},
    "b": [],
    "c": [
        1,
        {
            "d": "x{y}[z],\": "
        }
    ],
    "e": {
        "f": null
    }
}"#
        );
    }

    #[test]
    fn text_rust_cannot_decode_passes_through_and_is_an_error_to_read() {
        let json = r#"{"value":"\ud83d","object":{"\ud83d":1}}"#;
        let mut settings = document(json);

        assert!(settings.get::<Value>(&["value"]).is_err());
        assert!(settings.object_mut(&["object"]).is_err());
        assert_eq!(settings.to_string(), json);
    }
}
