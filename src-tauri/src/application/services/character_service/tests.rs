use super::CharacterService;
use crate::application::dto::character_dto::{
    CreateCharacterDto, ExportCharacterContentDto, ExportCharacterDto, ImportCharacterDto,
    MergeCharacterCardDataDto, UpdateAvatarDto, UpdateCharacterCardDataDto, UpdateCharacterDto,
};
use crate::application::errors::ApplicationError;
use crate::application::services::agent_workspace_lifecycle_service::{
    AgentRunActivity, AgentWorkspaceLifecycleService,
};
use crate::domain::models::character::Character;
use crate::domain::repositories::character_repository::CharacterRepository;
use crate::domain::repositories::world_info_repository::WorldInfoRepository;
use crate::infrastructure::persistence::png_utils::{
    read_character_data_from_png, write_character_data_to_png,
};
use crate::infrastructure::repositories::file_agent_repository::FileAgentRepository;
use crate::infrastructure::repositories::file_character_repository::FileCharacterRepository;
use crate::infrastructure::repositories::file_chat_repository::FileChatRepository;
use crate::infrastructure::repositories::file_world_info_repository::FileWorldInfoRepository;
use async_trait::async_trait;
use image::{DynamicImage, ImageFormat, RgbaImage};
use rand::random;
use serde_json::json;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs;

struct NoActiveAgentRuns;

#[async_trait]
impl AgentRunActivity for NoActiveAgentRuns {
    async fn active_run_ids_for_workspace(
        &self,
        _workspace_id: &str,
    ) -> Result<Vec<String>, ApplicationError> {
        Ok(Vec::new())
    }
}

async fn write_character_png(root: &PathBuf, file_stem: &str, payload: &serde_json::Value) {
    let png_bytes = write_character_data_to_png(
        &build_minimal_png(),
        &serde_json::to_string(payload).expect("serialize card payload"),
    )
    .expect("embed card in png");
    fs::write(
        root.join("characters").join(format!("{}.png", file_stem)),
        png_bytes,
    )
    .await
    .expect("write character png");
}

fn unique_temp_root() -> PathBuf {
    std::env::temp_dir().join(format!("tauritavern-character-service-{}", random::<u64>()))
}

fn build_minimal_png() -> Vec<u8> {
    let image = DynamicImage::ImageRgba8(RgbaImage::new(1, 1));
    let mut output = Vec::new();
    let mut cursor = Cursor::new(&mut output);
    image
        .write_to(&mut cursor, ImageFormat::Png)
        .expect("should build png image");
    output
}

fn empty_update_character_dto() -> UpdateCharacterDto {
    UpdateCharacterDto {
        name: None,
        chat: None,
        description: None,
        personality: None,
        scenario: None,
        first_mes: None,
        mes_example: None,
        creator: None,
        creator_notes: None,
        character_version: None,
        tags: None,
        talkativeness: None,
        fav: None,
        alternate_greetings: None,
        system_prompt: None,
        post_history_instructions: None,
        extensions: None,
    }
}

async fn setup_service() -> (
    CharacterService,
    FileCharacterRepository,
    FileWorldInfoRepository,
    PathBuf,
) {
    let root = unique_temp_root();
    let characters_dir = root.join("characters");
    let chats_dir = root.join("chats");
    let thumbnails_avatar_dir = root.join("thumbnails/avatar");
    let worlds_dir = root.join("worlds");
    let default_avatar = root.join("default.png");

    fs::create_dir_all(&characters_dir)
        .await
        .expect("create characters dir");
    fs::create_dir_all(&chats_dir)
        .await
        .expect("create chats dir");
    fs::create_dir_all(&thumbnails_avatar_dir)
        .await
        .expect("create avatar thumbnails dir");
    fs::create_dir_all(&worlds_dir)
        .await
        .expect("create worlds dir");
    fs::write(&default_avatar, build_minimal_png())
        .await
        .expect("write default avatar");

    let character_repository = FileCharacterRepository::new(
        characters_dir,
        chats_dir,
        thumbnails_avatar_dir,
        default_avatar,
    );
    let world_info_repository = FileWorldInfoRepository::new(worlds_dir);
    let service = CharacterService::new(
        Arc::new(FileCharacterRepository::new(
            root.join("characters"),
            root.join("chats"),
            root.join("thumbnails/avatar"),
            root.join("default.png"),
        )),
        Arc::new(FileChatRepository::new(
            root.join("characters"),
            root.join("chats"),
            root.join("group_chats"),
            root.join("backups"),
        )),
        Arc::new(FileWorldInfoRepository::new(root.join("worlds"))),
        Arc::new(AgentWorkspaceLifecycleService::new(
            Arc::new(FileAgentRepository::new(
                root.join("_tauritavern/agent-workspaces"),
            )),
            Arc::new(NoActiveAgentRuns),
        )),
    );

    (service, character_repository, world_info_repository, root)
}

async fn save_bound_world(
    world_info_repository: &FileWorldInfoRepository,
    world_name: &str,
) -> serde_json::Value {
    let embedded_book: serde_json::Value = serde_json::from_str(
        r#"{
            "name": "",
            "entries": [
                {
                    "id": 1,
                    "keys": ["alpha"],
                    "secondary_keys": [],
                    "comment": "",
                    "content": "content",
                    "constant": false,
                    "selective": false,
                    "insertion_order": 100,
                    "enabled": true,
                    "position": "after_char",
                    "use_regex": true,
                    "extensions": {
                        "position": 1,
                        "display_index": 0,
                        "probability": 100,
                        "useProbability": false,
                        "depth": 4,
                        "selectiveLogic": 0,
                        "outlet_name": "",
                        "group": "",
                        "group_override": false,
                        "group_weight": null,
                        "prevent_recursion": false,
                        "delay_until_recursion": false,
                        "scan_depth": null,
                        "match_whole_words": null,
                        "use_group_scoring": false,
                        "case_sensitive": null,
                        "automation_id": "",
                        "role": 0,
                        "vectorized": false,
                        "sticky": null,
                        "cooldown": null,
                        "delay": null,
                        "match_persona_description": false,
                        "match_character_description": false,
                        "match_character_personality": false,
                        "match_character_depth_prompt": false,
                        "match_scenario": false,
                        "match_creator_notes": false,
                        "triggers": [],
                        "ignore_budget": false
                    }
                }
            ]
        }"#,
    )
    .expect("parse embedded book");
    let embedded_book = match embedded_book {
        serde_json::Value::Object(mut object) => {
            object.insert("name".to_string(), json!(world_name));
            serde_json::Value::Object(object)
        }
        _ => unreachable!("embedded book should be an object"),
    };
    let world_payload: serde_json::Value = serde_json::from_str(
        r#"{
            "entries": {
                "1": {
                    "uid": 1,
                    "key": ["alpha"],
                    "keysecondary": [],
                    "comment": "",
                    "content": "fresh",
                    "constant": false,
                    "selective": false,
                    "order": 100,
                    "position": 1,
                    "disable": false,
                    "extensions": {},
                    "displayIndex": 0,
                    "probability": 100,
                    "useProbability": false,
                    "depth": 4,
                    "selectiveLogic": 0,
                    "outletName": "",
                    "group": "",
                    "groupOverride": false,
                    "groupWeight": null,
                    "preventRecursion": false,
                    "delayUntilRecursion": false,
                    "scanDepth": null,
                    "matchWholeWords": null,
                    "useGroupScoring": false,
                    "caseSensitive": null,
                    "automationId": "",
                    "role": 0,
                    "vectorized": false,
                    "sticky": null,
                    "cooldown": null,
                    "delay": null,
                    "matchPersonaDescription": false,
                    "matchCharacterDescription": false,
                    "matchCharacterPersonality": false,
                    "matchCharacterDepthPrompt": false,
                    "matchScenario": false,
                    "matchCreatorNotes": false,
                    "triggers": [],
                    "ignoreBudget": false
                }
            }
        }"#,
    )
    .expect("parse bound world");
    let world_payload = match world_payload {
        serde_json::Value::Object(mut object) => {
            object.insert("originalData".to_string(), embedded_book.clone());
            serde_json::Value::Object(object)
        }
        _ => unreachable!("world payload should be an object"),
    };
    world_info_repository
        .save_world_info(world_name, &world_payload)
        .await
        .expect("save world info");
    embedded_book
}

async fn save_world_with_stale_original_data(
    world_info_repository: &FileWorldInfoRepository,
    world_name: &str,
) -> serde_json::Value {
    let original_book = json!({
        "name": "Imported Lore",
        "description": "preserve me",
        "entries": [
            {
                "id": 1,
                "keys": ["alpha"],
                "content": "stale",
                "extensions": {}
            }
        ]
    });
    let world_payload: serde_json::Value = serde_json::from_str(
        r#"{
            "entries": {
                "7": {
                    "uid": 7,
                    "key": ["beta"],
                    "keysecondary": [],
                    "comment": "memo",
                    "content": "fresh",
                    "constant": false,
                    "selective": false,
                    "order": 33,
                    "position": 1,
                    "disable": false,
                    "extensions": {
                        "custom": "value"
                    },
                    "displayIndex": 0,
                    "probability": 100,
                    "useProbability": false,
                    "depth": 4,
                    "selectiveLogic": 0,
                    "outletName": "",
                    "group": "",
                    "groupOverride": false,
                    "groupWeight": null,
                    "preventRecursion": false,
                    "delayUntilRecursion": false,
                    "scanDepth": null,
                    "matchWholeWords": null,
                    "useGroupScoring": false,
                    "caseSensitive": null,
                    "automationId": "",
                    "role": 0,
                    "vectorized": false,
                    "sticky": null,
                    "cooldown": null,
                    "delay": null,
                    "matchPersonaDescription": false,
                    "matchCharacterDescription": false,
                    "matchCharacterPersonality": false,
                    "matchCharacterDepthPrompt": false,
                    "matchScenario": false,
                    "matchCreatorNotes": false,
                    "triggers": [],
                    "ignoreBudget": false
                }
            }
        }"#,
    )
    .expect("parse world payload");
    let world_payload = match world_payload {
        serde_json::Value::Object(mut object) => {
            object.insert("originalData".to_string(), original_book.clone());
            serde_json::Value::Object(object)
        }
        _ => unreachable!("world payload should be an object"),
    };
    world_info_repository
        .save_world_info(world_name, &world_payload)
        .await
        .expect("save world info");

    original_book
}

#[test]
fn build_export_card_value_removes_private_fields() {
    let mut character = Character::new(
        "Export Test".to_string(),
        "desc".to_string(),
        "persona".to_string(),
        "hello".to_string(),
    );
    character.chat = "private-chat-name".to_string();
    character.fav = true;
    character.data.extensions.fav = true;

    let mut export_value = serde_json::to_value(&character.to_v2()).expect("build export payload");
    super::card_contract::unset_private_fields(&mut export_value)
        .expect("private fields should be removed");

    assert!(
        export_value.get("chat").is_none(),
        "chat should be removed from exported payload"
    );
    assert_eq!(
        export_value.get("fav").and_then(|value| value.as_bool()),
        Some(false)
    );
    assert_eq!(
        export_value
            .pointer("/data/extensions/fav")
            .and_then(|value| value.as_bool()),
        Some(false)
    );
}

#[tokio::test]
async fn export_character_content_preserves_unknown_card_fields() {
    let (service, _character_repository, _world_info_repository, root) = setup_service().await;

    let card_payload = json!({
        "spec": "chara_card_v3",
        "spec_version": "3.0",
        "name": "Unknown Export",
        "first_mes": "Hello",
        "creatorcomment": "legacy field",
        "x_custom_top": { "nested": true },
        "data": {
            "name": "Unknown Export",
            "first_mes": "Hello",
            "extensions": {
                "talkativeness": 0.5,
                "fav": false,
                "world": "",
                "depth_prompt": {
                    "prompt": "",
                    "depth": 4,
                    "role": "system",
                },
            },
            "x_custom_data": 123,
        },
    });

    write_character_png(&root, "Unknown Export", &card_payload).await;

    let exported = service
        .export_character_content(ExportCharacterContentDto {
            name: "Unknown Export".to_string(),
            format: "json".to_string(),
        })
        .await
        .expect("export should succeed");

    let exported_json = String::from_utf8(exported.data).expect("export json utf8");
    let exported_value: serde_json::Value =
        serde_json::from_str(&exported_json).expect("parse exported json");

    assert!(
        exported_value.get("x_custom_top").is_some(),
        "exported json should preserve unknown top-level fields"
    );
    assert!(
        exported_value.pointer("/data/x_custom_data").is_some(),
        "exported json should preserve unknown data fields"
    );

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn get_character_includes_raw_json_data() {
    let (service, _character_repository, _world_info_repository, root) = setup_service().await;

    let card_payload = json!({
        "spec": "chara_card_v3",
        "spec_version": "3.0",
        "name": "Raw Json Character",
        "first_mes": "Hello",
        "x_custom_top": { "nested": true },
        "data": {
            "name": "Raw Json Character",
            "first_mes": "Hello",
            "extensions": {
                "talkativeness": 0.5,
                "fav": false,
                "world": "",
                "depth_prompt": {
                    "prompt": "",
                    "depth": 4,
                    "role": "system"
                }
            },
            "x_custom_data": 123
        }
    });

    write_character_png(&root, "Raw Json Character", &card_payload).await;

    let dto = service
        .get_character("Raw Json Character")
        .await
        .expect("get character");
    let raw_json = dto.json_data.expect("character should include raw json");
    let raw_value: serde_json::Value = serde_json::from_str(&raw_json).expect("parse raw json");

    assert!(raw_value.get("x_custom_top").is_some());
    assert!(raw_value.pointer("/data/x_custom_data").is_some());

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn update_character_card_data_preserves_unknown_fields() {
    let (service, character_repository, _world_info_repository, root) = setup_service().await;

    let card_payload = json!({
        "spec": "chara_card_v3",
        "spec_version": "3.0",
        "name": "Raw Update",
        "description": "Before",
        "first_mes": "Hello",
        "x_custom_top": { "nested": true },
        "data": {
            "name": "Raw Update",
            "description": "Before",
            "first_mes": "Hello",
            "extensions": {
                "talkativeness": 0.5,
                "fav": false,
                "world": "",
                "depth_prompt": {
                    "prompt": "",
                    "depth": 4,
                    "role": "system"
                }
            },
            "x_custom_data": 123
        }
    });

    write_character_png(&root, "Raw Update", &card_payload).await;

    let updated_payload = json!({
        "spec": "chara_card_v2",
        "spec_version": "2.0",
        "name": "Raw Update",
        "description": "After",
        "personality": "",
        "scenario": "",
        "first_mes": "Hello",
        "mes_example": "",
        "x_custom_top": { "nested": true },
        "data": {
            "name": "Raw Update",
            "description": "After",
            "personality": "",
            "scenario": "",
            "first_mes": "Hello",
            "mes_example": "",
            "creator_notes": "",
            "system_prompt": "",
            "post_history_instructions": "",
            "tags": [],
            "creator": "",
            "character_version": "",
            "alternate_greetings": [],
            "extensions": {
                "talkativeness": 0.5,
                "fav": false,
                "world": "",
                "depth_prompt": {
                    "prompt": "",
                    "depth": 4,
                    "role": "system"
                },
                "tavern_helper": {
                    "scripts": [
                        { "id": "script-1" }
                    ]
                }
            },
            "x_custom_data": 123
        }
    });

    service
        .update_character_card_data(
            "Raw Update",
            UpdateCharacterCardDataDto {
                card_json: serde_json::to_string(&updated_payload)
                    .expect("serialize update payload"),
                avatar_path: None,
                crop: None,
            },
        )
        .await
        .expect("update raw card data");

    let stored_json = character_repository
        .read_character_card_json("Raw Update")
        .await
        .expect("read updated character");
    let stored_value: serde_json::Value =
        serde_json::from_str(&stored_json).expect("parse updated character");

    assert!(stored_value.get("x_custom_top").is_some());
    assert!(stored_value.pointer("/data/x_custom_data").is_some());
    assert_eq!(
        stored_value.pointer("/data/extensions/tavern_helper/scripts/0/id"),
        Some(&json!("script-1"))
    );
    assert_eq!(
        stored_value
            .get("description")
            .and_then(serde_json::Value::as_str),
        Some("After")
    );

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn update_character_card_data_returns_v2_data_metadata_when_top_level_is_stale() {
    let (service, character_repository, _world_info_repository, root) = setup_service().await;

    let source_payload = json!({
        "spec": "chara_card_v2",
        "spec_version": "2.0",
        "name": "Metadata Update",
        "description": "",
        "personality": "",
        "scenario": "",
        "first_mes": "Hello",
        "mes_example": "",
        "creator": "root creator old",
        "creator_notes": "root notes old",
        "character_version": "1.0-root",
        "data": {
            "name": "Metadata Update",
            "description": "",
            "personality": "",
            "scenario": "",
            "first_mes": "Hello",
            "mes_example": "",
            "creator_notes": "data notes old",
            "system_prompt": "",
            "post_history_instructions": "",
            "tags": [],
            "creator": "data creator old",
            "character_version": "1.0-data",
            "alternate_greetings": [],
            "extensions": {
                "talkativeness": 0.5,
                "fav": false,
                "world": "",
                "depth_prompt": {
                    "prompt": "",
                    "depth": 4,
                    "role": "system"
                }
            }
        }
    });
    write_character_png(&root, "Metadata Update", &source_payload).await;

    let update_payload = json!({
        "spec": "chara_card_v2",
        "spec_version": "2.0",
        "name": "Metadata Update",
        "description": "",
        "personality": "",
        "scenario": "",
        "first_mes": "Hello",
        "mes_example": "",
        "creator": "root creator old",
        "creator_notes": "root notes old",
        "character_version": "1.0-root",
        "data": {
            "name": "Metadata Update",
            "description": "",
            "personality": "",
            "scenario": "",
            "first_mes": "Hello",
            "mes_example": "",
            "creator_notes": "data notes new",
            "system_prompt": "",
            "post_history_instructions": "",
            "tags": [],
            "creator": "data creator new",
            "character_version": "1.1-data",
            "alternate_greetings": [],
            "extensions": {
                "talkativeness": 0.5,
                "fav": false,
                "world": "",
                "depth_prompt": {
                    "prompt": "",
                    "depth": 4,
                    "role": "system"
                }
            }
        }
    });

    let updated = service
        .update_character_card_data(
            "Metadata Update",
            UpdateCharacterCardDataDto {
                card_json: serde_json::to_string(&update_payload).expect("serialize update"),
                avatar_path: None,
                crop: None,
            },
        )
        .await
        .expect("update character metadata");

    assert_eq!(updated.creator, "data creator new");
    assert_eq!(updated.creator_notes, "data notes new");
    assert_eq!(updated.character_version, "1.1-data");

    let shallow = service
        .get_all_characters(true)
        .await
        .expect("load shallow character list");
    assert_eq!(shallow.len(), 1);
    assert_eq!(shallow[0].creator, "data creator new");
    assert_eq!(shallow[0].creator_notes, "data notes new");
    assert_eq!(shallow[0].character_version, "1.1-data");

    let stored_json = character_repository
        .read_character_card_json("Metadata Update")
        .await
        .expect("read updated character");
    let stored_value: serde_json::Value =
        serde_json::from_str(&stored_json).expect("parse updated character");
    assert_eq!(
        stored_value
            .pointer("/data/character_version")
            .and_then(serde_json::Value::as_str),
        Some("1.1-data")
    );

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn update_character_preserves_unknown_fields() {
    let (service, character_repository, _world_info_repository, root) = setup_service().await;

    let card_payload = json!({
        "spec": "chara_card_v3",
        "spec_version": "3.0",
        "name": "Structured Update",
        "description": "Before",
        "first_mes": "Hello",
        "x_custom_top": { "nested": true },
        "data": {
            "name": "Structured Update",
            "description": "Before",
            "first_mes": "Hello",
            "extensions": {
                "talkativeness": 0.5,
                "fav": false,
                "world": "",
                "depth_prompt": {
                    "prompt": "",
                    "depth": 4,
                    "role": "system"
                },
                "tavern_helper": {
                    "scripts": [
                        { "id": "script-1" }
                    ]
                }
            },
            "x_custom_data": 123
        }
    });

    write_character_png(&root, "Structured Update", &card_payload).await;

    let mut dto = empty_update_character_dto();
    dto.description = Some("After".to_string());

    service
        .update_character("Structured Update", dto)
        .await
        .expect("structured update should succeed");

    let stored_json = character_repository
        .read_character_card_json("Structured Update")
        .await
        .expect("read updated character");
    let stored_value: serde_json::Value =
        serde_json::from_str(&stored_json).expect("parse updated character");

    assert_eq!(stored_value.get("spec"), Some(&json!("chara_card_v3")));
    assert_eq!(stored_value.get("spec_version"), Some(&json!("3.0")));
    assert!(stored_value.get("x_custom_top").is_some());
    assert!(stored_value.pointer("/data/x_custom_data").is_some());
    assert_eq!(
        stored_value.pointer("/data/extensions/tavern_helper/scripts/0/id"),
        Some(&json!("script-1"))
    );
    assert_eq!(
        stored_value
            .get("description")
            .and_then(serde_json::Value::as_str),
        Some("After")
    );
    assert_eq!(
        stored_value
            .pointer("/data/description")
            .and_then(serde_json::Value::as_str),
        Some("After")
    );

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn update_character_card_data_materializes_bound_lorebook_for_v3_origin_cards() {
    let (service, character_repository, world_info_repository, root) = setup_service().await;
    let embedded_book = save_bound_world(&world_info_repository, "bound-book").await;

    let source_card = json!({
        "spec": "chara_card_v3",
        "spec_version": "3.0",
        "name": "Bound Raw Update",
        "description": "Before",
        "first_mes": "Hello",
        "x_custom_top": { "nested": true },
        "data": {
            "name": "Bound Raw Update",
            "description": "Before",
            "first_mes": "Hello",
            "character_book": embedded_book,
            "extensions": {
                "talkativeness": 0.5,
                "fav": false,
                "world": "bound-book",
                "depth_prompt": {
                    "prompt": "",
                    "depth": 4,
                    "role": "system"
                },
                "tavern_helper": {
                    "scripts": [
                        { "id": "script-1" }
                    ]
                }
            },
            "x_custom_data": 123
        }
    });
    write_character_png(&root, "Bound Raw Update", &source_card).await;

    let update_payload = json!({
        "spec": "chara_card_v2",
        "spec_version": "2.0",
        "name": "Bound Raw Update",
        "description": "After",
        "personality": "",
        "scenario": "",
        "first_mes": "Hello",
        "mes_example": "",
        "x_custom_top": { "nested": true },
        "data": {
            "name": "Bound Raw Update",
            "description": "After",
            "personality": "",
            "scenario": "",
            "first_mes": "Hello",
            "mes_example": "",
            "creator_notes": "",
            "system_prompt": "",
            "post_history_instructions": "",
            "tags": [],
            "creator": "",
            "character_version": "",
            "alternate_greetings": [],
            "character_book": {
                "name": "bound-book",
                "entries": [
                    {
                        "id": 1,
                        "keys": ["alpha"],
                        "content": "stale"
                    }
                ]
            },
            "extensions": {
                "talkativeness": 0.5,
                "fav": false,
                "world": "bound-book",
                "depth_prompt": {
                    "prompt": "",
                    "depth": 4,
                    "role": "system"
                },
                "tavern_helper": {
                    "scripts": [
                        { "id": "script-1" }
                    ]
                }
            },
            "x_custom_data": 123
        }
    });

    service
        .update_character_card_data(
            "Bound Raw Update",
            UpdateCharacterCardDataDto {
                card_json: update_payload.to_string(),
                avatar_path: None,
                crop: None,
            },
        )
        .await
        .expect("bound world update should succeed");

    let stored_json = character_repository
        .read_character_card_json("Bound Raw Update")
        .await
        .expect("read updated character");
    let stored_value: serde_json::Value =
        serde_json::from_str(&stored_json).expect("parse updated character");
    assert_eq!(
        stored_value.pointer("/data/character_book/extensions"),
        Some(&json!({}))
    );
    assert_eq!(
        stored_value.pointer("/data/character_book/entries/0/content"),
        Some(&json!("fresh"))
    );
    assert_eq!(
        stored_value.pointer("/data/extensions/tavern_helper/scripts/0/id"),
        Some(&json!("script-1"))
    );

    let exported = service
        .export_character_content(ExportCharacterContentDto {
            name: "Bound Raw Update".to_string(),
            format: "json".to_string(),
        })
        .await
        .expect("export updated character");
    let exported_value: serde_json::Value =
        serde_json::from_slice(&exported.data).expect("parse exported character");
    assert_eq!(
        exported_value.pointer("/data/character_book/extensions"),
        Some(&json!({}))
    );
    assert_eq!(
        exported_value.pointer("/data/character_book/entries/0/content"),
        Some(&json!("fresh"))
    );
    assert_eq!(
        exported_value.pointer("/data/extensions/tavern_helper/scripts/0/id"),
        Some(&json!("script-1"))
    );
    assert!(exported_value.get("x_custom_top").is_some());
    assert!(exported_value.pointer("/data/x_custom_data").is_some());

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn update_character_materializes_bound_lorebook_for_v3_origin_cards() {
    let (service, character_repository, world_info_repository, root) = setup_service().await;
    let embedded_book = save_bound_world(&world_info_repository, "bound-book").await;

    let source_card = json!({
        "spec": "chara_card_v3",
        "spec_version": "3.0",
        "name": "Bound Structured Update",
        "description": "Before",
        "first_mes": "Hello",
        "x_custom_top": { "nested": true },
        "data": {
            "name": "Bound Structured Update",
            "description": "Before",
            "first_mes": "Hello",
            "character_book": embedded_book,
            "extensions": {
                "talkativeness": 0.5,
                "fav": false,
                "world": "bound-book",
                "depth_prompt": {
                    "prompt": "",
                    "depth": 4,
                    "role": "system"
                }
            },
            "x_custom_data": 123
        }
    });
    write_character_png(&root, "Bound Structured Update", &source_card).await;

    let mut dto = empty_update_character_dto();
    dto.description = Some("After".to_string());

    service
        .update_character("Bound Structured Update", dto)
        .await
        .expect("bound structured update should succeed");

    let stored_json = character_repository
        .read_character_card_json("Bound Structured Update")
        .await
        .expect("read updated character");
    let stored_value: serde_json::Value =
        serde_json::from_str(&stored_json).expect("parse updated character");

    assert_eq!(
        stored_value.pointer("/data/character_book/extensions"),
        Some(&json!({}))
    );
    assert_eq!(
        stored_value.pointer("/data/character_book/entries/0/content"),
        Some(&json!("fresh"))
    );
    assert!(stored_value.get("x_custom_top").is_some());
    assert!(stored_value.pointer("/data/x_custom_data").is_some());
    assert_eq!(
        stored_value
            .get("description")
            .and_then(serde_json::Value::as_str),
        Some("After")
    );

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn merge_character_card_data_preserves_unknown_fields() {
    let (service, character_repository, _world_info_repository, root) = setup_service().await;

    let card_payload = json!({
        "spec": "chara_card_v3",
        "spec_version": "3.0",
        "name": "Raw Merge",
        "description": "Before",
        "first_mes": "Hello",
        "x_custom_top": { "nested": true },
        "data": {
            "name": "Raw Merge",
            "description": "Before",
            "first_mes": "Hello",
            "extensions": {
                "talkativeness": 0.5,
                "fav": false,
                "world": "",
                "depth_prompt": {
                    "prompt": "",
                    "depth": 4,
                    "role": "system"
                }
            },
            "x_custom_data": 123
        }
    });

    write_character_png(&root, "Raw Merge", &card_payload).await;

    service
        .merge_character_card_data(
            "Raw Merge",
            MergeCharacterCardDataDto {
                update: json!({
                    "description": "After Merge",
                    "data": {
                        "extensions": {
                            "tavern_helper": {
                                "scripts": [
                                    { "id": "merged-script" }
                                ]
                            }
                        }
                    }
                }),
            },
        )
        .await
        .expect("merge raw card data");

    let stored_json = character_repository
        .read_character_card_json("Raw Merge")
        .await
        .expect("read merged character");
    let stored_value: serde_json::Value =
        serde_json::from_str(&stored_json).expect("parse merged character");

    assert!(stored_value.get("x_custom_top").is_some());
    assert!(stored_value.pointer("/data/x_custom_data").is_some());
    assert_eq!(
        stored_value.pointer("/data/extensions/tavern_helper/scripts/0/id"),
        Some(&json!("merged-script"))
    );
    assert_eq!(
        stored_value
            .get("description")
            .and_then(serde_json::Value::as_str),
        Some("After Merge")
    );

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn merge_character_card_data_rejects_invalid_v2_payloads() {
    let (service, _character_repository, _world_info_repository, root) = setup_service().await;

    let card_payload = json!({
        "spec": "chara_card_v3",
        "spec_version": "3.0",
        "name": "Invalid Raw Merge",
        "description": "Before",
        "first_mes": "Hello",
        "data": {
            "name": "Invalid Raw Merge",
            "description": "Before",
            "first_mes": "Hello",
            "extensions": {
                "talkativeness": 0.5,
                "fav": false,
                "world": "",
                "depth_prompt": {
                    "prompt": "",
                    "depth": 4,
                    "role": "system"
                }
            }
        }
    });

    write_character_png(&root, "Invalid Raw Merge", &card_payload).await;

    let error = service
        .merge_character_card_data(
            "Invalid Raw Merge",
            MergeCharacterCardDataDto {
                update: json!({
                    "spec": "chara_card_v2",
                    "spec_version": "2.0",
                    "description": "After",
                    "personality": "",
                    "scenario": "",
                    "mes_example": "",
                    "data": {
                        "name": "Invalid Raw Merge",
                        "description": "After",
                        "personality": "",
                        "scenario": "",
                        "first_mes": "Hello",
                        "mes_example": "",
                        "creator_notes": "",
                        "post_history_instructions": "",
                        "alternate_greetings": [],
                        "tags": [],
                        "creator": "",
                        "character_version": "",
                        "extensions": {}
                    }
                }),
            },
        )
        .await
        .expect_err("invalid V2 payload should fail");

    assert!(matches!(error, ApplicationError::ValidationError(_)));
    assert!(
        error.to_string().contains("data.system_prompt"),
        "unexpected error: {}",
        error
    );

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn import_character_with_embedded_world_preserves_unknown_fields_after_auto_import() {
    let (service, character_repository, _world_info_repository, root) = setup_service().await;

    let character_book = json!({
        "name": "Embedded Book",
        "extensions": {},
        "entries": [
            {
                "id": 1,
                "keys": ["alpha"],
                "content": "fresh lore",
                "enabled": true
            }
        ]
    });
    let card_payload = json!({
        "spec": "chara_card_v3",
        "spec_version": "3.0",
        "name": "Embedded Raw Import",
        "description": "desc",
        "first_mes": "hello",
        "x_custom_root": { "nested": true },
        "data": {
            "name": "Embedded Raw Import",
            "description": "desc",
            "first_mes": "hello",
            "character_book": character_book,
            "extensions": {
                "talkativeness": 0.5,
                "fav": false,
                "world": "",
                "depth_prompt": {
                    "prompt": "",
                    "depth": 4,
                    "role": "system"
                },
                "tavern_helper": {
                    "scripts": [
                        { "id": "script-1" }
                    ]
                }
            },
            "x_data_custom": 123
        }
    });
    let source_png = write_character_data_to_png(
        &build_minimal_png(),
        &serde_json::to_string(&card_payload).expect("serialize card"),
    )
    .expect("embed card in png");
    let import_path = root.join("embedded-raw-import.png");
    fs::write(&import_path, source_png)
        .await
        .expect("write import png");

    let imported = service
        .import_character(ImportCharacterDto {
            file_path: import_path.to_string_lossy().into_owned(),
            preserve_file_name: None,
        })
        .await
        .expect("import character with embedded world");

    let stored_name = imported.avatar.trim_end_matches(".png");
    let stored_json = character_repository
        .read_character_card_json(stored_name)
        .await
        .expect("read imported character");
    let stored_value: serde_json::Value =
        serde_json::from_str(&stored_json).expect("parse stored character");

    assert_eq!(
        stored_value.get("x_custom_root"),
        Some(&json!({ "nested": true }))
    );
    assert_eq!(
        stored_value.pointer("/data/x_data_custom"),
        Some(&json!(123))
    );
    assert_eq!(
        stored_value.pointer("/data/extensions/tavern_helper/scripts/0/id"),
        Some(&json!("script-1"))
    );
    assert_eq!(
        stored_value.pointer("/data/extensions/world"),
        Some(&json!("Embedded Book"))
    );
    assert_eq!(
        stored_value.pointer("/data/character_book/entries/0/content"),
        Some(&json!("fresh lore"))
    );

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn export_after_import_preserves_unknown_card_fields() {
    let (service, _character_repository, _world_info_repository, root) = setup_service().await;

    let card_payload = json!({
        "spec": "chara_card_v3",
        "spec_version": "3.0",
        "name": "Roundtrip Import Export",
        "description": "desc",
        "first_mes": "hello",
        "chat": "source-chat",
        "fav": true,
        "x_custom_root": { "nested": true },
        "data": {
            "name": "Roundtrip Import Export",
            "description": "desc",
            "first_mes": "hello",
            "extensions": {
                "talkativeness": 0.5,
                "fav": true,
                "world": "",
                "depth_prompt": {
                    "prompt": "",
                    "depth": 4,
                    "role": "system"
                },
                "tavern_helper": {
                    "scripts": [
                        { "id": "script-1" }
                    ]
                }
            },
            "x_data_custom": 123
        }
    });
    let source_png = write_character_data_to_png(
        &build_minimal_png(),
        &serde_json::to_string(&card_payload).expect("serialize card"),
    )
    .expect("embed card in png");
    let import_path = root.join("roundtrip-import-export.png");
    fs::write(&import_path, source_png)
        .await
        .expect("write import png");

    let imported = service
        .import_character(ImportCharacterDto {
            file_path: import_path.to_string_lossy().into_owned(),
            preserve_file_name: None,
        })
        .await
        .expect("import character");
    let stored_name = imported.avatar.trim_end_matches(".png").to_string();

    let exported_json = service
        .export_character_content(ExportCharacterContentDto {
            name: stored_name.clone(),
            format: "json".to_string(),
        })
        .await
        .expect("export imported character as json");
    let exported_json_value: serde_json::Value =
        serde_json::from_slice(&exported_json.data).expect("parse exported json");

    assert_eq!(
        exported_json_value.get("x_custom_root"),
        Some(&json!({ "nested": true }))
    );
    assert_eq!(
        exported_json_value.pointer("/data/x_data_custom"),
        Some(&json!(123))
    );
    assert_eq!(
        exported_json_value.pointer("/data/extensions/tavern_helper/scripts/0/id"),
        Some(&json!("script-1"))
    );
    assert!(exported_json_value.get("chat").is_none());
    assert_eq!(exported_json_value.get("fav"), Some(&json!(false)));
    assert_eq!(
        exported_json_value.pointer("/data/extensions/fav"),
        Some(&json!(false))
    );

    let exported_png = service
        .export_character_content(ExportCharacterContentDto {
            name: stored_name,
            format: "png".to_string(),
        })
        .await
        .expect("export imported character as png");
    let exported_png_json =
        read_character_data_from_png(&exported_png.data).expect("read exported png metadata");
    let exported_png_value: serde_json::Value =
        serde_json::from_str(&exported_png_json).expect("parse exported png json");

    assert_eq!(
        exported_png_value.get("x_custom_root"),
        Some(&json!({ "nested": true }))
    );
    assert_eq!(
        exported_png_value.pointer("/data/x_data_custom"),
        Some(&json!(123))
    );
    assert_eq!(
        exported_png_value.pointer("/data/extensions/tavern_helper/scripts/0/id"),
        Some(&json!("script-1"))
    );
    assert!(exported_png_value.get("chat").is_none());
    assert_eq!(exported_png_value.get("fav"), Some(&json!(false)));
    assert_eq!(
        exported_png_value.pointer("/data/extensions/fav"),
        Some(&json!(false))
    );

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn merge_character_card_data_succeeds_after_normal_bound_world_edit() {
    let (service, character_repository, world_info_repository, root) = setup_service().await;
    let embedded_book = save_bound_world(&world_info_repository, "bound-book").await;

    let source_card = json!({
        "spec": "chara_card_v3",
        "spec_version": "3.0",
        "name": "Bound Raw Merge",
        "description": "Before",
        "first_mes": "Hello",
        "data": {
            "name": "Bound Raw Merge",
            "description": "Before",
            "first_mes": "Hello",
            "character_book": embedded_book,
            "extensions": {
                "talkativeness": 0.5,
                "fav": false,
                "world": "bound-book",
                "depth_prompt": {
                    "prompt": "",
                    "depth": 4,
                    "role": "system"
                },
                "tavern_helper": {
                    "scripts": [
                        { "id": "script-1" }
                    ]
                }
            }
        }
    });
    write_character_png(&root, "Bound Raw Merge", &source_card).await;

    service
        .update_character_card_data(
            "Bound Raw Merge",
            UpdateCharacterCardDataDto {
                card_json: json!({
                    "spec": "chara_card_v2",
                    "spec_version": "2.0",
                    "name": "Bound Raw Merge",
                    "description": "Before",
                    "personality": "",
                    "scenario": "",
                    "first_mes": "Hello",
                    "mes_example": "",
                    "data": {
                        "name": "Bound Raw Merge",
                        "description": "Before",
                        "personality": "",
                        "scenario": "",
                        "first_mes": "Hello",
                        "mes_example": "",
                        "creator_notes": "",
                        "system_prompt": "",
                        "post_history_instructions": "",
                        "tags": [],
                        "creator": "",
                        "character_version": "",
                        "alternate_greetings": [],
                        "character_book": {
                            "name": "bound-book",
                            "entries": [
                                {
                                    "id": 1,
                                    "keys": ["alpha"],
                                    "content": "stale"
                                }
                            ]
                        },
                        "extensions": {
                            "talkativeness": 0.5,
                            "fav": false,
                            "world": "bound-book",
                            "depth_prompt": {
                                "prompt": "",
                                "depth": 4,
                                "role": "system"
                            },
                            "tavern_helper": {
                                "scripts": [
                                    { "id": "script-1" }
                                ]
                            }
                        }
                    }
                })
                .to_string(),
                avatar_path: None,
                crop: None,
            },
        )
        .await
        .expect("initial update should succeed");

    service
        .merge_character_card_data(
            "Bound Raw Merge",
            MergeCharacterCardDataDto {
                update: json!({
                    "description": "After Merge"
                }),
            },
        )
        .await
        .expect("merge after normal edit should succeed");

    let stored_json = character_repository
        .read_character_card_json("Bound Raw Merge")
        .await
        .expect("read merged character");
    let stored_value: serde_json::Value =
        serde_json::from_str(&stored_json).expect("parse merged character");
    assert_eq!(
        stored_value
            .get("description")
            .and_then(serde_json::Value::as_str),
        Some("After Merge")
    );
    assert_eq!(
        stored_value.pointer("/data/character_book/extensions"),
        Some(&json!({}))
    );
    assert_eq!(
        stored_value.pointer("/data/extensions/tavern_helper/scripts/0/id"),
        Some(&json!("script-1"))
    );

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn merge_character_card_data_succeeds_when_bound_world_is_missing() {
    let (service, character_repository, _world_info_repository, root) = setup_service().await;

    let card_payload = json!({
        "spec": "chara_card_v3",
        "spec_version": "3.0",
        "name": "Missing World Merge",
        "description": "Before",
        "first_mes": "Hello",
        "data": {
            "name": "Missing World Merge",
            "description": "Before",
            "first_mes": "Hello",
            "extensions": {
                "talkativeness": 0.5,
                "fav": false,
                "world": "missing-book",
                "depth_prompt": {
                    "prompt": "",
                    "depth": 4,
                    "role": "system"
                }
            }
        }
    });

    write_character_png(&root, "Missing World Merge", &card_payload).await;

    service
        .merge_character_card_data(
            "Missing World Merge",
            MergeCharacterCardDataDto {
                update: json!({
                    "description": "After Merge",
                }),
            },
        )
        .await
        .expect("merge should succeed even when bound world is missing");

    let stored_json = character_repository
        .read_character_card_json("Missing World Merge")
        .await
        .expect("read merged character");
    let stored_value: serde_json::Value =
        serde_json::from_str(&stored_json).expect("parse merged character");

    assert_eq!(
        stored_value
            .get("description")
            .and_then(serde_json::Value::as_str),
        Some("After Merge")
    );
    assert_eq!(
        stored_value
            .pointer("/data/extensions/world")
            .and_then(serde_json::Value::as_str),
        Some("missing-book")
    );

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn export_character_content_png_preserves_unknown_card_fields() {
    let (service, _character_repository, _world_info_repository, root) = setup_service().await;

    let card_payload = json!({
        "spec": "chara_card_v3",
        "spec_version": "3.0",
        "name": "Unknown Export PNG",
        "first_mes": "Hello",
        "chat": "private-chat-name",
        "fav": true,
        "creatorcomment": "legacy field",
        "x_custom_top": { "nested": true },
        "data": {
            "name": "Unknown Export PNG",
            "first_mes": "Hello",
            "extensions": {
                "talkativeness": 0.5,
                "fav": true,
                "world": "",
                "depth_prompt": {
                    "prompt": "",
                    "depth": 4,
                    "role": "system",
                },
            },
            "x_custom_data": 123,
        },
    });

    write_character_png(&root, "Unknown Export PNG", &card_payload).await;

    let exported = service
        .export_character_content(ExportCharacterContentDto {
            name: "Unknown Export PNG".to_string(),
            format: "png".to_string(),
        })
        .await
        .expect("export should succeed");

    let exported_json =
        read_character_data_from_png(&exported.data).expect("read exported png metadata");
    let exported_value: serde_json::Value =
        serde_json::from_str(&exported_json).expect("parse exported json");

    assert!(
        exported_value.get("x_custom_top").is_some(),
        "exported png should preserve unknown top-level fields"
    );
    assert!(
        exported_value.pointer("/data/x_custom_data").is_some(),
        "exported png should preserve unknown data fields"
    );
    assert!(
        exported_value.get("chat").is_none(),
        "chat should be removed from exported payload"
    );
    assert_eq!(
        exported_value.get("fav").and_then(|value| value.as_bool()),
        Some(false)
    );
    assert_eq!(
        exported_value
            .pointer("/data/extensions/fav")
            .and_then(|value| value.as_bool()),
        Some(false)
    );

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn create_character_persists_embedded_primary_lorebook() {
    let (service, character_repository, world_info_repository, root) = setup_service().await;
    save_bound_world(&world_info_repository, "bound-book").await;

    service
        .create_character(CreateCharacterDto {
            name: "Export Test".to_string(),
            description: "desc".to_string(),
            personality: "persona".to_string(),
            scenario: String::new(),
            first_mes: "hello".to_string(),
            mes_example: String::new(),
            creator: None,
            creator_notes: None,
            character_version: None,
            tags: None,
            talkativeness: Some(0.5),
            fav: Some(false),
            alternate_greetings: None,
            system_prompt: None,
            post_history_instructions: None,
            extensions: Some(json!({ "world": "bound-book" })),
        })
        .await
        .expect("create character");

    let stored = character_repository
        .find_by_name("Export Test")
        .await
        .expect("load stored character");
    assert_eq!(stored.data.extensions.world, "bound-book");
    assert_eq!(
        stored
            .data
            .character_book
            .as_ref()
            .and_then(|value| value.get("name")),
        Some(&json!("bound-book"))
    );
    assert_eq!(
        stored
            .data
            .character_book
            .as_ref()
            .and_then(|value| value.pointer("/entries/0/content")),
        Some(&json!("fresh"))
    );

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn create_character_requires_existing_primary_lorebook() {
    let (service, _character_repository, _world_info_repository, root) = setup_service().await;

    let error = service
        .create_character(CreateCharacterDto {
            name: "Missing World".to_string(),
            description: "desc".to_string(),
            personality: "persona".to_string(),
            scenario: String::new(),
            first_mes: "hello".to_string(),
            mes_example: String::new(),
            creator: None,
            creator_notes: None,
            character_version: None,
            tags: None,
            talkativeness: Some(0.5),
            fav: Some(false),
            alternate_greetings: None,
            system_prompt: None,
            post_history_instructions: None,
            extensions: Some(json!({ "world": "missing-book" })),
        })
        .await
        .expect_err("missing primary lorebook should fail");

    assert!(matches!(
        error,
        ApplicationError::NotFound(message) if message == "World info file missing-book doesn't exist"
    ));

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn export_character_content_materializes_bound_lorebook_for_stale_cards() {
    let (service, character_repository, world_info_repository, root) = setup_service().await;
    save_bound_world(&world_info_repository, "bound-book").await;

    let mut character = Character::new(
        "Stale Export".to_string(),
        "desc".to_string(),
        "persona".to_string(),
        "hello".to_string(),
    );
    character.data.extensions.world = "bound-book".to_string();
    character_repository
        .save(&character)
        .await
        .expect("save stale character");

    let exported = service
        .export_character_content(ExportCharacterContentDto {
            name: "Stale Export".to_string(),
            format: "json".to_string(),
        })
        .await
        .expect("export character content");
    let export_value: serde_json::Value =
        serde_json::from_slice(&exported.data).expect("parse export json");

    assert_eq!(
        export_value.pointer("/data/character_book/name"),
        Some(&json!("bound-book"))
    );
    assert_eq!(
        export_value.pointer("/data/character_book/extensions"),
        Some(&json!({}))
    );
    assert_eq!(
        export_value.pointer("/data/character_book/entries/0/content"),
        Some(&json!("fresh"))
    );

    let updated = service
        .update_character(
            "Stale Export",
            UpdateCharacterDto {
                name: None,
                chat: None,
                description: None,
                personality: None,
                scenario: None,
                first_mes: None,
                mes_example: None,
                creator: None,
                creator_notes: None,
                character_version: None,
                tags: None,
                talkativeness: None,
                fav: None,
                alternate_greetings: None,
                system_prompt: None,
                post_history_instructions: None,
                extensions: Some(json!({ "world": "" })),
            },
        )
        .await
        .expect("unbind world");

    assert_eq!(
        updated.extensions,
        Some(json!({
            "talkativeness": 0.5,
            "fav": false,
            "world": "",
            "depth_prompt": {
                "prompt": "",
                "depth": 4,
                "role": "system"
            }
        }))
    );

    character_repository
        .clear_cache()
        .await
        .expect("clear stale repository cache");
    let stored = character_repository
        .find_by_name("Stale Export")
        .await
        .expect("load updated character");
    assert!(stored.data.character_book.is_none());
    assert_eq!(stored.data.extensions.world, "");

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn export_character_uses_current_world_entries_without_mutating_source_card() {
    let (service, character_repository, world_info_repository, root) = setup_service().await;
    let _original_book =
        save_world_with_stale_original_data(&world_info_repository, "bound-book").await;

    let mut character = Character::new(
        "Export File".to_string(),
        "desc".to_string(),
        "persona".to_string(),
        "hello".to_string(),
    );
    character.data.extensions.world = "bound-book".to_string();
    character_repository
        .save(&character)
        .await
        .expect("save stale character");

    let export_path = root.join("exported.json");
    service
        .export_character(ExportCharacterDto {
            name: "Export File".to_string(),
            target_path: export_path.to_string_lossy().into_owned(),
        })
        .await
        .expect("export character");

    let exported_json = fs::read_to_string(&export_path)
        .await
        .expect("read exported json");
    let exported_value: serde_json::Value =
        serde_json::from_str(&exported_json).expect("parse exported json");
    assert_eq!(
        exported_value.pointer("/data/character_book/name"),
        Some(&json!("bound-book"))
    );
    assert_eq!(
        exported_value.pointer("/data/character_book/extensions"),
        Some(&json!({}))
    );
    assert_eq!(
        exported_value.pointer("/data/character_book/description"),
        Some(&json!("preserve me"))
    );
    assert_eq!(
        exported_value.pointer("/data/character_book/entries/0/id"),
        Some(&json!(7))
    );
    assert_eq!(
        exported_value.pointer("/data/character_book/entries/0/content"),
        Some(&json!("fresh"))
    );
    assert_eq!(
        exported_value.pointer("/data/character_book/entries/0/extensions/custom"),
        Some(&json!("value"))
    );

    character_repository
        .clear_cache()
        .await
        .expect("clear stale repository cache");
    let stored = character_repository
        .find_by_name("Export File")
        .await
        .expect("reload source character");
    assert!(stored.data.character_book.is_none());

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn update_avatar_materializes_bound_lorebook_into_written_card() {
    let (service, character_repository, world_info_repository, root) = setup_service().await;
    save_bound_world(&world_info_repository, "bound-book").await;

    let card_payload = json!({
        "spec": "chara_card_v3",
        "spec_version": "3.0",
        "name": "Avatar Export",
        "description": "desc",
        "personality": "persona",
        "first_mes": "hello",
        "x_custom_top": { "nested": true },
        "data": {
            "name": "Avatar Export",
            "description": "desc",
            "personality": "persona",
            "first_mes": "hello",
            "extensions": {
                "talkativeness": 0.5,
                "fav": false,
                "world": "bound-book",
                "depth_prompt": {
                    "prompt": "",
                    "depth": 4,
                    "role": "system"
                },
                "tavern_helper": {
                    "scripts": [
                        { "id": "script-1" }
                    ]
                }
            },
            "x_custom_data": 123
        }
    });
    write_character_png(&root, "Avatar Export", &card_payload).await;

    let avatar_path = root.join("replacement.png");
    fs::write(&avatar_path, build_minimal_png())
        .await
        .expect("write replacement avatar");

    service
        .update_avatar(UpdateAvatarDto {
            name: "Avatar Export".to_string(),
            avatar_path: avatar_path.to_string_lossy().into_owned(),
            crop: None,
        })
        .await
        .expect("update avatar");

    character_repository
        .clear_cache()
        .await
        .expect("clear stale repository cache");
    let stored = character_repository
        .find_by_name("Avatar Export")
        .await
        .expect("reload updated character");
    let stored_json = character_repository
        .read_character_card_json("Avatar Export")
        .await
        .expect("read updated character card");
    let stored_value: serde_json::Value =
        serde_json::from_str(&stored_json).expect("parse updated character card");
    assert_eq!(
        stored_value.get("x_custom_top"),
        Some(&json!({ "nested": true }))
    );
    assert_eq!(
        stored_value.pointer("/data/x_custom_data"),
        Some(&json!(123))
    );
    assert_eq!(
        stored_value.pointer("/data/extensions/tavern_helper/scripts/0/id"),
        Some(&json!("script-1"))
    );
    assert_eq!(
        stored
            .data
            .character_book
            .as_ref()
            .and_then(|value| value.get("name")),
        Some(&json!("bound-book"))
    );
    assert_eq!(
        stored
            .data
            .character_book
            .as_ref()
            .and_then(|value| value.pointer("/entries/0/content")),
        Some(&json!("fresh"))
    );

    let _ = fs::remove_dir_all(&root).await;
}
