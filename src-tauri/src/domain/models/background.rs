use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundListEntry {
    pub filename: String,
    pub is_animated: bool,
}

#[derive(Debug, Clone)]
pub struct BackgroundAsset {
    pub bytes: Vec<u8>,
    pub mime_type: String,
}
