use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use crc32fast::Hasher;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use rand::random;
use serde_json::{Value, json};
use tokio::fs;

use crate::png_card_metadata::{
    read_character_data_from_png, read_text_chunks_from_png, write_character_data_to_png,
};
use tt_adapter_storage_core::{
    FileChatRepository, chat_directory_identity::new_shared_chat_alias_store_for_user_dir,
};
use tt_domain::errors::DomainError;
use tt_domain::models::character::Character;
use tt_ports::repositories::character_repository::{
    CHARACTER_CREATE_WARNING_AVATAR_IMPORT_FAILED, CharacterRepository,
};

use super::FileCharacterRepository;

fn unique_temp_root() -> PathBuf {
    std::env::temp_dir().join(format!("tauritavern-character-import-{}", random::<u64>()))
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

fn build_distinct_png() -> Vec<u8> {
    let mut image = RgbaImage::new(2, 2);
    image.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
    image.put_pixel(1, 0, Rgba([0, 255, 0, 255]));
    image.put_pixel(0, 1, Rgba([0, 0, 255, 255]));
    image.put_pixel(1, 1, Rgba([255, 255, 0, 255]));

    let image = DynamicImage::ImageRgba8(image);
    let mut output = Vec::new();
    let mut cursor = Cursor::new(&mut output);
    image
        .write_to(&mut cursor, ImageFormat::Png)
        .expect("should build png image");
    output
}

fn build_text_chunk(keyword: &str, text: &str) -> Vec<u8> {
    let mut data = Vec::with_capacity(keyword.len() + 1 + text.len());
    data.extend_from_slice(keyword.as_bytes());
    data.push(0);
    data.extend_from_slice(text.as_bytes());

    let chunk_type = *b"tEXt";
    let mut chunk = Vec::with_capacity(data.len() + 12);
    chunk.extend_from_slice(&(data.len() as u32).to_be_bytes());
    chunk.extend_from_slice(&chunk_type);
    chunk.extend_from_slice(&data);

    let mut hasher = Hasher::new();
    hasher.update(&chunk_type);
    hasher.update(&data);
    chunk.extend_from_slice(&hasher.finalize().to_be_bytes());
    chunk
}

fn insert_text_chunk_before_iend(mut png: Vec<u8>, keyword: &str, text: &str) -> Vec<u8> {
    let iend_start = png
        .len()
        .checked_sub(12)
        .expect("minimal png should contain IEND");
    let text_chunk = build_text_chunk(keyword, text);
    png.splice(iend_start..iend_start, text_chunk);
    png
}

async fn repository_for_root(root: &Path) -> FileCharacterRepository {
    let characters_dir = root.join("characters");
    let chats_dir = root.join("chats");
    let default_avatar = root.join("default.png");

    fs::create_dir_all(&characters_dir)
        .await
        .expect("create characters dir");
    fs::create_dir_all(&chats_dir)
        .await
        .expect("create chats dir");
    fs::write(&default_avatar, build_minimal_png())
        .await
        .expect("write default avatar");

    let chat_aliases = new_shared_chat_alias_store_for_user_dir(root);
    let chat_repository = Arc::new(FileChatRepository::with_chat_aliases(
        characters_dir.clone(),
        chats_dir.clone(),
        root.join("group chats"),
        root.join("backups"),
        chat_aliases.clone(),
    ));

    FileCharacterRepository::with_chat_repository(
        characters_dir,
        chats_dir,
        default_avatar,
        chat_aliases,
        chat_repository,
    )
}

async fn setup_repository() -> (FileCharacterRepository, PathBuf) {
    let root = unique_temp_root();
    let repository = repository_for_root(&root).await;
    (repository, root)
}

async fn create_character(repository: &FileCharacterRepository, character: &Character) {
    repository
        .create_with_avatar(character, None, None)
        .await
        .expect("create character");
}

fn shallow_index_path(root: &Path) -> PathBuf {
    root.join("user")
        .join("cache")
        .join("character_shallow_index_v1.json")
}

fn chat_summary_index_path(root: &Path) -> PathBuf {
    root.join("user")
        .join("cache")
        .join("chat_summary_index_v1.json")
}

#[tokio::test]
async fn find_by_name_preserves_nonstandard_create_date() {
    let (repository, root) = setup_repository().await;

    let card_payload = json!({
        "name": "Invalid Date Character",
        "description": "desc",
        "personality": "persona",
        "first_mes": "hello",
        "create_date": "not-a-date",
    });

    let source_png = write_character_data_to_png(
        &build_minimal_png(),
        &serde_json::to_string(&card_payload).expect("serialize card"),
    )
    .expect("embed card in png");

    let character_path = root.join("characters").join("InvalidDate.png");
    fs::write(&character_path, source_png)
        .await
        .expect("write character png");

    let loaded = repository
        .find_by_name("InvalidDate")
        .await
        .expect("load character");

    assert_eq!(loaded.create_date, "not-a-date");

    let updated_png = fs::read(&character_path)
        .await
        .expect("read updated character png");
    let updated_json =
        read_character_data_from_png(&updated_png).expect("extract updated card json");
    let updated_value: serde_json::Value =
        serde_json::from_str(&updated_json).expect("parse updated card json");

    assert_eq!(
        updated_value
            .get("create_date")
            .and_then(|value| value.as_str()),
        Some("not-a-date")
    );

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn create_with_avatar_allocates_unique_file_stems() {
    let (repository, root) = setup_repository().await;

    let first = Character::new(
        "Duplicate".to_string(),
        "desc".to_string(),
        "persona".to_string(),
        "First greeting".to_string(),
    );
    let created_first = repository
        .create_with_avatar(&first, None, None)
        .await
        .expect("create first character")
        .character;

    let second = Character::new(
        "Duplicate".to_string(),
        "desc".to_string(),
        "persona".to_string(),
        "Second greeting".to_string(),
    );
    let created_second = repository
        .create_with_avatar(&second, None, None)
        .await
        .expect("create second character")
        .character;

    assert_eq!(created_first.avatar, "Duplicate.png");
    assert_eq!(created_second.avatar, "Duplicate1.png");

    let loaded_first = repository
        .find_by_name("Duplicate")
        .await
        .expect("load first character");
    let loaded_second = repository
        .find_by_name("Duplicate1")
        .await
        .expect("load second character");

    assert_eq!(loaded_first.first_mes, "First greeting");
    assert_eq!(loaded_second.first_mes, "Second greeting");

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn write_character_card_json_canonicalizes_dirty_metadata_chunks() {
    let (repository, root) = setup_repository().await;

    let character = Character::new(
        "Dirty Chunks".to_string(),
        "desc".to_string(),
        "persona".to_string(),
        "hello".to_string(),
    );
    create_character(&repository, &character).await;
    repository
        .find_all(true)
        .await
        .expect("create persistent shallow index");

    let character_path = root.join("characters").join("Dirty Chunks.png");
    let clean_png = fs::read(&character_path)
        .await
        .expect("read clean character png");
    let selected_json =
        read_character_data_from_png(&clean_png).expect("extract selected character data");
    let stale_json = r#"{"spec":"chara_card_v2","spec_version":"2.0","name":"Dirty Chunks","description":"stale"}"#;
    let dirty_png =
        insert_text_chunk_before_iend(clean_png, "chara", &BASE64.encode(stale_json.as_bytes()));
    fs::write(&character_path, dirty_png)
        .await
        .expect("write dirty metadata chunks");

    repository
        .write_character_card_json("Dirty Chunks", &selected_json, None, None)
        .await
        .expect("canonicalize dirty metadata");

    let rewritten_png = fs::read(&character_path)
        .await
        .expect("read rewritten character png");
    let character_chunks_count = read_text_chunks_from_png(&rewritten_png)
        .expect("read text metadata")
        .iter()
        .filter(|chunk| {
            chunk.keyword.eq_ignore_ascii_case("chara")
                || chunk.keyword.eq_ignore_ascii_case("ccv3")
        })
        .count();

    assert_eq!(character_chunks_count, 2);
    assert!(!shallow_index_path(&root).exists());

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn write_character_card_json_replaces_avatar_even_when_metadata_is_unchanged() {
    let (repository, root) = setup_repository().await;

    let character = Character::new(
        "Avatar Edit".to_string(),
        "desc".to_string(),
        "persona".to_string(),
        "hello".to_string(),
    );
    create_character(&repository, &character).await;
    repository
        .find_all(true)
        .await
        .expect("create persistent shallow index");

    let avatar_path = root.join("replacement-avatar.png");
    fs::write(&avatar_path, build_distinct_png())
        .await
        .expect("write replacement avatar");
    let card_json = serde_json::to_string(&character.to_v2()).expect("serialize card");

    repository
        .write_character_card_json("Avatar Edit", &card_json, Some(&avatar_path), None)
        .await
        .expect("replace avatar");

    let character_path = root.join("characters").join("Avatar Edit.png");
    let stored_png = fs::read(&character_path)
        .await
        .expect("read updated character png");
    let stored_image = image::load_from_memory(&stored_png).expect("decode stored avatar");

    assert_eq!(stored_image.width(), 2);
    assert_eq!(stored_image.height(), 2);
    assert!(!shallow_index_path(&root).exists());

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn create_with_avatar_sanitizes_file_stem_like_sillytavern() {
    let (repository, root) = setup_repository().await;

    let character = Character::new(
        "Unsafe/Name".to_string(),
        "desc".to_string(),
        "persona".to_string(),
        "Hi".to_string(),
    );
    let created = repository
        .create_with_avatar(&character, None, None)
        .await
        .expect("create character")
        .character;

    assert_eq!(created.avatar, "UnsafeName.png");

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn create_with_avatar_invalid_avatar_bytes_falls_back_to_default_avatar() {
    let (repository, root) = setup_repository().await;

    let invalid_avatar_path = root.join("invalid-upload.bin");
    fs::write(&invalid_avatar_path, b"not an image")
        .await
        .expect("write invalid avatar");

    let character = Character::new(
        "Invalid Avatar".to_string(),
        "desc".to_string(),
        "persona".to_string(),
        "hello".to_string(),
    );

    let result = repository
        .create_with_avatar(&character, Some(&invalid_avatar_path), None)
        .await
        .expect("create character with invalid avatar fallback");
    assert_eq!(result.warnings.len(), 1);
    assert_eq!(
        result.warnings[0].code,
        CHARACTER_CREATE_WARNING_AVATAR_IMPORT_FAILED
    );
    let created = result.character;

    let stored_path = root.join("characters").join(&created.avatar);
    let stored_bytes = fs::read(&stored_path)
        .await
        .expect("read stored character png");
    let stored_image = image::load_from_memory(&stored_bytes).expect("decode fallback avatar");
    assert_eq!(stored_image.width(), 1);
    assert_eq!(stored_image.height(), 1);

    let stored_json =
        read_character_data_from_png(&stored_bytes).expect("extract stored character data");
    let stored_value: serde_json::Value =
        serde_json::from_str(&stored_json).expect("parse stored character data");
    assert_eq!(
        stored_value.get("name").and_then(|value| value.as_str()),
        Some("Invalid Avatar")
    );

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn duplicate_copies_png_bytes_and_uses_upstream_suffix() {
    let (repository, root) = setup_repository().await;

    let card_payload = json!({
        "name": "Display Name",
        "description": "desc",
        "personality": "persona",
        "first_mes": "hello",
        "x_custom_root": { "keep": true },
        "data": {
            "name": "Display Name",
            "description": "desc",
            "personality": "persona",
            "first_mes": "hello",
            "extensions": {
                "world": "Shared Lore"
            }
        }
    });
    let source_png = write_character_data_to_png(
        &build_distinct_png(),
        &serde_json::to_string(&card_payload).expect("serialize card"),
    )
    .expect("embed card in png");

    let source_path = root.join("characters").join("Alice_1.png");
    let occupied_path = root.join("characters").join("Alice_2.png");
    fs::write(&source_path, &source_png)
        .await
        .expect("write source character png");
    fs::write(
        &occupied_path,
        write_character_data_to_png(
            &build_minimal_png(),
            &serde_json::to_string(&json!({ "name": "Occupied", "first_mes": "hi" }))
                .expect("serialize occupied card"),
        )
        .expect("embed occupied card"),
    )
    .await
    .expect("write occupied duplicate target");

    let duplicated = repository
        .duplicate("Alice_1")
        .await
        .expect("duplicate character");

    assert_eq!(duplicated.avatar, "Alice_3.png");
    assert_eq!(duplicated.file_name, Some("Alice_3".to_string()));

    let duplicated_path = root.join("characters").join("Alice_3.png");
    let duplicated_bytes = fs::read(&duplicated_path)
        .await
        .expect("read duplicated character png");
    assert_eq!(duplicated_bytes, source_png);

    let duplicated_json =
        read_character_data_from_png(&duplicated_bytes).expect("extract duplicated card json");
    let duplicated_value: serde_json::Value =
        serde_json::from_str(&duplicated_json).expect("parse duplicated card json");
    assert_eq!(
        duplicated_value["x_custom_root"]["keep"].as_bool(),
        Some(true)
    );

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn import_png_does_not_eagerly_create_chat_file() {
    let (repository, root) = setup_repository().await;

    let mut character = Character::new(
        "Test Character".to_string(),
        "desc".to_string(),
        "persona".to_string(),
        "Hello from import".to_string(),
    );
    character.chat = "Imported Chat".to_string();

    let source_png = write_character_data_to_png(
        &build_minimal_png(),
        &serde_json::to_string(&character.to_v2()).expect("serialize card"),
    )
    .expect("embed card in png");
    let import_path = root.join("upload.png");
    fs::write(&import_path, source_png)
        .await
        .expect("write import png");

    let imported = repository
        .import_character(&import_path, None)
        .await
        .expect("import png character");

    let character_id = imported.avatar.trim_end_matches(".png").to_string();
    let chat_path = root
        .join("chats")
        .join(character_id)
        .join(format!("{}.jsonl", imported.chat));

    assert!(
        !chat_path.exists(),
        "character import should not eagerly create chat files"
    );
    assert_eq!(imported.avatar, "Test Character.png");

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn import_json_uses_exact_preserved_file_name() {
    let (repository, root) = setup_repository().await;

    let character = Character::new(
        "Another Character".to_string(),
        "".to_string(),
        "".to_string(),
        "Hi".to_string(),
    );
    let import_path = root.join("upload.json");
    fs::write(
        &import_path,
        serde_json::to_vec(&character.to_v2()).expect("serialize json card"),
    )
    .await
    .expect("write import json");

    let imported = repository
        .import_character(&import_path, Some("Preserved.png".to_string()))
        .await
        .expect("import json character");

    assert_eq!(imported.avatar, "Preserved.png");
    assert!(root.join("characters").join("Preserved.png").exists());
    assert!(!root.join("characters").join("Preserved.png.png").exists());

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn replace_character_preserves_requested_primary_lorebook_in_single_import_write() {
    let (repository, root) = setup_repository().await;
    let mut old_character = Character::new(
        "Preserved".to_string(),
        "old description".to_string(),
        "old personality".to_string(),
        "old first message".to_string(),
    );
    old_character.data.extensions.set_world("Local Lore");
    create_character(&repository, &old_character).await;

    let mut replacement = Character::new(
        "Replacement".to_string(),
        "new description".to_string(),
        "new personality".to_string(),
        "new first message".to_string(),
    );
    replacement.data.extensions.set_world("Incoming Lore");
    let source_png = write_character_data_to_png(
        &build_distinct_png(),
        &serde_json::to_string(&replacement.to_v2()).expect("serialize replacement card"),
    )
    .expect("embed replacement card in png");
    let import_path = root.join("replacement.png");
    fs::write(&import_path, source_png)
        .await
        .expect("write replacement png");

    let imported = repository
        .replace_character(&import_path, "Preserved", Some("Local Lore"))
        .await
        .expect("replace character");
    assert_eq!(imported.avatar, "Preserved.png");
    assert_eq!(imported.name, "Replacement");
    assert_eq!(imported.data.extensions.world(), "Local Lore");

    let stored_json = repository
        .read_character_card_json("Preserved")
        .await
        .expect("read stored replacement card");
    let stored_value: Value = serde_json::from_str(&stored_json).expect("parse stored card");
    assert_eq!(
        stored_value.pointer("/data/extensions/world"),
        Some(&json!("Local Lore"))
    );

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn failed_replace_keeps_existing_character() {
    let (repository, root) = setup_repository().await;
    let character = Character::new(
        "Preserved".to_string(),
        "old description".to_string(),
        "old personality".to_string(),
        "old first message".to_string(),
    );
    create_character(&repository, &character).await;

    let import_path = root.join("invalid.png");
    fs::write(&import_path, b"not a character card")
        .await
        .expect("write invalid card");

    repository
        .replace_character(&import_path, "Preserved", None)
        .await
        .expect_err("invalid replacement must fail");

    let reloaded = repository
        .find_by_name("Preserved")
        .await
        .expect("reload existing character");
    assert_eq!(reloaded.first_mes, "old first message");

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn replace_character_rejects_non_segment_storage_identity() {
    let (repository, root) = setup_repository().await;
    let import_path = root.join("replacement.json");
    fs::write(&import_path, b"{}")
        .await
        .expect("write replacement");

    let error = repository
        .replace_character(&import_path, "../outside", None)
        .await
        .expect_err("path-like replacement identity must fail");

    assert!(matches!(error, DomainError::InvalidData(_)));
    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn import_png_preserves_unknown_card_fields() {
    let (repository, root) = setup_repository().await;

    let card_payload = json!({
        "spec": "chara_card_v3",
        "spec_version": "3.0",
        "name": "Unknown Import",
        "description": "desc",
        "personality": "persona",
        "scenario": "scenario",
        "first_mes": "hello",
        "mes_example": "",
        "creatorcomment": "legacy creator notes",
        "chat": "source-chat",
        "fav": true,
        "x_custom_root": { "nested": true },
        "x_list": [1, 2, 3],
        "x_string": "keep me",
        "unknown_root_array": [{ "id": 1 }],
        "data": {
            "name": "Unknown Import",
            "description": "desc",
            "personality": "persona",
            "scenario": "scenario",
            "first_mes": "hello",
            "mes_example": "",
            "creator_notes": "canonical notes",
            "system_prompt": "",
            "post_history_instructions": "",
            "tags": [],
            "creator": "tester",
            "character_version": "1.0",
            "alternate_greetings": [],
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
            "x_data_custom": { "answer": 42 }
        }
    });

    let source_png = write_character_data_to_png(
        &build_minimal_png(),
        &serde_json::to_string(&card_payload).expect("serialize card"),
    )
    .expect("embed card in png");
    let import_path = root.join("unknown-import.png");
    fs::write(&import_path, source_png)
        .await
        .expect("write import png");

    let imported = repository
        .import_character(&import_path, None)
        .await
        .expect("import png character");

    let stored_name = imported.avatar.trim_end_matches(".png");
    let stored_json = repository
        .read_character_card_json(stored_name)
        .await
        .expect("read stored character");
    let stored_value: serde_json::Value =
        serde_json::from_str(&stored_json).expect("parse stored character");

    assert_eq!(
        stored_value.get("x_custom_root"),
        Some(&json!({ "nested": true }))
    );
    assert_eq!(stored_value.get("x_list"), Some(&json!([1, 2, 3])));
    assert_eq!(stored_value.get("x_string"), Some(&json!("keep me")));
    assert_eq!(
        stored_value.get("unknown_root_array"),
        Some(&json!([{ "id": 1 }]))
    );
    assert_eq!(
        stored_value.get("creatorcomment"),
        Some(&json!("legacy creator notes"))
    );
    assert_eq!(
        stored_value.pointer("/data/x_data_custom"),
        Some(&json!({ "answer": 42 }))
    );
    assert_eq!(
        stored_value.pointer("/data/extensions/tavern_helper/scripts/0/id"),
        Some(&json!("script-1"))
    );
    assert_eq!(stored_value.get("fav"), Some(&json!(false)));
    assert_eq!(
        stored_value.pointer("/data/extensions/fav"),
        Some(&json!(false))
    );
    assert_ne!(stored_value.get("chat"), Some(&json!("source-chat")));

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn import_png_preserves_open_card_field_types() {
    let (repository, root) = setup_repository().await;
    let card_payload = json!({
        "spec": "chara_card_v2",
        "spec_version": "2.0",
        "data": {
            "name": "AICharED",
            "description": 7,
            "personality": null,
            "scenario": false,
            "first_mes": { "text": "hello" },
            "mes_example": ["example"],
            "creator_notes": 9,
            "system_prompt": ["system"],
            "post_history_instructions": { "kept": true },
            "creator": true,
            "character_version": 1,
            "alternate_greetings": "hello again",
            "group_only_greetings": null,
            "extensions": {
                "talkativeness": "not-a-number",
                "world": 42,
                "depth_prompt": {
                    "prompt": "",
                    "depth": "",
                    "role": "system"
                }
            }
        }
    });
    let source_png = write_character_data_to_png(
        &build_minimal_png(),
        &serde_json::to_string(&card_payload).expect("serialize card"),
    )
    .expect("embed card in png");
    let import_path = root.join("aichared.png");
    fs::write(&import_path, source_png)
        .await
        .expect("write import png");

    let imported = repository
        .import_character(&import_path, None)
        .await
        .expect("import open character card");
    let stored_json = repository
        .read_character_card_json(imported.avatar.trim_end_matches(".png"))
        .await
        .expect("read stored card");
    let stored: Value = serde_json::from_str(&stored_json).expect("parse stored card");

    for pointer in [
        "/data/description",
        "/data/personality",
        "/data/scenario",
        "/data/first_mes",
        "/data/mes_example",
        "/data/creator_notes",
        "/data/system_prompt",
        "/data/post_history_instructions",
        "/data/creator",
        "/data/character_version",
        "/data/alternate_greetings",
        "/data/group_only_greetings",
        "/data/extensions/talkativeness",
        "/data/extensions/world",
        "/data/extensions/depth_prompt/depth",
    ] {
        assert_eq!(
            stored.pointer(pointer),
            card_payload.pointer(pointer),
            "{pointer}"
        );
    }

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn import_json_preserves_unknown_card_fields() {
    let (repository, root) = setup_repository().await;

    let card_payload = json!({
        "spec": "chara_card_v3",
        "spec_version": "3.0",
        "name": "Unknown Json Import",
        "description": "desc",
        "first_mes": "hello",
        "x_custom_root": true,
        "data": {
            "name": "Unknown Json Import",
            "description": "desc",
            "first_mes": "hello",
            "extensions": {
                "talkativeness": 0.5,
                "fav": false,
                "tavern_helper": {
                    "enabled": true
                }
            },
            "x_data_custom": "data-value"
        }
    });

    let import_path = root.join("unknown-import.json");
    fs::write(
        &import_path,
        serde_json::to_vec(&card_payload).expect("serialize card"),
    )
    .await
    .expect("write import json");

    let imported = repository
        .import_character(&import_path, None)
        .await
        .expect("import json character");

    let stored_name = imported.avatar.trim_end_matches(".png");
    let stored_json = repository
        .read_character_card_json(stored_name)
        .await
        .expect("read stored character");
    let stored_value: serde_json::Value =
        serde_json::from_str(&stored_json).expect("parse stored character");

    assert_eq!(stored_value.get("x_custom_root"), Some(&json!(true)));
    assert_eq!(
        stored_value.pointer("/data/x_data_custom"),
        Some(&json!("data-value"))
    );
    assert_eq!(
        stored_value.pointer("/data/extensions/tavern_helper/enabled"),
        Some(&json!(true))
    );

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn import_legacy_json_persists_a_v2_fallback_without_dropping_unknown_fields() {
    let (repository, root) = setup_repository().await;
    let import_path = root.join("legacy.json");
    fs::write(
        &import_path,
        serde_json::to_vec(&json!({
            "name": "Legacy",
            "description": "desc",
            "personality": "persona",
            "scenario": "scenario",
            "first_mes": "hello",
            "mes_example": "",
            "custom": { "kept": true }
        }))
        .expect("serialize legacy card"),
    )
    .await
    .expect("write legacy card");

    let imported = repository
        .import_character(&import_path, None)
        .await
        .expect("import legacy card");
    let stored_png = fs::read(root.join("characters").join(&imported.avatar))
        .await
        .expect("read imported card");
    let v2_chunk = read_text_chunks_from_png(&stored_png)
        .expect("read imported card metadata")
        .into_iter()
        .find(|chunk| chunk.keyword.eq_ignore_ascii_case("chara"))
        .expect("find V2 metadata");
    let stored: Value = serde_json::from_slice(
        &BASE64
            .decode(v2_chunk.text)
            .expect("decode V2 card metadata"),
    )
    .expect("parse V2 card metadata");

    assert_eq!(stored.get("spec"), Some(&json!("chara_card_v2")));
    assert_eq!(stored.get("spec_version"), Some(&json!("2.0")));
    assert_eq!(stored.pointer("/data/name"), Some(&json!("Legacy")));
    assert_eq!(stored.pointer("/custom/kept"), Some(&json!(true)));

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn import_v3_uses_data_fields_when_top_level_is_stale() {
    let (repository, root) = setup_repository().await;

    let card_payload = json!({
        "spec": "chara_card_v3",
        "spec_version": "3.0",
        "name": "Stale Root Name",
        "description": "stale root desc",
        "personality": "stale root persona",
        "scenario": "stale root scenario",
        "first_mes": "stale root hello",
        "mes_example": "stale root example",
        "tags": ["root-tag"],
        "talkativeness": 0.1,
        "data": {
            "name": "Canonical Import",
            "description": "canonical desc",
            "personality": "canonical persona",
            "scenario": "canonical scenario",
            "first_mes": "canonical hello",
            "mes_example": "canonical example",
            "tags": ["data-tag"],
            "extensions": {
                "talkativeness": 0.8,
                "fav": false
            }
        }
    });

    let import_path = root.join("stale-root.json");
    fs::write(
        &import_path,
        serde_json::to_vec(&card_payload).expect("serialize card"),
    )
    .await
    .expect("write import json");

    let imported = repository
        .import_character(&import_path, None)
        .await
        .expect("import stale root character");

    assert_eq!(imported.name, "Canonical Import");
    assert_eq!(imported.description, "canonical desc");
    assert_eq!(imported.personality, "canonical persona");
    assert_eq!(imported.scenario, "canonical scenario");
    assert_eq!(imported.first_mes, "canonical hello");
    assert_eq!(imported.mes_example, "canonical example");
    assert_eq!(imported.tags, vec!["data-tag".to_string()]);
    assert_eq!(imported.talkativeness, 0.8);

    let stored_json = repository
        .read_character_card_json("Canonical Import")
        .await
        .expect("read stored character");
    let stored_value: serde_json::Value =
        serde_json::from_str(&stored_json).expect("parse stored character");

    assert_eq!(stored_value.get("name"), Some(&json!("Canonical Import")));
    assert_eq!(
        stored_value.get("description"),
        Some(&json!("canonical desc"))
    );
    assert_eq!(
        stored_value.pointer("/data/description"),
        Some(&json!("canonical desc"))
    );
    assert_eq!(stored_value.get("tags"), Some(&json!(["data-tag"])));
    assert_eq!(
        stored_value.pointer("/data/extensions/talkativeness"),
        Some(&json!(0.8))
    );

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn import_json_preserves_top_level_alternate_greetings_array() {
    let (repository, root) = setup_repository().await;

    let card_payload = json!({
        "name": "Legacy Greeting Character",
        "description": "desc",
        "personality": "persona",
        "first_mes": "Hello",
        "alternate_greetings": [
            "Hi there",
            "Howdy"
        ],
    });

    let import_path = root.join("legacy-alt-array.json");
    fs::write(
        &import_path,
        serde_json::to_vec(&card_payload).expect("serialize card"),
    )
    .await
    .expect("write import json");

    let imported = repository
        .import_character(&import_path, None)
        .await
        .expect("import json character");

    assert_eq!(
        imported.data.alternate_greetings,
        vec!["Hi there".to_string(), "Howdy".to_string()]
    );

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn import_json_with_only_alternate_greetings_keeps_payload_for_first_open() {
    let (repository, root) = setup_repository().await;

    let card_payload = json!({
        "name": "Alternate Only Character",
        "description": "desc",
        "personality": "persona",
        "first_mes": "",
        "alternate_greetings": ["Only Alt"],
    });

    let import_path = root.join("alternate-only.json");
    fs::write(
        &import_path,
        serde_json::to_vec(&card_payload).expect("serialize card"),
    )
    .await
    .expect("write import json");

    let imported = repository
        .import_character(&import_path, None)
        .await
        .expect("import json character");

    let character_id = imported.avatar.trim_end_matches(".png").to_string();
    let chat_path = root
        .join("chats")
        .join(character_id)
        .join(format!("{}.jsonl", imported.chat));

    assert_eq!(imported.first_mes, "");
    assert_eq!(
        imported.data.alternate_greetings,
        vec!["Only Alt".to_string()]
    );
    assert!(
        !chat_path.exists(),
        "character import should keep first-message selection for chat open flow"
    );

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn import_json_with_lone_surrogate_escape_sequence_succeeds() {
    let (repository, root) = setup_repository().await;

    let card_payload = r#"{
        "name": "Surrogate Character",
        "description": "desc",
        "personality": "persona",
        "first_mes": "Hello \uD83D"
    }"#;

    let import_path = root.join("surrogate.json");
    fs::write(&import_path, card_payload.as_bytes())
        .await
        .expect("write import json");

    let imported = repository
        .import_character(&import_path, None)
        .await
        .expect("import json character");

    assert_eq!(imported.first_mes, "Hello \u{FFFD}");
    assert_eq!(imported.data.first_mes, "Hello \u{FFFD}");

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn v2_data_metadata_is_canonical_for_full_and_shallow_reads() {
    let (repository, root) = setup_repository().await;

    let card_payload = json!({
        "spec": "chara_card_v2",
        "spec_version": "2.0",
        "name": "Metadata Target",
        "description": "root desc",
        "personality": "root persona",
        "scenario": "root scenario",
        "first_mes": "root hello",
        "mes_example": "root example",
        "creator": "root creator",
        "creator_notes": "root notes",
        "character_version": "1.0-root",
        "tags": ["root-tag"],
        "talkativeness": 0.1,
        "fav": true,
        "data": {
            "name": "Metadata Target",
            "description": "data desc",
            "personality": "data persona",
            "scenario": "data scenario",
            "first_mes": "data hello",
            "mes_example": "data example",
            "creator_notes": "data notes",
            "system_prompt": "",
            "post_history_instructions": "",
            "tags": ["data-tag"],
            "creator": "data creator",
            "character_version": "1.1-data",
            "alternate_greetings": [],
            "extensions": {
                "talkativeness": 0.8,
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

    let source_png = write_character_data_to_png(
        &build_minimal_png(),
        &serde_json::to_string(&card_payload).expect("serialize card"),
    )
    .expect("embed card in png");
    fs::write(
        root.join("characters").join("MetadataTarget.png"),
        source_png,
    )
    .await
    .expect("write character png");

    let full = repository
        .find_by_name("MetadataTarget")
        .await
        .expect("load full character");
    assert_eq!(full.description, "data desc");
    assert_eq!(full.personality, "data persona");
    assert_eq!(full.scenario, "data scenario");
    assert_eq!(full.first_mes, "data hello");
    assert_eq!(full.mes_example, "data example");
    assert_eq!(full.tags, vec!["data-tag".to_string()]);
    assert_eq!(full.talkativeness, 0.8);
    assert!(!full.fav);
    assert_eq!(full.creator, "data creator");
    assert_eq!(full.creator_notes, "data notes");
    assert_eq!(full.character_version, "1.1-data");

    let shallow = repository
        .find_all(true)
        .await
        .expect("load shallow character list");
    assert_eq!(shallow.len(), 1);
    assert_eq!(shallow[0].creator, "data creator");
    assert_eq!(shallow[0].data.creator, "data creator");
    assert_eq!(shallow[0].creator_notes, "data notes");
    assert_eq!(shallow[0].data.creator_notes, "data notes");
    assert_eq!(shallow[0].character_version, "1.1-data");
    assert_eq!(shallow[0].data.character_version, "1.1-data");
    assert_eq!(shallow[0].tags, vec!["data-tag".to_string()]);
    assert_eq!(shallow[0].talkativeness, 0.8);
    assert!(!shallow[0].fav);

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn missing_chat_identity_is_stable_and_incompatible_index_is_rebuilt() {
    let (repository, root) = setup_repository().await;

    let card_payload = json!({
        "name": "Stable Missing",
        "first_mes": "hello",
    });
    let source_png = write_character_data_to_png(
        &build_minimal_png(),
        &serde_json::to_string(&card_payload).expect("serialize card"),
    )
    .expect("embed card in png");
    fs::write(
        root.join("characters").join("StableMissing.png"),
        source_png,
    )
    .await
    .expect("write character png");

    let shallow = repository
        .find_all(true)
        .await
        .expect("load shallow character list");
    let full = repository
        .find_by_name("StableMissing")
        .await
        .expect("load full character");
    let reopened = repository_for_root(&root)
        .await
        .find_by_name("StableMissing")
        .await
        .expect("reload full character");

    let index_path = shallow_index_path(&root);
    let mut stale_index: Value = serde_json::from_slice(
        &fs::read(&index_path)
            .await
            .expect("read persistent shallow index"),
    )
    .expect("parse persistent shallow index");
    stale_index["schema_version"] = json!(1);
    stale_index["entries"][0]["character"]["chat"] = json!("drifted chat");
    fs::write(
        &index_path,
        serde_json::to_vec(&stale_index).expect("serialize stale shallow index"),
    )
    .await
    .expect("write stale shallow index");
    let rebuilt = repository_for_root(&root)
        .await
        .find_all(true)
        .await
        .expect("rebuild stale shallow index");

    assert_eq!(shallow[0].chat, "Stable Missing - chat");
    assert_eq!(full.chat, shallow[0].chat);
    assert_eq!(reopened.chat, full.chat);
    assert_eq!(rebuilt[0].chat, full.chat);

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn legacy_cards_get_data_size_after_normalization() {
    let (repository, root) = setup_repository().await;

    let card_payload = json!({
        "name": "Legacy Size",
        "description": "desc",
        "personality": "persona",
        "first_mes": "hello",
        "tags": ["x", "😀"],
    });
    let source_png = write_character_data_to_png(
        &build_minimal_png(),
        &serde_json::to_string(&card_payload).expect("serialize card"),
    )
    .expect("embed card in png");
    fs::write(root.join("characters").join("LegacySize.png"), source_png)
        .await
        .expect("write character png");

    let full = repository
        .find_by_name("LegacySize")
        .await
        .expect("load full character");
    assert!(full.data_size > 0);

    let shallow = repository
        .find_all(true)
        .await
        .expect("load shallow character list");
    assert_eq!(shallow.len(), 1);
    assert_eq!(shallow[0].data_size, full.data_size);

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn unreadable_chat_statistics_do_not_hide_the_character() {
    let (repository, root) = setup_repository().await;
    let character = Character::new(
        "Stats Fallback".to_string(),
        "desc".to_string(),
        "persona".to_string(),
        "hello".to_string(),
    );
    create_character(&repository, &character).await;
    let chat_dir = root.join("chats").join("Stats Fallback");
    fs::create_dir_all(&chat_dir)
        .await
        .expect("create chat directory");
    fs::write(chat_dir.join("broken.jsonl"), [0xff])
        .await
        .expect("write invalid UTF-8 chat");

    let reopened = repository_for_root(&root).await;
    let shallow = reopened
        .find_all(true)
        .await
        .expect("list character despite unreadable chat statistics");
    assert_eq!(shallow.len(), 1);
    assert_eq!(shallow[0].avatar, "Stats Fallback.png");
    assert_eq!((shallow[0].chat_size, shallow[0].date_last_chat), (0, 0));

    let full = reopened
        .find_by_name("Stats Fallback")
        .await
        .expect("read character despite unreadable chat statistics");
    assert_eq!((full.chat_size, full.date_last_chat), (0, 0));

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn unreadable_character_does_not_restore_the_entire_stale_shallow_index() {
    let (repository, root) = setup_repository().await;
    for name in ["Alice", "Broken"] {
        create_character(
            &repository,
            &Character::new(
                name.to_string(),
                "desc".to_string(),
                "persona".to_string(),
                "hello".to_string(),
            ),
        )
        .await;
    }
    repository
        .find_all(true)
        .await
        .expect("build initial shallow index");

    fs::write(root.join("characters/Broken.png"), b"not a png")
        .await
        .expect("corrupt one character");
    let added = json!({ "name": "Added", "first_mes": "hello" });
    fs::write(
        root.join("characters/Added.png"),
        write_character_data_to_png(
            &build_minimal_png(),
            &serde_json::to_string(&added).expect("serialize added card"),
        )
        .expect("build added card"),
    )
    .await
    .expect("write added character");

    let avatars: Vec<_> = repository
        .find_all(true)
        .await
        .expect("rebuild partial shallow index")
        .into_iter()
        .map(|character| character.avatar)
        .collect();
    assert_eq!(avatars, vec!["Added.png", "Alice.png"]);

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn rename_sanitizes_target_file_name_and_moves_chat_directory() {
    let (repository, root) = setup_repository().await;

    let character = Character::new(
        "Source".to_string(),
        "desc".to_string(),
        "persona".to_string(),
        "hello".to_string(),
    );
    create_character(&repository, &character).await;

    let old_chat_dir = root.join("chats").join("Source");
    fs::create_dir_all(&old_chat_dir)
        .await
        .expect("create old chat directory");
    fs::write(old_chat_dir.join("session.jsonl"), b"{}\n")
        .await
        .expect("write chat file");

    let renamed = repository
        .rename("Source", "Renamed:/Name")
        .await
        .expect("rename character");

    assert_eq!(renamed.name, "Renamed:/Name");
    assert_eq!(renamed.avatar, "RenamedName.png");
    assert!(root.join("characters").join("RenamedName.png").exists());
    assert!(!root.join("characters").join("Source.png").exists());
    assert!(root.join("chats").join("RenamedName").exists());
    assert!(!root.join("chats").join("Source").exists());

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn character_chat_listing_reads_legacy_alias_directory() {
    let (repository, root) = setup_repository().await;

    let character = Character::new(
        "Alice#1".to_string(),
        "desc".to_string(),
        "persona".to_string(),
        "hello".to_string(),
    );
    create_character(&repository, &character).await;

    let legacy_chat_dir = root.join("chats").join("Alice");
    fs::create_dir_all(&legacy_chat_dir)
        .await
        .expect("create legacy chat directory");
    fs::write(
        legacy_chat_dir.join("session.jsonl"),
        b"{\"chat_metadata\":{}}\n{\"mes\":\"hello\",\"send_date\":\"2026-01-01T00:00:00.000Z\"}\n",
    )
    .await
    .expect("write legacy chat file");

    let chats = repository
        .get_character_chats("Alice#1", false)
        .await
        .expect("list legacy character chats");
    assert_eq!(chats.len(), 1);
    assert_eq!(chats[0].file_name, "session.jsonl");
    assert_eq!(chats[0].last_message, "hello");

    repository
        .clear_cache()
        .await
        .expect("clear character cache");
    let characters = repository
        .find_all(true)
        .await
        .expect("list shallow characters");
    let alice = characters
        .iter()
        .find(|character| character.avatar == "Alice#1.png")
        .expect("find exact character");
    assert!(alice.chat_size > 0);
    assert!(alice.date_last_chat > 0);

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn rename_moves_legacy_alias_chat_directory_to_new_canonical_dir() {
    let (repository, root) = setup_repository().await;

    let character = Character::new(
        "Alice#1".to_string(),
        "desc".to_string(),
        "persona".to_string(),
        "hello".to_string(),
    );
    create_character(&repository, &character).await;

    let legacy_chat_dir = root.join("chats").join("Alice");
    fs::create_dir_all(&legacy_chat_dir)
        .await
        .expect("create legacy chat directory");
    fs::write(
        legacy_chat_dir.join("session.jsonl"),
        b"{}\n{\"mes\":\"cached before rename\",\"send_date\":\"2026-01-01T00:00:00.000Z\"}\n",
    )
    .await
    .expect("write legacy chat file");
    let listed = repository
        .get_character_chats("Alice#1", false)
        .await
        .expect("build shared summary cache");
    assert_eq!(listed.len(), 1);
    assert!(chat_summary_index_path(&root).exists());

    let renamed = repository
        .rename("Alice#1", "Renamed")
        .await
        .expect("rename character");

    assert_eq!(renamed.avatar, "Renamed.png");
    assert!(
        root.join("chats")
            .join("Renamed")
            .join("session.jsonl")
            .exists()
    );
    assert!(!legacy_chat_dir.exists());
    let index_text = fs::read_to_string(chat_summary_index_path(&root))
        .await
        .expect("read chat summary index after rename");
    let index_json: Value =
        serde_json::from_str(&index_text).expect("parse chat summary index after rename");
    assert!(
        index_json
            .get("entries")
            .and_then(Value::as_array)
            .expect("summary entries array")
            .is_empty()
    );
    assert!(!index_text.contains("cached before rename"));

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn delete_with_chats_removes_legacy_alias_chat_directory() {
    let (repository, root) = setup_repository().await;

    let character = Character::new(
        "Alice#1".to_string(),
        "desc".to_string(),
        "persona".to_string(),
        "hello".to_string(),
    );
    create_character(&repository, &character).await;

    let legacy_chat_dir = root.join("chats").join("Alice");
    fs::create_dir_all(&legacy_chat_dir)
        .await
        .expect("create legacy chat directory");
    fs::write(
        legacy_chat_dir.join("session.jsonl"),
        b"{}\n{\"mes\":\"cached before delete\",\"send_date\":\"2026-01-01T00:00:00.000Z\"}\n",
    )
    .await
    .expect("write legacy chat file");
    let listed = repository
        .get_character_chats("Alice#1", false)
        .await
        .expect("build shared summary cache");
    assert_eq!(listed.len(), 1);
    assert!(chat_summary_index_path(&root).exists());

    repository
        .delete("Alice#1", true)
        .await
        .expect("delete exact character and chats");

    assert!(!root.join("characters").join("Alice#1.png").exists());
    assert!(!legacy_chat_dir.exists());
    let index_text = fs::read_to_string(chat_summary_index_path(&root))
        .await
        .expect("read chat summary index after delete");
    let index_json: Value =
        serde_json::from_str(&index_text).expect("parse chat summary index after delete");
    assert!(
        index_json
            .get("entries")
            .and_then(Value::as_array)
            .expect("summary entries array")
            .is_empty()
    );
    assert!(!index_text.contains("cached before delete"));

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn rename_uses_next_available_file_stem_when_target_exists() {
    let (repository, root) = setup_repository().await;

    let source = Character::new(
        "Source".to_string(),
        "desc".to_string(),
        "persona".to_string(),
        "hello".to_string(),
    );
    create_character(&repository, &source).await;

    let existing = Character::new(
        "Taken".to_string(),
        "desc".to_string(),
        "persona".to_string(),
        "hello".to_string(),
    );
    create_character(&repository, &existing).await;

    let renamed = repository
        .rename("Source", "Taken")
        .await
        .expect("rename character with conflict");

    assert_eq!(renamed.name, "Taken");
    assert_eq!(renamed.avatar, "Taken1.png");
    assert!(root.join("characters").join("Taken.png").exists());
    assert!(root.join("characters").join("Taken1.png").exists());
    assert!(!root.join("characters").join("Source.png").exists());

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn rename_preserves_avatar_pixel_data() {
    let (repository, root) = setup_repository().await;

    let avatar_path = root.join("custom.png");
    fs::write(&avatar_path, build_distinct_png())
        .await
        .expect("write custom avatar png");

    let character = Character::new(
        "Original".to_string(),
        "desc".to_string(),
        "persona".to_string(),
        "hello".to_string(),
    );

    let created = repository
        .create_with_avatar(&character, Some(&avatar_path), None)
        .await
        .expect("create character with avatar")
        .character;

    let old_file_path = root.join("characters").join(&created.avatar);
    let old_bytes = fs::read(&old_file_path)
        .await
        .expect("read old character file");

    let renamed = repository
        .rename("Original", "Renamed")
        .await
        .expect("rename character");

    let new_file_path = root.join("characters").join(&renamed.avatar);
    let new_bytes = fs::read(&new_file_path)
        .await
        .expect("read renamed character file");

    let old_image = image::load_from_memory(&old_bytes).expect("decode old avatar png");
    let new_image = image::load_from_memory(&new_bytes).expect("decode renamed avatar png");
    assert_eq!(old_image.to_rgba8(), new_image.to_rgba8());

    assert!(!old_file_path.exists());

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn write_character_card_json_rejects_invalid_avatar_bytes() {
    let (repository, root) = setup_repository().await;

    let character = Character::new(
        "Strict Avatar Target".to_string(),
        "desc".to_string(),
        "personality".to_string(),
        "hello".to_string(),
    );
    create_character(&repository, &character).await;
    let card_json = repository
        .read_character_card_json("Strict Avatar Target")
        .await
        .expect("read character card JSON");

    let invalid_avatar_path = root.join("invalid-replacement.bin");
    fs::write(&invalid_avatar_path, b"not an image")
        .await
        .expect("write invalid avatar");

    let result = repository
        .write_character_card_json(
            "Strict Avatar Target",
            &card_json,
            Some(&invalid_avatar_path),
            None,
        )
        .await;

    assert!(result.is_err(), "invalid avatar replacement should fail");

    let _ = fs::remove_dir_all(&root).await;
}
