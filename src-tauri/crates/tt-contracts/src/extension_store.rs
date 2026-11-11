use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EntryKind {
    Json,
    Blob,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WriteOperation {
    SetJson,
    UpdateJson,
    SetBlob,
}

impl WriteOperation {
    pub fn kind(self) -> EntryKind {
        match self {
            Self::SetJson | Self::UpdateJson => EntryKind::Json,
            Self::SetBlob => EntryKind::Blob,
        }
    }
}
