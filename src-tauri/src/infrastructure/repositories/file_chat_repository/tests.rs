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
        root.join("characters"),
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

#[tokio::test]
async fn rename_chat_keeps_raw_header_fields_intact() {
    let (repository, root) = setup_repository().await;
    let payload = vec![
        json!({
            "chat_metadata": {
                "integrity": "rename-a",
            },
            "user_name": "unused",
            "character_name": "unused",
            "custom_header": {
                "keep": true,
            },
        }),
        json!({
            "name": "User",
            "is_user": true,
            "send_date": "2026-01-01T00:00:00.000Z",
            "mes": "hello",
            "extra": {},
        }),
    ];

    repository
        .save_chat_payload("alice", "session", &payload, false)
        .await
        .expect("save payload");

    repository
        .rename_chat("alice", "session", "session-renamed")
        .await
        .expect("rename chat");

    let renamed = repository
        .get_chat_payload("alice", "session-renamed")
        .await
        .expect("read renamed payload");
    assert_eq!(
        renamed[0]
            .get("custom_header")
            .and_then(Value::as_object)
            .and_then(|entry| entry.get("keep"))
            .and_then(Value::as_bool),
        Some(true)
    );

    let old = repository.get_chat_payload("alice", "session").await;
    assert!(matches!(old, Err(DomainError::NotFound(_))));

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn list_chat_summaries_returns_streamed_metadata() {
    let (repository, root) = setup_repository().await;
    let payload = vec![
        json!({
            "chat_metadata": {
                "integrity": "summary-a",
                "chat_id_hash": 42,
                "custom": "value",
            },
            "user_name": "unused",
            "character_name": "unused",
        }),
        json!({
            "name": "User",
            "is_user": true,
            "send_date": "2026-01-01T00:00:00.000Z",
            "mes": "hello there",
            "extra": {},
        }),
        json!({
            "name": "Alice",
            "is_user": false,
            "send_date": "2026-01-02T00:00:00.000Z",
            "mes": "latest response",
            "extra": {},
        }),
    ];

    repository
        .save_chat_payload("alice", "session", &payload, false)
        .await
        .expect("save payload");

    let summaries = repository
        .list_chat_summaries(Some("alice"), true)
        .await
        .expect("list chat summaries");
    assert_eq!(summaries.len(), 1);
    let summary = &summaries[0];
    assert_eq!(summary.character_name, "alice");
    assert_eq!(summary.file_name, "session.jsonl");
    assert_eq!(summary.message_count, 2);
    assert_eq!(summary.preview, "latest response");
    assert_eq!(summary.chat_id.as_deref(), Some("42"));
    assert_eq!(
        summary
            .chat_metadata
            .as_ref()
            .and_then(|meta| meta.get("custom"))
            .and_then(Value::as_str),
        Some("value")
    );

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn search_group_chats_respects_query_and_chat_filter() {
    let (repository, root) = setup_repository().await;

    let group_one = vec![
        json!({
            "chat_metadata": {
                "chat_id_hash": 100,
            },
            "user_name": "User",
            "character_name": "unused",
        }),
        json!({
            "name": "Narrator",
            "is_user": false,
            "send_date": "2026-01-01T00:00:00.000Z",
            "mes": "dragon appears",
            "extra": {},
        }),
    ];
    let group_two = vec![
        json!({
            "chat_metadata": {
                "chat_id_hash": 101,
            },
            "user_name": "User",
            "character_name": "unused",
        }),
        json!({
            "name": "Narrator",
            "is_user": false,
            "send_date": "2026-01-01T00:00:00.000Z",
            "mes": "unicorn appears",
            "extra": {},
        }),
    ];

    repository
        .save_group_chat_payload("group-one", &group_one, false)
        .await
        .expect("save group one");
    repository
        .save_group_chat_payload("group-two", &group_two, false)
        .await
        .expect("save group two");

    let group_filter = vec!["group-one".to_string()];
    let filtered = repository
        .search_group_chats("dragon", Some(&group_filter))
        .await
        .expect("search group chats");
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].file_name, "group-one.jsonl");

    let no_match = repository
        .search_group_chats("unicorn", Some(&group_filter))
        .await
        .expect("search group chats no match");
    assert!(no_match.is_empty());

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn summary_cache_is_invalidated_after_payload_save() {
    let (repository, root) = setup_repository().await;
    let first_payload = vec![
        json!({
            "chat_metadata": {
                "chat_id_hash": 300,
            },
            "user_name": "User",
            "character_name": "Alice",
        }),
        json!({
            "name": "Alice",
            "is_user": false,
            "send_date": "2026-01-01T00:00:00.000Z",
            "mes": "old message",
            "extra": {},
        }),
    ];
    repository
        .save_chat_payload("alice", "session", &first_payload, false)
        .await
        .expect("save first payload");

    let initial = repository
        .list_chat_summaries(Some("alice"), false)
        .await
        .expect("list summaries");
    assert_eq!(initial[0].preview, "old message");

    let updated_payload = vec![
        json!({
            "chat_metadata": {
                "chat_id_hash": 300,
            },
            "user_name": "User",
            "character_name": "Alice",
        }),
        json!({
            "name": "Alice",
            "is_user": false,
            "send_date": "2026-01-02T00:00:00.000Z",
            "mes": "new message",
            "extra": {},
        }),
    ];
    repository
        .save_chat_payload("alice", "session", &updated_payload, true)
        .await
        .expect("save updated payload");

    let refreshed = repository
        .list_chat_summaries(Some("alice"), false)
        .await
        .expect("list refreshed summaries");
    assert_eq!(refreshed[0].preview, "new message");

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn search_cache_is_invalidated_when_new_chat_file_is_saved() {
    let (repository, root) = setup_repository().await;

    let first_payload = vec![
        json!({
            "chat_metadata": {
                "chat_id_hash": 500,
            },
            "user_name": "User",
            "character_name": "Alice",
        }),
        json!({
            "name": "Alice",
            "is_user": false,
            "send_date": "2026-01-01T00:00:00.000Z",
            "mes": "hello world",
            "extra": {},
        }),
    ];
    repository
        .save_chat_payload("alice", "session-a", &first_payload, false)
        .await
        .expect("save first payload");

    let cached_empty = repository
        .search_chats("dragon", Some("alice"))
        .await
        .expect("initial search should succeed");
    assert!(cached_empty.is_empty());

    let second_payload = vec![
        json!({
            "chat_metadata": {
                "chat_id_hash": 501,
            },
            "user_name": "User",
            "character_name": "Alice",
        }),
        json!({
            "name": "Alice",
            "is_user": false,
            "send_date": "2026-01-02T00:00:00.000Z",
            "mes": "a dragon appears",
            "extra": {},
        }),
    ];
    repository
        .save_chat_payload("alice", "session-b", &second_payload, false)
        .await
        .expect("save second payload");

    let refreshed = repository
        .search_chats("dragon", Some("alice"))
        .await
        .expect("search after save should refresh cache");
    assert_eq!(refreshed.len(), 1);
    assert_eq!(refreshed[0].file_name, "session-b.jsonl");

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn search_cache_is_invalidated_after_import_chat_payload() {
    let (repository, root) = setup_repository().await;

    let cached_empty = repository
        .search_chats("phoenix", Some("alice"))
        .await
        .expect("initial search should succeed");
    assert!(cached_empty.is_empty());

    let import_path = root.join("import-phoenix.jsonl");
    let import_content = payload_to_jsonl(&vec![
        json!({
            "chat_metadata": {
                "chat_id_hash": 600,
            },
            "user_name": "User",
            "character_name": "Alice",
        }),
        json!({
            "name": "Alice",
            "is_user": false,
            "send_date": "2026-01-03T00:00:00.000Z",
            "mes": "phoenix rises",
            "extra": {},
        }),
    ]);
    fs::write(&import_path, import_content)
        .await
        .expect("write import source");

    repository
        .import_chat_payload("alice", "Alice", "User", &import_path, "jsonl")
        .await
        .expect("import payload");

    let refreshed = repository
        .search_chats("phoenix", Some("alice"))
        .await
        .expect("search after import should refresh cache");
    assert_eq!(refreshed.len(), 1);

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn summary_index_is_persisted_and_reloaded() {
    let (repository, root) = setup_repository().await;

    let payload = vec![
        json!({
            "chat_metadata": {
                "chat_id_hash": 700,
            },
            "user_name": "User",
            "character_name": "Alice",
        }),
        json!({
            "name": "Alice",
            "is_user": false,
            "send_date": "2026-01-04T00:00:00.000Z",
            "mes": "persist me",
            "extra": {},
        }),
    ];
    repository
        .save_chat_payload("alice", "session", &payload, false)
        .await
        .expect("save payload");

    let summaries = repository
        .list_chat_summaries(Some("alice"), false)
        .await
        .expect("list summaries");
    assert_eq!(summaries.len(), 1);

    let index_path = root
        .join("user")
        .join("cache")
        .join("chat_summary_index_v1.json");
    assert!(index_path.exists());

    let persisted_text = fs::read_to_string(&index_path)
        .await
        .expect("read persisted index");
    let persisted_json: Value =
        serde_json::from_str(&persisted_text).expect("parse persisted index as json");
    assert_eq!(
        persisted_json
            .get("entries")
            .and_then(Value::as_array)
            .map(|entries| entries.len()),
        Some(1)
    );

    let reloaded_repository = FileChatRepository::new(
        root.join("characters"),
        root.join("chats"),
        root.join("group chats"),
        root.join("backups"),
    );
    reloaded_repository
        .ensure_directory_exists()
        .await
        .expect("create directories for reloaded repository");

    let reloaded = reloaded_repository
        .list_chat_summaries(Some("alice"), false)
        .await
        .expect("list summaries after reload");
    assert_eq!(reloaded.len(), 1);
    assert_eq!(reloaded[0].preview, "persist me");

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn list_chat_summaries_without_filter_ignores_non_character_directories() {
    let (repository, root) = setup_repository().await;

    let backup_like_dir = root.join("chats").join("backups");
    fs::create_dir_all(&backup_like_dir)
        .await
        .expect("create backup-like directory");
    fs::write(
        backup_like_dir.join("chat_alice_20260218-120000.jsonl"),
        payload_to_jsonl(&payload_with_integrity("backup-a")),
    )
    .await
    .expect("write backup-like chat file");

    let summaries = repository
        .list_chat_summaries(None, false)
        .await
        .expect("list summaries");
    assert!(summaries.is_empty());

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn list_chat_summaries_without_filter_keeps_character_directories_with_cards() {
    let (repository, root) = setup_repository().await;

    let characters_dir = root.join("characters");
    fs::create_dir_all(&characters_dir)
        .await
        .expect("create characters directory");
    fs::write(characters_dir.join("alice.png"), b"")
        .await
        .expect("create character card");

    repository
        .save_chat_payload(
            "alice",
            "session",
            &payload_with_integrity("normal-a"),
            false,
        )
        .await
        .expect("save normal character chat");

    let summaries = repository
        .list_chat_summaries(None, false)
        .await
        .expect("list summaries");

    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].file_name, "session.jsonl");

    let _ = fs::remove_dir_all(&root).await;
}

fn payload_to_jsonl(payload: &[Value]) -> String {
    payload
        .iter()
        .map(|item| serde_json::to_string(item).expect("serialize line"))
        .collect::<Vec<_>>()
        .join("\n")
}
