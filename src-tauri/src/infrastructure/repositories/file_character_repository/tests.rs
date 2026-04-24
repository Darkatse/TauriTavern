use chrono::DateTime;
use std::io::Cursor;
use std::path::PathBuf;

use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use rand::random;
use serde_json::json;
use tokio::fs;

use crate::domain::models::character::Character;
use crate::domain::repositories::character_repository::CharacterRepository;
use crate::infrastructure::persistence::png_utils::{
    read_character_data_from_png, write_character_data_to_png,
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

async fn setup_repository() -> (FileCharacterRepository, PathBuf) {
    let root = unique_temp_root();
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

    let repository = FileCharacterRepository::new(characters_dir, chats_dir, default_avatar);
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
        .expect("create first character");

    let second = Character::new(
        "Duplicate".to_string(),
        "desc".to_string(),
        "persona".to_string(),
        "Second greeting".to_string(),
    );
    let created_second = repository
        .create_with_avatar(&second, None, None)
        .await
        .expect("create second character");

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
        .expect("create character");

    assert_eq!(created.avatar, "UnsafeName.png");

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
        .expect("create character with avatar");

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
