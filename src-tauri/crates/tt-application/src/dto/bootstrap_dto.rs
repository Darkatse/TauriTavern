use serde::{Deserialize, Serialize};

use crate::dto::character_dto::CharacterDto;
use crate::dto::group_dto::GroupDto;
use crate::dto::secret_dto::SecretStateDto;
use tt_domain::ios_policy::IosPolicyActivationReport;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapMetadataDto {
    pub characters: Vec<CharacterDto>,
    pub groups: Vec<GroupDto>,
    pub avatars: Vec<String>,
    pub secret_state: SecretStateDto,
    pub ios_policy: IosPolicyActivationReport,
}
