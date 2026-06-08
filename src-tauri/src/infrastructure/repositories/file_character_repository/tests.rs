use chrono::DateTime;
use crc32fast::Hasher;
use std::io::Cursor;
use std::path::PathBuf;

use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use rand::random;
use serde_json::json;
use tokio::fs;

use crate::domain::models::character::Character;
use crate::domain::repositories::character_repository::{
    CHARACTER_CREATE_WARNING_AVATAR_IMPORT_FAILED, CharacterRepository,
};
use crate::infrastructure::persistence::png_utils::{
    read_character_data_from_png, read_text_chunks_from_png, write_character_data_to_png,
};
use crate::infrastructure::repositories::chat_directory_identity::new_shared_chat_alias_store_for_user_dir;

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

async fn setup_repository() -> (FileCharacterRepository, PathBuf) {
    let root = unique_temp_root();
    let characters_dir = root.join("characters");
    let chats_dir = root.join("chats");
    let thumbnails_avatar_dir = root.join("thumbnails/avatar");
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
    fs::write(&default_avatar, build_minimal_png())
        .await
        .expect("write default avatar");

    let repository = FileCharacterRepository::with_chat_aliases(
        characters_dir,
        chats_dir,
        thumbnails_avatar_dir,
        default_avatar,
        new_shared_chat_alias_store_for_user_dir(&root),
    );
    (repository, root)
}

#[tokio::test]
async fn find_by_name_repairs_invalid_create_date_and_persists_patch() {
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
        .expect("load repaired character");

    assert_ne!(loaded.create_date, "not-a-date");
    assert!(
        DateTime::parse_from_rfc3339(&loaded.create_date).is_ok(),
        "expected repaired create_date to be RFC3339"
    );

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
        Some(loaded.create_date.as_str())
    );

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn find_by_name_repairs_legacy_utc_create_date_format() {
    let (repository, root) = setup_repository().await;

    let card_payload = json!({
        "name": "Legacy Date Character",
        "description": "desc",
        "personality": "persona",
        "first_mes": "hello",
        "create_date": "2026-03-16 12:34:56 UTC",
    });

    let source_png = write_character_data_to_png(
        &build_minimal_png(),
        &serde_json::to_string(&card_payload).expect("serialize card"),
    )
    .expect("embed card in png");

    let character_path = root.join("characters").join("LegacyDate.png");
    fs::write(&character_path, source_png)
        .await
        .expect("write character png");

    let loaded = repository
        .find_by_name("LegacyDate")
        .await
        .expect("load repaired character");

    assert_eq!(loaded.create_date, "2026-03-16T12:34:56.000Z");

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
        Some("2026-03-16T12:34:56.000Z")
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
async fn create_with_avatar_prefers_explicit_file_stem() {
    let (repository, root) = setup_repository().await;

    let mut character = Character::new(
        "Display Name".to_string(),
        "desc".to_string(),
        "persona".to_string(),
        "Hi".to_string(),
    );
    character.file_name = Some("Permanent Assistant".to_string());

    let created = repository
        .create_with_avatar(&character, None, None)
        .await
        .expect("create character")
        .character;

    assert_eq!(created.avatar, "Permanent Assistant.png");
    assert_eq!(created.file_name, Some("Permanent Assistant".to_string()));

    let loaded = repository
        .find_by_name("Permanent Assistant")
        .await
        .expect("load character by file stem");
    assert_eq!(loaded.name, "Display Name");

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn create_with_avatar_missing_avatar_file_falls_back_to_default_avatar() {
    let (repository, root) = setup_repository().await;

    let character = Character::new(
        "Missing Avatar".to_string(),
        "desc".to_string(),
        "persona".to_string(),
        "hello".to_string(),
    );
    let missing_avatar_path = root.join("missing-upload.png");

    let result = repository
        .create_with_avatar(&character, Some(&missing_avatar_path), None)
        .await
        .expect("create character with default avatar fallback");
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
        Some("Missing Avatar")
    );

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
async fn create_with_avatar_png_without_crop_preserves_png_metadata_fast_path() {
    let (repository, root) = setup_repository().await;

    let avatar_path = root.join("metadata-avatar.png");
    fs::write(
        &avatar_path,
        insert_text_chunk_before_iend(build_distinct_png(), "tauritavern-fast-path", "preserve me"),
    )
    .await
    .expect("write metadata avatar");

    let character = Character::new(
        "Fast Path Avatar".to_string(),
        "desc".to_string(),
        "persona".to_string(),
        "hello".to_string(),
    );

    let result = repository
        .create_with_avatar(&character, Some(&avatar_path), None)
        .await
        .expect("create character with png fast path");
    assert!(result.warnings.is_empty());

    let stored_path = root.join("characters").join(&result.character.avatar);
    let stored_bytes = fs::read(&stored_path)
        .await
        .expect("read stored character png");
    let text_chunks = read_text_chunks_from_png(&stored_bytes).expect("read png text chunks");

    assert!(
        text_chunks.iter().any(|chunk| {
            chunk.keyword == "tauritavern-fast-path" && chunk.text == "preserve me"
        })
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
async fn import_json_normalizes_preserved_file_name() {
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
async fn import_png_updates_create_date_without_dropping_unknown_fields() {
    let (repository, root) = setup_repository().await;

    let source_create_date = "2000-01-02T03:04:05.006Z";
    let card_payload = json!({
        "spec": "chara_card_v3",
        "spec_version": "3.0",
        "name": "Import Date Refresh",
        "description": "desc",
        "first_mes": "hello",
        "create_date": source_create_date,
        "x_custom_root": { "survives": true },
        "data": {
            "name": "Import Date Refresh",
            "description": "desc",
            "first_mes": "hello",
            "extensions": {
                "talkativeness": 0.5,
                "fav": false,
                "tavern_helper": {
                    "script": "keep"
                }
            },
            "x_data_custom": "keep"
        }
    });

    let source_png = write_character_data_to_png(
        &build_minimal_png(),
        &serde_json::to_string(&card_payload).expect("serialize card"),
    )
    .expect("embed card in png");
    let import_path = root.join("date-refresh.png");
    fs::write(&import_path, source_png)
        .await
        .expect("write import png");

    let imported = repository
        .import_character(&import_path, None)
        .await
        .expect("import png character");

    assert_ne!(imported.create_date, source_create_date);
    assert!(
        DateTime::parse_from_rfc3339(&imported.create_date).is_ok(),
        "import create_date should be RFC3339"
    );

    let stored_name = imported.avatar.trim_end_matches(".png");
    let stored_json = repository
        .read_character_card_json(stored_name)
        .await
        .expect("read stored character");
    let stored_value: serde_json::Value =
        serde_json::from_str(&stored_json).expect("parse stored character");

    assert_eq!(
        stored_value
            .get("create_date")
            .and_then(|value| value.as_str()),
        Some(imported.create_date.as_str())
    );
    assert_eq!(
        stored_value.pointer("/x_custom_root/survives"),
        Some(&json!(true))
    );
    assert_eq!(
        stored_value.pointer("/data/x_data_custom"),
        Some(&json!("keep"))
    );
    assert_eq!(
        stored_value.pointer("/data/extensions/tavern_helper/script"),
        Some(&json!("keep"))
    );

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
async fn import_png_uses_data_description_when_top_level_is_empty() {
    let (repository, root) = setup_repository().await;

    let card_payload = json!({
        "spec": "chara_card_v3",
        "spec_version": "3.0",
        "name": "Data Fallback Character",
        "description": "",
        "data": {
            "name": "Data Fallback Character",
            "description": "Description from data field",
            "first_mes": "Hello",
            "extensions": {
                "talkativeness": 0.5,
                "fav": false,
            },
        },
    });

    let source_png = write_character_data_to_png(
        &build_minimal_png(),
        &serde_json::to_string(&card_payload).expect("serialize card"),
    )
    .expect("embed card in png");

    let import_path = root.join("data-fallback.png");
    fs::write(&import_path, source_png)
        .await
        .expect("write import png");

    let imported = repository
        .import_character(&import_path, None)
        .await
        .expect("import png character");

    assert_eq!(imported.description, "Description from data field");
    assert_eq!(imported.data.description, "Description from data field");

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
async fn import_json_preserves_top_level_alternate_greetings_string() {
    let (repository, root) = setup_repository().await;

    let card_payload = json!({
        "name": "Legacy Greeting String Character",
        "description": "desc",
        "personality": "persona",
        "first_mes": "Hello",
        "alternate_greetings": "Hello, traveler",
    });

    let import_path = root.join("legacy-alt-string.json");
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
        vec!["Hello, traveler".to_string()]
    );

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn import_json_with_alternate_greetings_does_not_create_initial_chat_file() {
    let (repository, root) = setup_repository().await;

    let card_payload = json!({
        "name": "No Eager Chat Character",
        "description": "desc",
        "personality": "persona",
        "first_mes": "Primary greeting",
        "alternate_greetings": ["Alt A", "Alt B"],
    });

    let import_path = root.join("no-eager-chat.json");
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

    assert_eq!(
        imported.data.alternate_greetings,
        vec!["Alt A".to_string(), "Alt B".to_string()]
    );
    assert!(
        !chat_path.exists(),
        "character import should not write initial chat payload"
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
async fn import_json_with_valid_surrogate_pair_preserves_emoji() {
    let (repository, root) = setup_repository().await;

    let card_payload = r#"{
        "name": "Emoji Character",
        "description": "desc",
        "personality": "persona",
        "first_mes": "Hello \uD83D\uDE00"
    }"#;

    let import_path = root.join("emoji.json");
    fs::write(&import_path, card_payload.as_bytes())
        .await
        .expect("write import json");

    let imported = repository
        .import_character(&import_path, None)
        .await
        .expect("import json character");

    assert_eq!(imported.first_mes, "Hello 😀");
    assert_eq!(imported.data.first_mes, "Hello 😀");

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn save_character_cache_exposes_real_avatar_file_name() {
    let (repository, root) = setup_repository().await;

    let character = Character::new(
        "Invalid:Name".to_string(),
        "desc".to_string(),
        "persona".to_string(),
        "hello".to_string(),
    );

    repository.save(&character).await.expect("save character");

    let loaded = repository
        .find_all(false)
        .await
        .expect("load characters from cache-backed list");
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].avatar, "InvalidName.png");

    assert!(root.join("characters").join("InvalidName.png").exists());

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn list_avatar_filenames_uses_directory_entries_without_card_parsing() {
    let (repository, root) = setup_repository().await;

    fs::write(root.join("characters").join("Broken.png"), b"not a card")
        .await
        .expect("write placeholder png");
    fs::write(root.join("characters").join("Notes.json"), b"{}")
        .await
        .expect("write non-character file");

    let mut avatars = repository
        .list_avatar_filenames()
        .await
        .expect("list avatar filenames");
    avatars.sort();

    assert_eq!(avatars, vec!["Broken.png".to_string()]);

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn find_all_shallow_preserves_runtime_fields_and_omits_character_book() {
    let (repository, root) = setup_repository().await;

    let mut character = Character::new(
        "Shallow Target".to_string(),
        "very long description".to_string(),
        "very long personality".to_string(),
        "hello there".to_string(),
    );
    character.scenario = "scenario".to_string();
    character.mes_example = "example".to_string();
    character.creator = "tester".to_string();
    character.creator_notes = "notes".to_string();
    character.character_version = "1.0".to_string();
    character.tags = vec!["tag-a".to_string(), "tag-b".to_string()];
    character.fav = true;
    character.talkativeness = 0.7;
    character.data.system_prompt = "system".to_string();
    character.data.post_history_instructions = "post-history".to_string();
    character.data.alternate_greetings = vec!["alt".to_string()];
    character.data.extensions.world = "world".to_string();
    character
        .data
        .extensions
        .additional
        .insert("regex_scripts".to_string(), json!(["rule"]));
    character.data.character_book = Some(json!({
        "entries": [
            { "id": 1, "content": "book-entry" }
        ]
    }));

    repository.save(&character).await.expect("save character");

    let characters = repository
        .find_all(true)
        .await
        .expect("load shallow characters");
    assert_eq!(characters.len(), 1);

    let shallow = &characters[0];
    assert!(shallow.shallow, "expected shallow projection");
    assert_eq!(shallow.name, "Shallow Target");
    assert_eq!(shallow.avatar, "Shallow Target.png");
    assert_eq!(shallow.creator, "tester");
    assert_eq!(shallow.creator_notes, "notes");
    assert_eq!(shallow.tags, vec!["tag-a".to_string(), "tag-b".to_string()]);
    assert!(shallow.fav);
    assert_eq!(shallow.talkativeness, 0.7);

    assert!(shallow.description.is_empty());
    assert!(shallow.personality.is_empty());
    assert!(shallow.scenario.is_empty());
    assert!(shallow.first_mes.is_empty());
    assert!(shallow.mes_example.is_empty());
    assert!(shallow.data.system_prompt.is_empty());
    assert!(shallow.data.post_history_instructions.is_empty());
    assert!(shallow.data.alternate_greetings.is_empty());
    assert!(shallow.data.extensions.world.is_empty());
    assert!(shallow.data.extensions.additional.is_empty());
    assert!(shallow.data.character_book.is_none());

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
async fn find_by_name_promotes_cached_shallow_character_to_full() {
    let (repository, root) = setup_repository().await;

    let mut character = Character::new(
        "cache_promotion".to_string(),
        "desc".to_string(),
        "persona".to_string(),
        "hello".to_string(),
    );
    character.data.character_book = Some(json!({
        "entries": [
            { "id": 1, "content": "keep me" }
        ]
    }));
    character.data.system_prompt = "system".to_string();
    character.data.alternate_greetings = vec!["alt".to_string()];

    repository.save(&character).await.expect("save character");

    let shallow = repository
        .find_all(true)
        .await
        .expect("load shallow character list");
    assert_eq!(shallow.len(), 1);
    assert!(shallow[0].shallow, "list should be shallow");
    assert!(shallow[0].description.is_empty());
    assert!(shallow[0].data.character_book.is_none());

    let full = repository
        .find_by_name("cache_promotion")
        .await
        .expect("load full character");
    assert!(!full.shallow, "find_by_name should return full character");
    assert_eq!(full.description, "desc");
    assert_eq!(full.personality, "persona");
    assert_eq!(full.first_mes, "hello");
    assert_eq!(full.data.system_prompt, "system");
    assert_eq!(full.data.alternate_greetings, vec!["alt".to_string()]);
    assert!(full.data.character_book.is_some());

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
    repository.save(&character).await.expect("save character");

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
    repository.save(&character).await.expect("save character");

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
    repository.save(&character).await.expect("save character");

    let legacy_chat_dir = root.join("chats").join("Alice");
    fs::create_dir_all(&legacy_chat_dir)
        .await
        .expect("create legacy chat directory");
    fs::write(legacy_chat_dir.join("session.jsonl"), b"{}\n")
        .await
        .expect("write legacy chat file");

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
    repository.save(&character).await.expect("save character");

    let legacy_chat_dir = root.join("chats").join("Alice");
    fs::create_dir_all(&legacy_chat_dir)
        .await
        .expect("create legacy chat directory");
    fs::write(legacy_chat_dir.join("session.jsonl"), b"{}\n")
        .await
        .expect("write legacy chat file");

    repository
        .delete("Alice#1", true)
        .await
        .expect("delete exact character and chats");

    assert!(!root.join("characters").join("Alice#1.png").exists());
    assert!(!legacy_chat_dir.exists());

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
    repository.save(&source).await.expect("save source");

    let existing = Character::new(
        "Taken".to_string(),
        "desc".to_string(),
        "persona".to_string(),
        "hello".to_string(),
    );
    repository.save(&existing).await.expect("save existing");

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
async fn update_avatar_invalidates_stale_thumbnail() {
    let (repository, root) = setup_repository().await;

    let character = Character::new(
        "Avatar Target".to_string(),
        "desc".to_string(),
        "personality".to_string(),
        "hello".to_string(),
    );
    let created = repository
        .create_with_avatar(&character, None, None)
        .await
        .expect("create character")
        .character;

    let thumbnail_path = root.join("thumbnails/avatar").join(&created.avatar);
    fs::write(&thumbnail_path, b"stale thumbnail")
        .await
        .expect("write stale thumbnail");

    let replacement_path = root.join("replacement.png");
    fs::write(&replacement_path, build_distinct_png())
        .await
        .expect("write replacement avatar");

    repository
        .update_avatar(&created, &replacement_path, None)
        .await
        .expect("update avatar");

    assert!(
        !fs::try_exists(&thumbnail_path)
            .await
            .expect("check thumbnail")
    );

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn update_avatar_keeps_invalid_avatar_bytes_as_failure() {
    let (repository, root) = setup_repository().await;

    let character = Character::new(
        "Strict Avatar Target".to_string(),
        "desc".to_string(),
        "personality".to_string(),
        "hello".to_string(),
    );
    let created = repository
        .create_with_avatar(&character, None, None)
        .await
        .expect("create character")
        .character;

    let invalid_avatar_path = root.join("invalid-replacement.bin");
    fs::write(&invalid_avatar_path, b"not an image")
        .await
        .expect("write invalid avatar");

    let result = repository
        .update_avatar(&created, &invalid_avatar_path, None)
        .await;

    assert!(result.is_err(), "invalid avatar replacement should fail");

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn rename_allocates_new_file_stem_even_when_base_matches_current() {
    let (repository, root) = setup_repository().await;

    let character = Character::new(
        "Source".to_string(),
        "desc".to_string(),
        "persona".to_string(),
        "hello".to_string(),
    );
    repository.save(&character).await.expect("save character");

    let renamed = repository
        .rename("Source", "Source. ")
        .await
        .expect("rename character with trimmed stem");

    assert_eq!(renamed.avatar, "Source1.png");
    assert!(root.join("characters").join("Source1.png").exists());
    assert!(!root.join("characters").join("Source.png").exists());

    let _ = fs::remove_dir_all(&root).await;
}
