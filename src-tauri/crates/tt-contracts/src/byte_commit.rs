use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitBegin {
    pub session_id: String,
    pub max_frame_bytes: u64,
}
