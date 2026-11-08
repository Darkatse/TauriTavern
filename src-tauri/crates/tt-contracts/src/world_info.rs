use serde::Serialize;
use serde_json::value::RawValue;

/// Serialized as one item of SillyTavern's `POST /api/worldinfo/list` response.
#[derive(Debug, Serialize)]
pub struct WorldInfoListItem {
    pub file_id: String,
    pub name: String,
    pub extensions: Box<RawValue>,
}
