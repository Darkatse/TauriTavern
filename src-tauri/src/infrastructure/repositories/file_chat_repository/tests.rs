use std::path::PathBuf;

use rand::random;
use serde_json::{json, Value};
use tokio::fs;

use crate::domain::errors::DomainError;
use crate::domain::repositories::chat_repository::ChatRepository;

use super::FileChatRepository;

fn unique_temp_root() -> PathBuf {
    std::env::temp_dir().join(format!("tauritavern-chat-repo-{}", random::<u64>()))
}

async fn setup_repository() -> (FileChatRepository, PathBuf) {
    let root = unique_temp_root();
    let repository = FileChatRepository::new(
        root.join("chats"),
        root.join("group chats"),
        root.join("backups"),
    );

    repository
        .ensure_directory_exists()
        .await
        .expect("create chat directories");

    (repository, root)
}

fn payload_with_integrity(integrity: &str) -> Vec<Value> {
    vec![
        json!({
            "chat_metadata": {
                "integrity": integrity,
            },
            "user_name": "unused",
            "character_name": "unused",
        }),
        json!({
            "name": "User",
            "is_user": true,
            "send_date": "2026-01-01T00:00:00.000Z",
            "mes": "hello",
            "extra": {},
        }),
    ]
}

#[test]
fn backup_file_name_uses_windows_safe_timestamp() {
    let backup_file_name = FileChatRepository::backup_file_name("Alice");

    assert!(backup_file_name.starts_with(FileChatRepository::CHAT_BACKUP_PREFIX));
    assert!(backup_file_name.ends_with(".jsonl"));
    assert!(!backup_file_name.contains(':'));

    let stem = backup_file_name
        .strip_suffix(".jsonl")
        .expect("backup file should end with .jsonl");
    let (_chat_key, timestamp) = stem
        .rsplit_once('_')
        .expect("backup file should contain trailing timestamp");

    assert_eq!(timestamp.len(), 15);
    assert_eq!(timestamp.chars().nth(8), Some('-'));
    assert!(timestamp
        .chars()
        .enumerate()
        .all(|(index, ch)| (index == 8 && ch == '-') || ch.is_ascii_digit()));
}

#[test]
fn backup_name_matches_sillytavern_sanitization() {
    let key = FileChatRepository::sanitize_backup_name_for_sillytavern("A:li*ce Name");
    assert_eq!(key, "alice_name");

    let unicode = FileChatRepository::sanitize_backup_name_for_sillytavern("角色-A");
    assert_eq!(unicode, "___a");
}

#[test]
fn backup_name_reserved_windows_name_becomes_empty() {
    let key = FileChatRepository::sanitize_backup_name_for_sillytavern("CON");
    assert_eq!(key, "");
}

#[test]
fn backup_file_prefix_matches_sillytavern_pattern() {
    let prefix = FileChatRepository::backup_file_prefix("A:li*ce Name");
    assert_eq!(prefix, "chat_alice_name_");
}

#[tokio::test]
async fn save_chat_payload_enforces_integrity_when_not_forced() {
    let (repository, root) = setup_repository().await;

    let first_payload = payload_with_integrity("slug-a");
    repository
        .save_chat_payload("alice", "session", &first_payload, false)
        .await
        .expect("initial save should succeed");

    let second_payload = payload_with_integrity("slug-b");
    let error = repository
        .save_chat_payload("alice", "session", &second_payload, false)
        .await
        .expect_err("save should fail on integrity mismatch");

    assert!(matches!(error, DomainError::InvalidData(message) if message == "integrity"));

    repository
        .save_chat_payload("alice", "session", &second_payload, true)
        .await
        .expect("forced overwrite should bypass integrity check");

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn save_and_load_chat_preserves_additional_fields() {
    let (repository, root) = setup_repository().await;

    let payload = vec![
        json!({
            "chat_metadata": {
                "integrity": "slug-a",
                "scenario": "metadata value",
            },
            "user_name": "unused",
            "character_name": "unused",
        }),
        json!({
            "name": "Assistant",
            "is_user": false,
            "send_date": "2026-01-01T00:00:00.000Z",
            "mes": "Hello",
            "custom_top_level": "kept",
            "extra": {
                "display_text": "Hello",
                "custom_extra": "kept",
            },
        }),
    ];

    repository
        .save_chat_payload("alice", "session", &payload, false)
        .await
        .expect("save payload");

    let chat = repository
        .get_chat("alice", "session")
        .await
        .expect("load chat");
    let message = chat.messages.first().expect("message should exist");

    assert_eq!(
        chat.chat_metadata
            .additional
            .get("scenario")
            .and_then(Value::as_str),
        Some("metadata value")
    );
    assert_eq!(
        message
            .additional
            .get("custom_top_level")
            .and_then(Value::as_str),
        Some("kept")
    );
    assert_eq!(
        message
            .extra
            .additional
            .get("custom_extra")
            .and_then(Value::as_str),
        Some("kept")
    );

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn group_chat_payload_roundtrip_and_delete() {
    let (repository, root) = setup_repository().await;
    let payload = payload_with_integrity("group-a");

    repository
        .save_group_chat_payload("group-session", &payload, false)
        .await
        .expect("save group chat payload");

    let saved = repository
        .get_group_chat_payload("group-session")
        .await
        .expect("read group chat payload");
    assert_eq!(saved.len(), payload.len());

    repository
        .delete_group_chat_payload("group-session")
        .await
        .expect("delete group chat payload");

    let deleted = repository.get_group_chat_payload("group-session").await;
    assert!(matches!(deleted, Err(DomainError::NotFound(_))));

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn import_chat_payload_creates_unique_files() {
    let (repository, root) = setup_repository().await;

    let import_path = root.join("import.jsonl");
    let import_content = payload_to_jsonl(&payload_with_integrity("import-a"));
    fs::write(&import_path, import_content)
        .await
        .expect("write import file");

    let first = repository
        .import_chat_payload("alice", "Alice", "User", &import_path, "jsonl")
        .await
        .expect("first import");
    let second = repository
        .import_chat_payload("alice", "Alice", "User", &import_path, "jsonl")
        .await
        .expect("second import");

    assert_eq!(first.len(), 1);
    assert_eq!(second.len(), 1);
    assert_ne!(first[0], second[0]);
    assert!(root.join("chats").join("alice").join(&first[0]).exists());
    assert!(root.join("chats").join("alice").join(&second[0]).exists());

    let _ = fs::remove_dir_all(&root).await;
}

fn payload_to_jsonl(payload: &[Value]) -> String {
    payload
        .iter()
        .map(|item| serde_json::to_string(item).expect("serialize line"))
        .collect::<Vec<_>>()
        .join("\n")
}
