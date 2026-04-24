use serde::{Deserialize, Serialize};

use crate::application::dto::character_dto::CharacterDto;
use crate::application::dto::group_dto::GroupDto;
use crate::application::dto::secret_dto::SecretStateDto;
use crate::application::dto::settings_dto::SillyTavernSettingsResponseDto;
use crate::domain::ios_policy::IosPolicyActivationReport;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapSnapshotDto {
    pub settings: SillyTavernSettingsResponseDto,
    pub characters: Vec<CharacterDto>,
    pub groups: Vec<GroupDto>,
    pub avatars: Vec<String>,
    pub secret_state: SecretStateDto,
    pub ios_policy: IosPolicyActivationReport,
}
