use std::io::Cursor;
use std::path::PathBuf;

use image::{DynamicImage, ImageFormat, RgbaImage};
use rand::random;
use serde_json::json;
use tokio::fs;

use crate::domain::models::character::Character;
use crate::domain::repositories::character_repository::CharacterRepository;
use crate::infrastructure::persistence::png_utils::write_character_data_to_png;

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
