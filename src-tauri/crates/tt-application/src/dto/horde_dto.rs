use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Reaches the Horde as the page wrote it, except the generation `type`, which only the host reads.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct HordeTextGenerationRequestDto {
    #[serde(flatten)]
    pub payload: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HordeUserInfoDto {
    pub anonymous: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shared_key: Option<Value>,
}
