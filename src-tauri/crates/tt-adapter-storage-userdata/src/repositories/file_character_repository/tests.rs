use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use chrono::DateTime;
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
use tt_domain::models::character::{Character, CharacterData};
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

#[test]
fn calculate_data_size_uses_js_string_length_semantics() {
    let data = CharacterData {
        name: "😀".to_string(),
        description: "abc".to_string(),
        tags: vec!["x".to_string(), "😀".to_string()],
        ..Default::default()
    };

    assert_eq!(FileCharacterRepository::calculate_data_size(&data), 28);
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
async fn shallow_index_repairs_create_date_without_persisting_patch() {
    let (repository, root) = setup_repository().await;

    let card_payload = json!({
        "name": "Shallow Legacy Date",
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

    let character_path = root.join("characters").join("ShallowLegacyDate.png");
    fs::write(&character_path, source_png)
        .await
        .expect("write character png");

    let shallow = repository
        .find_all(true)
        .await
        .expect("load shallow character list");
    assert_eq!(shallow.len(), 1);
    assert_eq!(shallow[0].create_date, "2026-03-16T12:34:56.000Z");

    let png_after_shallow = fs::read(&character_path)
        .await
        .expect("read shallow character png");
    let json_after_shallow =
        read_character_data_from_png(&png_after_shallow).expect("extract shallow card json");
    let value_after_shallow: serde_json::Value =
        serde_json::from_str(&json_after_shallow).expect("parse shallow card json");
    assert_eq!(
        value_after_shallow
            .get("create_date")
            .and_then(|value| value.as_str()),
        Some("2026-03-16 12:34:56 UTC")
    );

    let full = repository
        .find_by_name("ShallowLegacyDate")
        .await
        .expect("load full character");
    assert_eq!(full.create_date, "2026-03-16T12:34:56.000Z");

    let png_after_full = fs::read(&character_path)
        .await
        .expect("read repaired character png");
    let json_after_full =
        read_character_data_from_png(&png_after_full).expect("extract repaired card json");
    let value_after_full: serde_json::Value =
        serde_json::from_str(&json_after_full).expect("parse repaired card json");
    assert_eq!(
        value_after_full
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
async fn write_character_card_json_skips_when_writer_output_is_unchanged() {
    let (repository, root) = setup_repository().await;

    let character = Character::new(
        "Noop Edit".to_string(),
        "desc".to_string(),
        "persona".to_string(),
        "hello".to_string(),
    );
    repository.save(&character).await.expect("save character");
    repository
        .find_all(true)
        .await
        .expect("create persistent shallow index");

    let character_path = root.join("characters").join("Noop Edit.png");
    let before_png = fs::read(&character_path)
        .await
        .expect("read character png before edit");
    let card_json = serde_json::to_string(&character.to_v2()).expect("serialize card");

    let updated = repository
        .write_character_card_json("Noop Edit", &card_json, None, None)
        .await
        .expect("skip byte-identical metadata save");

    let after_png = fs::read(&character_path)
        .await
        .expect("read character png after edit");
    assert_eq!(updated.name, "Noop Edit");
    assert_eq!(after_png, before_png);
    assert!(shallow_index_path(&root).is_file());

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn write_character_card_json_rewrites_changed_metadata_and_clears_shallow_index() {
    let (repository, root) = setup_repository().await;

    let character = Character::new(
        "Changed Edit".to_string(),
        "desc".to_string(),
        "persona".to_string(),
        "hello".to_string(),
    );
    repository.save(&character).await.expect("save character");
    repository
        .find_all(true)
        .await
        .expect("create persistent shallow index");

    let character_path = root.join("characters").join("Changed Edit.png");
    let before_png = fs::read(&character_path)
        .await
        .expect("read character png before edit");
    let stored_json =
        read_character_data_from_png(&before_png).expect("extract stored character data");
    let mut stored_value: Value = serde_json::from_str(&stored_json).expect("parse stored card");
    stored_value["description"] = Value::String("changed desc".to_string());
    stored_value["data"]["description"] = Value::String("changed desc".to_string());
    let changed_json = serde_json::to_string(&stored_value).expect("serialize changed card");

    let updated = repository
        .write_character_card_json("Changed Edit", &changed_json, None, None)
        .await
        .expect("write changed metadata");

    let after_png = fs::read(&character_path)
        .await
        .expect("read character png after edit");
    let after_json =
        read_character_data_from_png(&after_png).expect("extract updated character data");
    let after_value: Value = serde_json::from_str(&after_json).expect("parse updated card");

    assert_eq!(updated.description, "changed desc");
    assert_ne!(after_png, before_png);
    assert_eq!(
        repository
            .find_by_name("Changed Edit")
            .await
            .expect("load updated cache entry")
            .description,
        "changed desc"
    );
    assert_eq!(
        after_value.get("description").and_then(Value::as_str),
        Some("changed desc")
    );
    assert!(!shallow_index_path(&root).exists());
    assert!(repository.shallow_index_cache.lock().await.is_none());

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn write_character_card_json_overwrites_invalid_existing_metadata() {
    let (repository, root) = setup_repository().await;

    let character = Character::new(
        "Repair Edit".to_string(),
        "desc".to_string(),
        "persona".to_string(),
        "hello".to_string(),
    );
    repository.save(&character).await.expect("save character");
    repository
        .find_all(true)
        .await
        .expect("create persistent shallow index");

    let character_path = root.join("characters").join("Repair Edit.png");
    let broken_png = insert_text_chunk_before_iend(build_minimal_png(), "chara", "not-base64");
    fs::write(&character_path, broken_png)
        .await
        .expect("replace card with broken metadata");
    let card_json = serde_json::to_string(&character.to_v2()).expect("serialize card");

    let updated = repository
        .write_character_card_json("Repair Edit", &card_json, None, None)
        .await
        .expect("repair broken metadata");

    let repaired_png = fs::read(&character_path)
        .await
        .expect("read repaired character png");
    let repaired_json =
        read_character_data_from_png(&repaired_png).expect("extract repaired character data");
    let repaired_value: Value = serde_json::from_str(&repaired_json).expect("parse repaired card");

    assert_eq!(updated.name, "Repair Edit");
    assert_eq!(
        repaired_value.get("name").and_then(Value::as_str),
        Some("Repair Edit")
    );
    assert!(!shallow_index_path(&root).exists());

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
    repository.save(&character).await.expect("save character");
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
    repository.save(&character).await.expect("save character");
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
async fn import_preserved_json_replaces_cached_full_card() {
    let (repository, root) = setup_repository().await;

    let old_character = Character::new(
        "Preserved".to_string(),
        "old description".to_string(),
        "old personality".to_string(),
        "old first message".to_string(),
    );
    repository
        .save(&old_character)
        .await
        .expect("save old character");
    repository
        .find_by_name("Preserved")
        .await
        .expect("load old character into full cache");

    let new_character = Character::new(
        "Imported Json Replacement".to_string(),
        "new json description".to_string(),
        "new json personality".to_string(),
        "new json first message".to_string(),
    );
    let import_path = root.join("replacement.json");
    fs::write(
        &import_path,
        serde_json::to_vec(&new_character.to_v2()).expect("serialize replacement json"),
    )
    .await
    .expect("write replacement json");

    let imported = repository
        .import_character(&import_path, Some("Preserved.png".to_string()))
        .await
        .expect("import preserved json replacement");
    assert_eq!(imported.avatar, "Preserved.png");
    assert_eq!(imported.first_mes, "new json first message");

    let reloaded = repository
        .find_by_name("Preserved")
        .await
        .expect("reload replacement");
    assert_eq!(reloaded.name, "Imported Json Replacement");
    assert_eq!(reloaded.first_mes, "new json first message");

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn import_preserved_character_replaces_cached_full_card() {
    let (repository, root) = setup_repository().await;

    let old_character = Character::new(
        "Preserved".to_string(),
        "old description".to_string(),
        "old personality".to_string(),
        "old first message".to_string(),
    );
    repository
        .save(&old_character)
        .await
        .expect("save old character");

    let cached_old = repository
        .find_by_name("Preserved")
        .await
        .expect("load old character into full cache");
    assert_eq!(cached_old.first_mes, "old first message");

    let new_character = Character::new(
        "Imported Replacement".to_string(),
        "new description".to_string(),
        "new personality".to_string(),
        "new first message".to_string(),
    );
    let source_png = write_character_data_to_png(
        &build_distinct_png(),
        &serde_json::to_string(&new_character.to_v2()).expect("serialize replacement card"),
    )
    .expect("embed replacement card in png");
    let import_path = root.join("replacement.png");
    fs::write(&import_path, source_png)
        .await
        .expect("write replacement png");

    let imported = repository
        .import_character(&import_path, Some("Preserved.png".to_string()))
        .await
        .expect("import preserved replacement");
    assert_eq!(imported.avatar, "Preserved.png");
    assert_eq!(imported.name, "Imported Replacement");
    assert_eq!(imported.first_mes, "new first message");
    let mut reloaded = repository
        .find_by_name("Preserved")
        .await
        .expect("reload replacement from full cache");
    assert_eq!(reloaded.name, "Imported Replacement");
    assert_eq!(reloaded.first_mes, "new first message");

    reloaded.chat = "Restored Existing Chat".to_string();
    let post_replace_json =
        serde_json::to_string(&reloaded.to_v2()).expect("serialize post-replace save");
    repository
        .write_character_card_json("Preserved", &post_replace_json, None, None)
        .await
        .expect("post-replace save should keep replacement metadata");

    let final_character = repository
        .find_by_name("Preserved")
        .await
        .expect("reload final character");
    assert_eq!(final_character.name, "Imported Replacement");
    assert_eq!(final_character.first_mes, "new first message");
    assert_eq!(final_character.chat, "Restored Existing Chat");

    let stored_json = repository
        .read_character_card_json("Preserved")
        .await
        .expect("read stored replacement card");
    let stored_value: serde_json::Value =
        serde_json::from_str(&stored_json).expect("parse stored replacement card");
    assert_eq!(
        stored_value.pointer("/data/first_mes"),
        Some(&json!("new first message"))
    );

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
    old_character.data.extensions.world = "Local Lore".to_string();
    repository
        .save(&old_character)
        .await
        .expect("save old character");

    let mut replacement = Character::new(
        "Replacement".to_string(),
        "new description".to_string(),
        "new personality".to_string(),
        "new first message".to_string(),
    );
    replacement.data.extensions.world = "Incoming Lore".to_string();
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
    assert_eq!(imported.data.extensions.world, "Local Lore");

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
    repository.save(&character).await.expect("save character");

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
    assert!(shallow.data_size > 0);

    assert!(shallow.description.is_empty());
    assert!(shallow.personality.is_empty());
    assert!(shallow.scenario.is_empty());
    assert!(shallow.first_mes.is_empty());
    assert!(shallow.mes_example.is_empty());
    assert!(shallow.data.system_prompt.is_empty());
    assert!(shallow.data.post_history_instructions.is_empty());
    assert!(shallow.data.alternate_greetings.is_empty());
    assert_eq!(shallow.data.extensions.world, "world");
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
async fn shallow_index_signature_tracks_chat_stats_changes() {
    let (repository, root) = setup_repository().await;

    let character = Character::new(
        "Indexed".to_string(),
        "desc".to_string(),
        "persona".to_string(),
        "hello".to_string(),
    );
    repository.save(&character).await.expect("save character");

    let first = repository
        .find_all(true)
        .await
        .expect("load initial shallow list");
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].chat_size, 0);
    assert_eq!(first[0].date_last_chat, 0);

    let chat_dir = root.join("chats").join("Indexed");
    fs::create_dir_all(&chat_dir)
        .await
        .expect("create chat dir");
    fs::write(
        chat_dir.join("session.jsonl"),
        b"{}\n{\"mes\":\"hello\",\"send_date\":\"2026-01-02T03:04:05.000Z\"}\n",
    )
    .await
    .expect("write chat");

    let expected_chat_date = DateTime::parse_from_rfc3339("2026-01-02T03:04:05.000Z")
        .expect("parse chat timestamp")
        .timestamp_millis();
    let second = repository
        .find_all(true)
        .await
        .expect("reload shallow list");
    assert_eq!(second.len(), 1);
    assert!(second[0].chat_size > 0);
    assert_eq!(second[0].date_last_chat, expected_chat_date);

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn persistent_shallow_index_is_used_after_repository_restart() {
    let (repository, root) = setup_repository().await;

    let character = Character::new(
        "Indexed".to_string(),
        "desc".to_string(),
        "persona".to_string(),
        "hello".to_string(),
    );
    repository.save(&character).await.expect("save character");

    let first = repository
        .find_all(true)
        .await
        .expect("create persistent shallow index");
    assert_eq!(first.len(), 1);

    let index_path = shallow_index_path(&root);
    assert!(
        index_path.is_file(),
        "persistent shallow index should be stored under default-user/user/cache"
    );

    let mut index_json: Value = serde_json::from_slice(
        &fs::read(&index_path)
            .await
            .expect("read persistent shallow index"),
    )
    .expect("parse persistent shallow index");
    index_json["entries"][0]["character"]["name"] =
        Value::String("From persistent index".to_string());
    fs::write(
        &index_path,
        serde_json::to_vec(&index_json).expect("serialize patched index"),
    )
    .await
    .expect("patch persistent shallow index");

    let restarted = repository_for_root(&root).await;
    let second = restarted
        .find_all(true)
        .await
        .expect("load persistent shallow index");
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].name, "From persistent index");
    assert!(second[0].shallow);
    assert_eq!(second[0].data_size, first[0].data_size);

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn persistent_shallow_index_rebuilds_when_chat_signature_changes() {
    let (repository, root) = setup_repository().await;

    let character = Character::new(
        "Indexed".to_string(),
        "desc".to_string(),
        "persona".to_string(),
        "hello".to_string(),
    );
    repository.save(&character).await.expect("save character");
    repository
        .find_all(true)
        .await
        .expect("create persistent shallow index");

    let index_path = shallow_index_path(&root);
    let mut index_json: Value = serde_json::from_slice(
        &fs::read(&index_path)
            .await
            .expect("read persistent shallow index"),
    )
    .expect("parse persistent shallow index");
    index_json["entries"][0]["character"]["name"] = Value::String("Stale".to_string());
    fs::write(
        &index_path,
        serde_json::to_vec(&index_json).expect("serialize stale index"),
    )
    .await
    .expect("write stale persistent shallow index");

    let chat_dir = root.join("chats").join("Indexed");
    fs::create_dir_all(&chat_dir)
        .await
        .expect("create chat dir");
    fs::write(chat_dir.join("session.jsonl"), b"{}\n{\"mes\":\"hello\"}\n")
        .await
        .expect("write chat");

    let restarted = repository_for_root(&root).await;
    let rebuilt = restarted
        .find_all(true)
        .await
        .expect("rebuild stale persistent shallow index");
    assert_eq!(rebuilt.len(), 1);
    assert_eq!(rebuilt[0].name, "Indexed");
    assert!(rebuilt[0].chat_size > 0);

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn invalid_persistent_shallow_index_is_ignored_and_rebuilt() {
    let (repository, root) = setup_repository().await;

    let character = Character::new(
        "Indexed".to_string(),
        "desc".to_string(),
        "persona".to_string(),
        "hello".to_string(),
    );
    repository.save(&character).await.expect("save character");
    repository
        .find_all(true)
        .await
        .expect("create persistent shallow index");

    let index_path = shallow_index_path(&root);
    fs::write(&index_path, b"{not json")
        .await
        .expect("corrupt persistent shallow index");

    let restarted = repository_for_root(&root).await;
    let rebuilt = restarted
        .find_all(true)
        .await
        .expect("rebuild invalid persistent shallow index");
    assert_eq!(rebuilt.len(), 1);
    assert_eq!(rebuilt[0].name, "Indexed");

    let repaired: Value = serde_json::from_slice(
        &fs::read(&index_path)
            .await
            .expect("read rebuilt persistent shallow index"),
    )
    .expect("rebuilt persistent shallow index should be valid JSON");
    assert_eq!(repaired["schema_version"], Value::from(1));

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn persistent_shallow_index_io_errors_do_not_fail_character_operations() {
    let (repository, root) = setup_repository().await;

    let index_path = shallow_index_path(&root);
    fs::create_dir_all(&index_path)
        .await
        .expect("create directory at persistent shallow index path");

    let character = Character::new(
        "Indexed".to_string(),
        "desc".to_string(),
        "persona".to_string(),
        "hello".to_string(),
    );
    repository
        .save(&character)
        .await
        .expect("save should ignore persistent shallow index remove failure");

    let characters = repository
        .find_all(true)
        .await
        .expect("shallow list should ignore persistent shallow index read/write failure");
    assert_eq!(characters.len(), 1);
    assert_eq!(characters[0].name, "Indexed");
    assert!(characters[0].shallow);
    assert!(repository.shallow_index_cache.lock().await.is_some());
    assert!(index_path.is_dir());

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn shallow_index_does_not_cache_partial_failures() {
    let (repository, root) = setup_repository().await;

    let good = Character::new(
        "Good".to_string(),
        "desc".to_string(),
        "persona".to_string(),
        "hello".to_string(),
    );
    repository.save(&good).await.expect("save good character");

    let broken_path = root.join("characters").join("Broken.png");
    fs::write(&broken_path, b"not a png")
        .await
        .expect("write broken character");

    let first = repository
        .find_all(true)
        .await
        .expect("load shallow list with broken card");
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].name, "Good");
    assert!(repository.shallow_index_cache.lock().await.is_none());

    let fixed = Character::new(
        "Broken".to_string(),
        "desc".to_string(),
        "persona".to_string(),
        "hello".to_string(),
    );
    let fixed_png = write_character_data_to_png(
        &build_minimal_png(),
        &serde_json::to_string(&fixed.to_v2()).expect("serialize fixed card"),
    )
    .expect("embed fixed card in png");
    fs::write(&broken_path, fixed_png)
        .await
        .expect("replace broken character");

    let mut names: Vec<String> = repository
        .find_all(true)
        .await
        .expect("reload shallow list")
        .into_iter()
        .map(|character| character.name)
        .collect();
    names.sort();
    assert_eq!(names, vec!["Broken".to_string(), "Good".to_string()]);

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn shallow_index_returns_stale_cache_when_refresh_partially_fails() {
    let (repository, root) = setup_repository().await;

    let good = Character::new(
        "Good".to_string(),
        "desc".to_string(),
        "persona".to_string(),
        "hello".to_string(),
    );
    repository.save(&good).await.expect("save good character");

    let cached = repository
        .find_all(true)
        .await
        .expect("warm shallow index cache");
    assert_eq!(cached.len(), 1);
    assert_eq!(cached[0].name, "Good");
    assert!(repository.shallow_index_cache.lock().await.is_some());

    let broken_path = root.join("characters").join("Broken.png");
    fs::write(&broken_path, b"not a png")
        .await
        .expect("write broken character");

    let stale = repository
        .find_all(true)
        .await
        .expect("load stale shallow index");
    assert_eq!(stale.len(), 1);
    assert_eq!(stale[0].name, "Good");
    assert_eq!(
        repository
            .shallow_index_cache
            .lock()
            .await
            .as_ref()
            .expect("stale cache should remain")
            .characters
            .len(),
        1
    );

    let fixed = Character::new(
        "Broken".to_string(),
        "desc".to_string(),
        "persona".to_string(),
        "hello".to_string(),
    );
    let fixed_png = write_character_data_to_png(
        &build_minimal_png(),
        &serde_json::to_string(&fixed.to_v2()).expect("serialize fixed card"),
    )
    .expect("embed fixed card in png");
    fs::write(&broken_path, fixed_png)
        .await
        .expect("replace broken character");

    let mut names: Vec<String> = repository
        .find_all(true)
        .await
        .expect("reload complete shallow index")
        .into_iter()
        .map(|character| character.name)
        .collect();
    names.sort();
    assert_eq!(names, vec!["Broken".to_string(), "Good".to_string()]);

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
async fn character_chat_simple_listing_does_not_build_summary_cache() {
    let (repository, root) = setup_repository().await;

    let chat_dir = root.join("chats").join("Alice");
    fs::create_dir_all(&chat_dir)
        .await
        .expect("create chat dir");
    fs::write(
        chat_dir.join("session.jsonl"),
        b"{}\n{\"mes\":\"hello\",\"send_date\":\"2026-01-01T00:00:00.000Z\"}\n",
    )
    .await
    .expect("write chat file");

    let chats = repository
        .get_character_chats("Alice", true)
        .await
        .expect("list simple character chats");
    assert_eq!(chats.len(), 1);
    assert_eq!(chats[0].file_name, "session.jsonl");
    assert_eq!(chats[0].file_size, "");
    assert_eq!(chats[0].chat_items, 0);
    assert_eq!(chats[0].last_message, "");
    assert_eq!(chats[0].last_message_date, 0);
    assert!(
        !chat_summary_index_path(&root).exists(),
        "simple listing should not scan chat payloads"
    );

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn character_chat_listing_uses_shared_summary_cache() {
    let (repository, root) = setup_repository().await;

    let character = Character::new(
        "Alice".to_string(),
        "desc".to_string(),
        "persona".to_string(),
        "hello".to_string(),
    );
    repository.save(&character).await.expect("save character");

    let chat_dir = root.join("chats").join("Alice");
    fs::create_dir_all(&chat_dir)
        .await
        .expect("create chat dir");
    let long_tail = format!("{} complete tail", "x".repeat(450));
    let payload = format!(
        "{{}}\n{{\"mes\":{},\"send_date\":\"2026-01-01T00:00:00.000Z\"}}\n",
        serde_json::to_string(&long_tail).expect("serialize long message")
    );
    fs::write(chat_dir.join("session.jsonl"), payload)
        .await
        .expect("write chat file");

    let chats = repository
        .get_character_chats("Alice", false)
        .await
        .expect("list character chats");
    assert_eq!(chats.len(), 1);
    assert_eq!(chats[0].file_name, "session.jsonl");
    assert_eq!(chats[0].chat_items, 1);
    assert_eq!(chats[0].last_message, long_tail);
    assert_eq!(
        chats[0].last_message_date,
        DateTime::parse_from_rfc3339("2026-01-01T00:00:00.000Z")
            .expect("parse chat timestamp")
            .timestamp_millis()
    );

    let index_path = chat_summary_index_path(&root);
    assert!(index_path.exists(), "summary listing should persist cache");
    let index_text = fs::read_to_string(&index_path)
        .await
        .expect("read chat summary index");
    assert!(index_text.contains("session.jsonl"));
    assert!(index_text.contains("complete tail"));
    assert!(
        !index_text.contains(&long_tail),
        "summary index should keep bounded previews, not full compatibility messages"
    );

    let restarted = repository_for_root(&root).await;
    let reloaded = restarted
        .get_character_chats("Alice", false)
        .await
        .expect("list character chats from persistent cache");
    assert_eq!(reloaded.len(), 1);
    assert_eq!(reloaded[0].last_message, long_tail);

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn character_chat_listing_refreshes_shared_summary_cache_after_file_change() {
    let (repository, root) = setup_repository().await;

    let chat_dir = root.join("chats").join("Alice");
    fs::create_dir_all(&chat_dir)
        .await
        .expect("create chat dir");
    let chat_path = chat_dir.join("session.jsonl");
    fs::write(
        &chat_path,
        b"{}\n{\"mes\":\"old message\",\"send_date\":\"2026-01-01T00:00:00.000Z\"}\n",
    )
    .await
    .expect("write initial chat file");

    let initial = repository
        .get_character_chats("Alice", false)
        .await
        .expect("list initial character chats");
    assert_eq!(initial.len(), 1);
    assert_eq!(initial[0].last_message, "old message");

    fs::write(
        &chat_path,
        b"{}\n{\"mes\":\"old message\",\"send_date\":\"2026-01-01T00:00:00.000Z\"}\n{\"mes\":\"new message with a larger payload\",\"send_date\":\"2026-01-02T00:00:00.000Z\"}\n",
    )
    .await
    .expect("update chat file");

    let refreshed = repository
        .get_character_chats("Alice", false)
        .await
        .expect("list refreshed character chats");
    assert_eq!(refreshed.len(), 1);
    assert_eq!(refreshed[0].chat_items, 2);
    assert_eq!(
        refreshed[0].last_message,
        "new message with a larger payload"
    );
    assert_eq!(
        refreshed[0].last_message_date,
        DateTime::parse_from_rfc3339("2026-01-02T00:00:00.000Z")
            .expect("parse chat timestamp")
            .timestamp_millis()
    );

    let index_text = fs::read_to_string(chat_summary_index_path(&root))
        .await
        .expect("read refreshed chat summary index");
    assert!(index_text.contains("new message with a larger payload"));
    assert!(!index_text.contains("old message"));

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
    repository.save(&character).await.expect("save character");

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
async fn update_avatar_replaces_stored_image() {
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

    let replacement_path = root.join("replacement.png");
    fs::write(&replacement_path, build_distinct_png())
        .await
        .expect("write replacement avatar");

    repository
        .update_avatar(&created, &replacement_path, None)
        .await
        .expect("update avatar");

    let stored_png = fs::read(root.join("characters").join(&created.avatar))
        .await
        .expect("read updated character png");
    let stored_image = image::load_from_memory(&stored_png).expect("decode updated avatar");
    assert_eq!(stored_image.width(), 2);
    assert_eq!(stored_image.height(), 2);

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
