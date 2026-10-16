use serde_json::Value;
use sha2::{Digest, Sha256};
use tt_domain::errors::DomainError;
use tt_domain::models::character::Character;
use tt_domain::models::filename::MAX_SANITIZED_FILENAME_BYTES;
use tt_domain::models::world_info::{WORLD_INFO_EXTENSION, sanitize_world_info_name};

use super::lorebook_codec::character_book_to_world_info;
use super::{CharacterCardValidationMode, CharacterService, card_contract};
use crate::dto::character_dto::{
    CharacterLorebookAdoptionChoice, CharacterLorebookAdoptionDto,
    ResolveCharacterLorebookAdoptionDto,
};
use crate::errors::ApplicationError;
use crate::services::hashing::hex_lower;

enum LorebookAdoptionPlan {
    None,
    Keep,
    Place(LorebookPlacement),
    Decision {
        placement: LorebookPlacement,
        can_update: bool,
        token: String,
    },
}

struct LorebookPlacement {
    world_info: Value,
    target: String,
    create: bool,
}

impl CharacterService {
    pub async fn adopt_character_lorebook(
        &self,
        name: &str,
    ) -> Result<CharacterLorebookAdoptionDto, ApplicationError> {
        let mut character = self.repository.find_by_name(name).await?;
        let previous = character.data.extensions.world().to_string();
        self.adopt_embedded_lorebook(&mut character, &previous)
            .await
    }

    pub(super) async fn adopt_embedded_lorebook(
        &self,
        character: &mut Character,
        previous: &str,
    ) -> Result<CharacterLorebookAdoptionDto, ApplicationError> {
        match self.prepare_lorebook_adoption(character, previous).await? {
            LorebookAdoptionPlan::None => Ok(CharacterLorebookAdoptionDto::None),
            LorebookAdoptionPlan::Keep => Ok(CharacterLorebookAdoptionDto::Linked {
                world: previous.to_string(),
                previous: previous.to_string(),
                created: false,
                updated: false,
            }),
            LorebookAdoptionPlan::Place(placement) => {
                self.place_and_link_lorebook(character, previous, placement)
                    .await
            }
            LorebookAdoptionPlan::Decision {
                placement,
                can_update,
                token,
            } => Ok(CharacterLorebookAdoptionDto::Decision {
                world: previous.to_string(),
                target: placement.target,
                can_update,
                token,
            }),
        }
    }

    pub async fn resolve_lorebook_adoption(
        &self,
        dto: ResolveCharacterLorebookAdoptionDto,
    ) -> Result<CharacterLorebookAdoptionDto, ApplicationError> {
        let mut character = self.repository.find_by_name(&dto.name).await?;
        let previous = character.data.extensions.world().to_string();
        let (placement, can_update) = match self
            .prepare_lorebook_adoption(&character, &previous)
            .await?
        {
            LorebookAdoptionPlan::Decision {
                placement,
                can_update,
                token,
            } if token == dto.token => (placement, can_update),
            _ => {
                return Err(ApplicationError::Conflict(
                    "The lorebook changed while awaiting a decision; nothing was applied"
                        .to_string(),
                ));
            }
        };

        match dto.choice {
            CharacterLorebookAdoptionChoice::Adopt => {
                self.place_and_link_lorebook(&mut character, &previous, placement)
                    .await
            }
            CharacterLorebookAdoptionChoice::Update => {
                if !can_update {
                    return Err(ApplicationError::ValidationError(
                        "The current lorebook is not an update target for this card".to_string(),
                    ));
                }
                self.world_info_repository
                    .save_world_info(&previous, &placement.world_info)
                    .await?;
                Ok(CharacterLorebookAdoptionDto::Linked {
                    world: previous.clone(),
                    previous,
                    created: false,
                    updated: true,
                })
            }
        }
    }

    async fn prepare_lorebook_adoption(
        &self,
        character: &Character,
        previous: &str,
    ) -> Result<LorebookAdoptionPlan, ApplicationError> {
        let Some(embedded) = character.data.character_book.as_ref() else {
            return Ok(LorebookAdoptionPlan::None);
        };
        let incoming_name = embedded
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| !name.is_empty())
            .map(ToString::to_string)
            .unwrap_or_else(|| format!("{}'s Lorebook", character.name));
        let incoming_name = sanitize_world_info_name(&incoming_name);
        if incoming_name.is_empty() {
            return Err(ApplicationError::ValidationError(
                "Embedded lorebook name is invalid".to_string(),
            ));
        }
        let world_info = character_book_to_world_info(embedded)?;
        let current = self.world_info_repository.get_world_info(previous).await?;
        let can_update = Self::is_lorebook_update_target(previous, &incoming_name)?;
        let differs = current
            .as_ref()
            .is_some_and(|current| current.get("entries") != world_info.get("entries"));

        if current.is_some() && !differs && can_update {
            return Ok(LorebookAdoptionPlan::Keep);
        }

        let (target, create) = self
            .find_lorebook_placement(&incoming_name, &world_info)
            .await?;
        let placement = LorebookPlacement {
            world_info,
            target,
            create,
        };
        if differs {
            let inputs = serde_json::to_vec(&(previous, &placement.target, embedded, &current))
                .map_err(|error| {
                    ApplicationError::InternalError(format!(
                        "Failed to serialize lorebook decision: {error}",
                    ))
                })?;
            Ok(LorebookAdoptionPlan::Decision {
                placement,
                can_update,
                token: hex_lower(&Sha256::digest(inputs)),
            })
        } else {
            Ok(LorebookAdoptionPlan::Place(placement))
        }
    }

    async fn find_lorebook_placement(
        &self,
        name: &str,
        world_info: &Value,
    ) -> Result<(String, bool), DomainError> {
        for index in 0..100_000 {
            let candidate = if index == 0 {
                name.to_string()
            } else {
                Self::indexed_world_name(name, index)?
            };
            match self
                .world_info_repository
                .get_world_info(&candidate)
                .await?
            {
                Some(existing) if existing.get("entries") == world_info.get("entries") => {
                    return Ok((candidate, false));
                }
                Some(_) => continue,
                None => return Ok((candidate, true)),
            }
        }
        Err(DomainError::InvalidData(
            "Unable to allocate an embedded lorebook name".to_string(),
        ))
    }

    async fn place_and_link_lorebook(
        &self,
        character: &mut Character,
        previous: &str,
        placement: LorebookPlacement,
    ) -> Result<CharacterLorebookAdoptionDto, ApplicationError> {
        let LorebookPlacement {
            target,
            world_info,
            create,
        } = placement;
        if create {
            self.world_info_repository
                .save_world_info(&target, &world_info)
                .await?;
        }
        if character.data.extensions.world() != target {
            let linked = async {
                let raw = character.json_data.as_deref().ok_or_else(|| {
                    ApplicationError::InternalError(
                        "Stored character card JSON is missing".to_string(),
                    )
                })?;
                let mut card = card_contract::parse_character_card_json(raw)?;
                card_contract::set_character_world(&mut card, target.clone())?;
                self.write_character_card_value(
                    &character.get_file_name(),
                    card,
                    None,
                    None,
                    CharacterCardValidationMode::ReadableOnly,
                )
                .await
            }
            .await
            .map_err(|error| {
                ApplicationError::InternalError(if create {
                    format!("Lorebook '{target}' was saved but could not be linked: {error}")
                } else {
                    format!("Lorebook '{target}' could not be linked: {error}")
                })
            })?;
            *character = linked;
        }
        Ok(CharacterLorebookAdoptionDto::Linked {
            world: target,
            previous: previous.to_string(),
            created: create,
            updated: false,
        })
    }

    fn is_lorebook_update_target(current: &str, incoming: &str) -> Result<bool, DomainError> {
        if current == incoming {
            return Ok(true);
        }
        let index = current
            .rsplit_once(" (")
            .and_then(|(_, suffix)| suffix.strip_suffix(')'))
            .and_then(|index| index.parse::<usize>().ok())
            .filter(|index| *index > 0);
        match index {
            Some(index) => Ok(Self::indexed_world_name(incoming, index)? == current),
            None => Ok(false),
        }
    }

    pub(super) fn indexed_world_name(base_name: &str, index: usize) -> Result<String, DomainError> {
        let suffix = format!(" ({index})");
        let max_base_bytes = MAX_SANITIZED_FILENAME_BYTES
            .checked_sub(WORLD_INFO_EXTENSION.len() + 1 + suffix.len())
            .ok_or_else(|| {
                DomainError::InvalidData("World info name suffix is too long".to_string())
            })?;
        let mut end = base_name.len().min(max_base_bytes);
        while !base_name.is_char_boundary(end) {
            end -= 1;
        }
        let candidate = sanitize_world_info_name(&format!("{}{suffix}", &base_name[..end]));
        if candidate.is_empty() {
            return Err(DomainError::InvalidData(
                "Unable to allocate a world info copy name".to_string(),
            ));
        }
        Ok(candidate)
    }
}
