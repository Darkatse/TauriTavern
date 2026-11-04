use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SafeModeReport {
    pub snapshot: String,
    pub disabled_extensions: usize,
    pub auto_upload_paused: bool,
    pub user_css_reset: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SafeModePrompt {
    Offer,
    Report(SafeModeReport),
}
