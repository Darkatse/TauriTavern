use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldInfoListItem {
    pub file_id: String,
    pub name: Value,
    pub extensions: Value,
}
