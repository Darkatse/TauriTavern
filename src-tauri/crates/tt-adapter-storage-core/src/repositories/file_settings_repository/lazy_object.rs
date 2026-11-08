//! Settings objects whose members keep the JSON text they arrived in until a reader opens them. The
//! backend reads a handful of fields; the rest, mostly extension data, passes through as text.
use indexmap::IndexMap;
use serde::ser::{Serialize, SerializeMap, Serializer};
use serde_json::Value;
use serde_json::value::RawValue;

pub(super) enum Member {
    Text(Box<RawValue>),
    Object(LazyObject),
}

#[derive(Default)]
pub(super) struct LazyObject(IndexMap<String, Member>);

impl LazyObject {
    /// Parses a JSON object, keeping each member's text.
    pub(super) fn parse(json: &[u8]) -> serde_json::Result<Self> {
        let members: IndexMap<String, Box<RawValue>> = serde_json::from_slice(json)?;
        Ok(Self(
            members
                .into_iter()
                .map(|(key, text)| (key, Member::Text(text)))
                .collect(),
        ))
    }

    pub(super) fn from_value(value: &Value) -> serde_json::Result<Self> {
        Self::parse(&serde_json::to_vec(value)?)
    }

    /// Unopened members are written back as the text they arrived in.
    pub(super) fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("settings members are valid JSON")
    }

    pub(super) fn into_value(self) -> Value {
        Value::Object(
            self.0
                .into_iter()
                .map(|(key, member)| (key, member.into_value()))
                .collect(),
        )
    }

    pub(super) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The object at `path`, opening members on the way; `None` where one is missing or not an object.
    pub(super) fn object_mut(&mut self, path: &[&str]) -> Option<&mut Self> {
        path.iter()
            .try_fold(self, |object, key| object.0.get_mut(*key)?.as_object_mut())
    }

    /// The object at `path`, replacing any member on the way that is missing or not an object.
    pub(super) fn object_entry(&mut self, path: &[&str]) -> &mut Self {
        let mut object = self;
        for key in path {
            let member = object
                .0
                .entry((*key).to_owned())
                .or_insert_with(|| Member::Object(Self::default()));
            if member.as_object_mut().is_none() {
                *member = Member::Object(Self::default());
            }
            object = member.as_object_mut().expect("an object member");
        }
        object
    }

    pub(super) fn remove(&mut self, key: &str) -> Option<Member> {
        self.0.shift_remove(key)
    }

    pub(super) fn insert(&mut self, key: &str, member: Member) {
        self.0.insert(key.to_owned(), member);
    }

    pub(super) fn value(&self, key: &str) -> Option<Value> {
        self.0.get(key).map(Member::to_value)
    }

    /// Replaces a member in place, or appends it.
    pub(super) fn set_value(&mut self, key: &str, value: &Value) {
        let text = serde_json::value::to_raw_value(value).expect("a JSON value serializes");
        self.0.insert(key.to_owned(), Member::Text(text));
    }
}

impl Member {
    /// Opens text that holds an object; `None` for any other value.
    fn as_object_mut(&mut self) -> Option<&mut LazyObject> {
        if let Member::Text(text) = self {
            if !text.get().starts_with('{') {
                return None;
            }
            let opened = LazyObject::parse(text.get().as_bytes())
                .expect("member text was validated as a JSON object");
            *self = Member::Object(opened);
        }
        match self {
            Member::Object(object) => Some(object),
            Member::Text(_) => None,
        }
    }

    pub(super) fn into_value(self) -> Value {
        match self {
            Member::Text(text) => text_value(&text),
            Member::Object(object) => object.into_value(),
        }
    }

    fn to_value(&self) -> Value {
        match self {
            Member::Text(text) => text_value(text),
            Member::Object(object) => {
                serde_json::to_value(object).expect("settings members are valid JSON")
            }
        }
    }
}

fn text_value(text: &RawValue) -> Value {
    serde_json::from_str(text.get()).expect("member text is valid JSON")
}

impl Serialize for LazyObject {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (key, member) in &self.0 {
            match member {
                Member::Text(text) => map.serialize_entry(key, text)?,
                Member::Object(object) => map.serialize_entry(key, object)?,
            }
        }
        map.end()
    }
}
