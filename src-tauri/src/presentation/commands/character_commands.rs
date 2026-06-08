use std::sync::Arc;

use tauri::State;

use crate::app::AppState;
use crate::application::dto::character_dto::{
    BulkMergeCharacterCardDataDto, BulkMergeCharacterCardDataResultDto, CharacterChatDto,
    CharacterDto, CreateCharacterDto, CreateCharacterWithAvatarResultDto, CreateWithAvatarDto,
    DeleteCharacterDto, DuplicateCharacterDto, ExportCharacterContentDto,
    ExportCharacterContentResultDto, ExportCharacterDto, GetCharacterChatsDto, ImportCharacterDto,
    MergeCharacterCardDataDto, RenameCharacterDto, UpdateAvatarDto, UpdateCharacterCardDataDto,
    UpdateCharacterDto,
};
use crate::domain::models::skill::{SkillScope, SkillScopeRetargetRequest};
use crate::presentation::commands::helpers::{log_command, map_command_error};
use crate::presentation::errors::CommandError;

const SKILL_SOURCE_KIND_CHARACTER: &str = "character";

#[tauri::command]
pub async fn get_all_characters(
    shallow: bool,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<CharacterDto>, CommandError> {
    log_command(format!("get_all_characters (shallow: {})", shallow));

    app_state
        .character_service
        .get_all_characters(shallow)
        .await
        .map_err(map_command_error("Failed to get all characters"))
}

#[tauri::command]
pub async fn get_character(
    name: String,
    app_state: State<'_, Arc<AppState>>,
) -> Result<CharacterDto, CommandError> {
    log_command(format!("get_character {}", name));

    app_state
        .character_service
        .get_character(&name)
        .await
        .map_err(map_command_error(format!(
            "Failed to get character {}",
            name
        )))
}

#[tauri::command]
pub async fn create_character(
    dto: CreateCharacterDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<CharacterDto, CommandError> {
    log_command(format!("create_character {}", dto.name));

    app_state
        .character_service
        .create_character(dto)
        .await
        .map_err(map_command_error("Failed to create character"))
}

#[tauri::command]
pub async fn create_character_with_avatar(
    dto: CreateWithAvatarDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<CreateCharacterWithAvatarResultDto, CommandError> {
    log_command(format!(
        "create_character_with_avatar {}",
        dto.character.name
    ));

    app_state
        .character_service
        .create_with_avatar(dto)
        .await
        .map_err(map_command_error("Failed to create character with avatar"))
}

#[tauri::command]
pub async fn update_character(
    name: String,
    dto: UpdateCharacterDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<CharacterDto, CommandError> {
    log_command(format!("update_character {}", name));

    app_state
        .character_service
        .update_character(&name, dto)
        .await
        .map_err(map_command_error("Failed to update character"))
}

#[tauri::command]
pub async fn update_character_card_data(
    name: String,
    dto: UpdateCharacterCardDataDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<CharacterDto, CommandError> {
    log_command(format!("update_character_card_data {}", name));

    app_state
        .character_service
        .update_character_card_data(&name, dto)
        .await
        .map_err(map_command_error("Failed to update character card data"))
}

#[tauri::command]
pub async fn merge_character_card_data(
    name: String,
    dto: MergeCharacterCardDataDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<CharacterDto, CommandError> {
    log_command(format!("merge_character_card_data {}", name));

    app_state
        .character_service
        .merge_character_card_data(&name, dto)
        .await
        .map_err(map_command_error("Failed to merge character card data"))
}

#[tauri::command]
pub async fn bulk_merge_character_card_data(
    dto: BulkMergeCharacterCardDataDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<BulkMergeCharacterCardDataResultDto, CommandError> {
    log_command("bulk_merge_character_card_data");

    app_state
        .character_service
        .bulk_merge_character_card_data(dto)
        .await
        .map_err(map_command_error(
            "Failed to bulk merge character card data",
        ))
}

#[tauri::command]
pub async fn delete_character(
    dto: DeleteCharacterDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command(format!("delete_character {}", dto.name));

    let name = dto.name.clone();
    app_state
        .character_service
        .delete_character(dto)
        .await
        .map_err(map_command_error("Failed to delete character"))?;

    app_state
        .skill_service
        .delete_skills_for_source(
            SKILL_SOURCE_KIND_CHARACTER,
            &character_skill_source_id(&name),
        )
        .await
        .map_err(map_command_error(
            "Failed to delete Agent Skills linked to character",
        ))?;

    Ok(())
}

#[tauri::command]
pub async fn rename_character(
    dto: RenameCharacterDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<CharacterDto, CommandError> {
    log_command(format!(
        "rename_character {} -> {}",
        dto.old_name, dto.new_name
    ));

    let old_character_id = dto.old_name.clone();
    let renamed = app_state
        .character_service
        .rename_character(dto)
        .await
        .map_err(map_command_error("Failed to rename character"))?;

    let new_character_id = character_id_from_avatar(&renamed.avatar)?;
    if old_character_id != new_character_id {
        app_state
            .skill_service
            .retarget_scope(SkillScopeRetargetRequest {
                from_scope: SkillScope::Character {
                    character_id: old_character_id,
                },
                to_scope: SkillScope::Character {
                    character_id: new_character_id,
                },
            })
            .await
            .map_err(map_command_error(
                "Failed to retarget Agent Skills linked to character",
            ))?;
    }

    Ok(renamed)
}

#[tauri::command]
pub async fn duplicate_character(
    dto: DuplicateCharacterDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<CharacterDto, CommandError> {
    log_command(format!("duplicate_character {}", dto.name));

    app_state
        .character_service
        .duplicate_character(dto)
        .await
        .map_err(map_command_error("Failed to duplicate character"))
}

#[tauri::command]
pub async fn import_character(
    dto: ImportCharacterDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<CharacterDto, CommandError> {
    log_command(format!("import_character from {}", dto.file_path));

    app_state
        .character_service
        .import_character(dto)
        .await
        .map_err(map_command_error("Failed to import character"))
}

#[tauri::command]
pub async fn export_character(
    dto: ExportCharacterDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command(format!(
        "export_character {} to {}",
        dto.name, dto.target_path
    ));

    app_state
        .character_service
        .export_character(dto)
        .await
        .map_err(map_command_error("Failed to export character"))
}

#[tauri::command]
pub async fn export_character_content(
    dto: ExportCharacterContentDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<ExportCharacterContentResultDto, CommandError> {
    log_command(format!(
        "export_character_content {} format {}",
        dto.name, dto.format
    ));

    app_state
        .character_service
        .export_character_content(dto)
        .await
        .map_err(map_command_error("Failed to export character content"))
}

#[tauri::command]
pub async fn update_avatar(
    dto: UpdateAvatarDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command(format!("update_avatar for {}", dto.name));

    app_state
        .character_service
        .update_avatar(dto)
        .await
        .map_err(map_command_error("Failed to update avatar"))
}

#[tauri::command]
pub async fn get_character_chats_by_id(
    dto: GetCharacterChatsDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<CharacterChatDto>, CommandError> {
    log_command(format!("get_character_chats_by_id for {}", dto.name));

    app_state
        .character_service
        .get_character_chats(dto)
        .await
        .map_err(map_command_error("Failed to get character chats"))
}

#[tauri::command]
pub async fn clear_character_cache(
    app_state: State<'_, Arc<AppState>>,
) -> Result<(), CommandError> {
    log_command("clear_character_cache");

    app_state
        .character_service
        .clear_cache()
        .await
        .map_err(map_command_error("Failed to clear character cache"))
}

fn character_skill_source_id(name: &str) -> String {
    format!("character:{}", name.trim())
}

fn character_id_from_avatar(avatar: &str) -> Result<String, CommandError> {
    let file_name = avatar
        .trim()
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .trim();
    let id = file_name
        .rsplit_once('.')
        .map(|(stem, _)| stem)
        .unwrap_or(file_name)
        .trim();
    if id.is_empty() {
        return Err(CommandError::BadRequest(
            "Character avatar did not resolve to a character id".to_string(),
        ));
    }
    Ok(id.to_string())
}
