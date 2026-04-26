use std::sync::Arc;

use std::path::PathBuf;

use rand::random;
use serde_json::{Value, json};
use tokio::fs;

use crate::domain::errors::DomainError;
use crate::domain::models::character::sanitize_filename;
use crate::domain::repositories::chat_repository::{
    ChatMessageRole, ChatMessageSearchFilters, ChatMessageSearchQuery, ChatPayloadPatchOp,
    ChatRepository, PinnedCharacterChat, PinnedGroupChat,
};
use crate::domain::repositories::group_chat_repository::GroupChatRepository;

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

fn payload_without_integrity() -> Vec<Value> {
    vec![
        json!({
            "chat_metadata": {},
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

fn payload_with_message(
    integrity: &str,
    send_date: &str,
    message: &str,
    character_name: &str,
) -> Vec<Value> {
    vec![
        json!({
            "chat_metadata": {
                "integrity": integrity,
            },
            "user_name": "unused",
            "character_name": character_name,
        }),
        json!({
            "name": character_name,
            "is_user": false,
            "send_date": send_date,
            "mes": message,
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
    assert!(
        timestamp
            .chars()
            .enumerate()
            .all(|(index, ch)| (index == 8 && ch == '-') || ch.is_ascii_digit())
    );
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

#[test]
fn normalize_backup_file_name_rejects_non_chat_prefix() {
    let result = FileChatRepository::normalize_backup_file_name("notes_20260101.jsonl");
    assert!(matches!(result, Err(DomainError::InvalidData(_))));
}

#[test]
fn normalize_backup_file_name_uses_leaf_name() {
    let normalized =
        FileChatRepository::normalize_backup_file_name("../chat_alice_20260101-000000.jsonl")
            .expect("normalize backup file name");
    assert_eq!(normalized, "chat_alice_20260101-000000.jsonl");
}

#[tokio::test]
async fn chat_payload_bytes_roundtrip_and_path() {
    let (repository, root) = setup_repository().await;

    let raw_payload = payload_to_jsonl(&payload_with_integrity("bytes-a"));
    let source = root.join("chat-source.jsonl");
    fs::write(&source, &raw_payload)
        .await
        .expect("write chat source payload");
    repository
        .save_chat_payload_from_path("alice", "session", &source, false)
        .await
        .expect("save payload from source file");

    let loaded_bytes = repository
        .get_chat_payload_bytes("alice", "session")
        .await
        .expect("load raw payload bytes");
    assert_eq!(loaded_bytes, raw_payload.as_bytes());

    let payload_path = repository
        .get_chat_payload_path("alice", "session")
        .await
        .expect("get payload path");
    assert!(payload_path.exists());
    assert_eq!(
        payload_path.file_name().and_then(|name| name.to_str()),
        Some("session.jsonl")
    );

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn save_chat_payload_from_path_sanitizes_windows_unsafe_path_segments() {
    let (repository, root) = setup_repository().await;

    let character_name = "ali:ce";
    let file_name = "session:2026/02*21?";
    let raw_payload = payload_to_jsonl(&payload_with_integrity("bytes-safe-path"));
    let source = root.join("unsafe-path-source.jsonl");
    fs::write(&source, &raw_payload)
        .await
        .expect("write unsafe chat payload source");

    repository
        .save_chat_payload_from_path(character_name, file_name, &source, false)
        .await
        .expect("save payload from source file with unsafe path segments");

    let expected_path = root
        .join("chats")
        .join(sanitize_filename(character_name))
        .join(format!("{}.jsonl", sanitize_filename(file_name)));
    assert!(expected_path.exists());

    let loaded_bytes = repository
        .get_chat_payload_bytes(character_name, file_name)
        .await
        .expect("load raw payload bytes via unsanitized identifiers");
    assert_eq!(loaded_bytes, raw_payload.as_bytes());

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn save_chat_payload_from_path_enforces_integrity() {
    let (repository, root) = setup_repository().await;

    let source_a = root.join("source-a.jsonl");
    let payload_a = payload_to_jsonl(&payload_with_integrity("path-a"));
    fs::write(&source_a, &payload_a)
        .await
        .expect("write first source payload");

    repository
        .save_chat_payload_from_path("alice", "session", &source_a, false)
        .await
        .expect("save payload from source file");

    let source_b = root.join("source-b.jsonl");
    let payload_b = payload_to_jsonl(&payload_with_integrity("path-b"));
    fs::write(&source_b, &payload_b)
        .await
        .expect("write second source payload");

    let error = repository
        .save_chat_payload_from_path("alice", "session", &source_b, false)
        .await
        .expect_err("save should fail on integrity mismatch");
    assert!(matches!(error, DomainError::InvalidData(message) if message == "integrity"));

    repository
        .save_chat_payload_from_path("alice", "session", &source_b, true)
        .await
        .expect("forced save should bypass integrity check");

    let loaded_bytes = repository
        .get_chat_payload_bytes("alice", "session")
        .await
        .expect("load chat payload bytes");
    assert_eq!(loaded_bytes, payload_b.as_bytes());

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn save_chat_payload_from_path_rejects_missing_integrity_when_existing_has_one() {
    let (repository, root) = setup_repository().await;

    let source_a = root.join("source-with-integrity.jsonl");
    let payload_a = payload_to_jsonl(&payload_with_integrity("path-a"));
    fs::write(&source_a, &payload_a)
        .await
        .expect("write source payload with integrity");

    repository
        .save_chat_payload_from_path("alice", "session", &source_a, false)
        .await
        .expect("save payload from source file");

    let source_b = root.join("source-without-integrity.jsonl");
    let payload_b = payload_to_jsonl(&payload_without_integrity());
    fs::write(&source_b, &payload_b)
        .await
        .expect("write source payload without integrity");

    let error = repository
        .save_chat_payload_from_path("alice", "session", &source_b, false)
        .await
        .expect_err("save should fail when incoming integrity is missing");
    assert!(matches!(error, DomainError::InvalidData(message) if message == "integrity"));

    repository
        .save_chat_payload_from_path("alice", "session", &source_b, true)
        .await
        .expect("forced save should bypass missing integrity check");

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn concurrent_save_chat_payload_from_path_serializes_same_target() {
    let (repository, root) = setup_repository().await;
    let repository = Arc::new(repository);

    let source_a = root.join("source-concurrent-a.jsonl");
    let payload_a = payload_to_jsonl(&payload_with_message(
        "path-concurrent",
        "2026-01-01T00:00:00.000Z",
        "concurrent-a",
        "Assistant",
    ));
    fs::write(&source_a, &payload_a)
        .await
        .expect("write first concurrent source payload");

    let source_b = root.join("source-concurrent-b.jsonl");
    let payload_b = payload_to_jsonl(&payload_with_message(
        "path-concurrent",
        "2026-01-01T00:00:00.000Z",
        "concurrent-b",
        "Assistant",
    ));
    fs::write(&source_b, &payload_b)
        .await
        .expect("write second concurrent source payload");

    let repository_a = Arc::clone(&repository);
    let repository_b = Arc::clone(&repository);
    let source_a_task = source_a.clone();
    let source_b_task = source_b.clone();

    let save_a = tokio::spawn(async move {
        repository_a
            .save_chat_payload_from_path("alice", "session", &source_a_task, false)
            .await
    });
    let save_b = tokio::spawn(async move {
        repository_b
            .save_chat_payload_from_path("alice", "session", &source_b_task, false)
            .await
    });

    let result_a = save_a.await.expect("join concurrent save a");
    let result_b = save_b.await.expect("join concurrent save b");
    assert!(result_a.is_ok(), "first concurrent save should succeed");
    assert!(result_b.is_ok(), "second concurrent save should succeed");

    let loaded_bytes = repository
        .get_chat_payload_bytes("alice", "session")
        .await
        .expect("load concurrent payload bytes");
    assert!(
        loaded_bytes == payload_a.as_bytes() || loaded_bytes == payload_b.as_bytes(),
        "final payload should match one completed save"
    );

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

    save_chat_payload_from_values(&repository, &root, "alice", "session", &payload, false)
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
async fn group_chat_payload_bytes_roundtrip_and_path() {
    let (repository, root) = setup_repository().await;

    let raw_payload = payload_to_jsonl(&payload_with_integrity("group-bytes-a"));
    let source = root.join("group-source.jsonl");
    fs::write(&source, &raw_payload)
        .await
        .expect("write group source payload");
    repository
        .save_group_chat_payload_from_path("group-session", &source, false)
        .await
        .expect("save group payload from source file");

    let payload_path = repository
        .get_group_chat_payload_path("group-session")
        .await
        .expect("get group payload path");
    assert!(payload_path.exists());

    let loaded_bytes = fs::read(&payload_path)
        .await
        .expect("load group payload bytes");
    assert_eq!(loaded_bytes, raw_payload.as_bytes());
    assert_eq!(
        payload_path.file_name().and_then(|name| name.to_str()),
        Some("group-session.jsonl")
    );

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn save_group_chat_payload_from_path_sanitizes_windows_unsafe_id() {
    let (repository, root) = setup_repository().await;

    let group_id = "group:one/2026*02?21";
    let raw_payload = payload_to_jsonl(&payload_with_integrity("group-safe-path"));
    let source = root.join("group-unsafe-id-source.jsonl");
    fs::write(&source, &raw_payload)
        .await
        .expect("write group payload source");

    repository
        .save_group_chat_payload_from_path(group_id, &source, false)
        .await
        .expect("save group payload from source file with unsafe id");

    let expected_path = root
        .join("group chats")
        .join(format!("{}.jsonl", sanitize_filename(group_id)));
    assert!(expected_path.exists());

    let loaded_bytes = fs::read(&expected_path)
        .await
        .expect("load group payload bytes via unsanitized id");
    assert_eq!(loaded_bytes, raw_payload.as_bytes());

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn save_group_chat_payload_from_path_enforces_integrity() {
    let (repository, root) = setup_repository().await;

    let source_a = root.join("group-source-a.jsonl");
    let payload_a = payload_to_jsonl(&payload_with_integrity("group-path-a"));
    fs::write(&source_a, &payload_a)
        .await
        .expect("write first group source payload");

    repository
        .save_group_chat_payload_from_path("group-session", &source_a, false)
        .await
        .expect("save group payload from source file");

    let source_b = root.join("group-source-b.jsonl");
    let payload_b = payload_to_jsonl(&payload_with_integrity("group-path-b"));
    fs::write(&source_b, &payload_b)
        .await
        .expect("write second group source payload");

    let error = repository
        .save_group_chat_payload_from_path("group-session", &source_b, false)
        .await
        .expect_err("save should fail on integrity mismatch");
    assert!(matches!(error, DomainError::InvalidData(message) if message == "integrity"));

    repository
        .save_group_chat_payload_from_path("group-session", &source_b, true)
        .await
        .expect("forced group save should bypass integrity check");

    let payload_path = repository
        .get_group_chat_payload_path("group-session")
        .await
        .expect("get group payload path");
    let loaded_bytes = fs::read(&payload_path)
        .await
        .expect("load group payload bytes");
    assert_eq!(loaded_bytes, payload_b.as_bytes());

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn group_chat_payload_roundtrip_and_delete() {
    let (repository, root) = setup_repository().await;
    let payload = payload_with_integrity("group-a");

    let source = root.join("group-roundtrip.jsonl");
    fs::write(&source, payload_to_jsonl(&payload))
        .await
        .expect("write group payload source");
    repository
        .save_group_chat_payload_from_path("group-session", &source, false)
        .await
        .expect("save group payload from source file");

    let payload_path = repository
        .get_group_chat_payload_path("group-session")
        .await
        .expect("get group payload path");
    let bytes = fs::read(&payload_path)
        .await
        .expect("read group payload bytes");
    let saved = crate::infrastructure::persistence::jsonl_utils::parse_jsonl_bytes(&bytes)
        .expect("parse group payload");
    assert_eq!(saved.len(), payload.len());

    repository
        .delete_group_chat_payload("group-session")
        .await
        .expect("delete group chat payload");

    let deleted = repository
        .get_group_chat_payload_path("group-session")
        .await;
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

    save_chat_payload_from_values(&repository, &root, "alice", "session", &payload, false)
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

    save_chat_payload_from_values(&repository, &root, "alice", "session", &payload, false)
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

    save_group_chat_payload_from_values(&repository, &root, "group-one", &group_one, false)
        .await
        .expect("save group one");
    save_group_chat_payload_from_values(&repository, &root, "group-two", &group_two, false)
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
async fn search_character_chat_messages_returns_scored_hits_and_respects_role_filter() {
    let (repository, root) = setup_repository().await;

    let payload = vec![
        json!({
            "chat_metadata": {
                "integrity": "search-a",
            },
            "user_name": "unused",
            "character_name": "unused",
        }),
        json!({
            "name": "User",
            "is_user": true,
            "is_system": false,
            "send_date": "2026-01-01T00:00:00.000Z",
            "mes": "今天我们去北京吃烤鸭。",
            "extra": {},
        }),
        json!({
            "name": "Alice",
            "is_user": false,
            "is_system": false,
            "send_date": "2026-01-01T00:00:01.000Z",
            "mes": "我最喜欢北京烤鸭，还有豆汁儿。",
            "extra": {},
        }),
        json!({
            "name": "System",
            "is_user": false,
            "is_system": true,
            "send_date": "2026-01-01T00:00:02.000Z",
            "mes": "系统提示：请注意安全。",
            "extra": {},
        }),
        json!({
            "name": "Alice",
            "is_user": false,
            "is_system": false,
            "send_date": "2026-01-01T00:00:03.000Z",
            "mes": "明天去上海吧。",
            "extra": {},
        }),
    ];

    save_chat_payload_from_values(&repository, &root, "alice", "session", &payload, false)
        .await
        .expect("save payload");

    let hits = repository
        .search_character_chat_messages(
            "alice",
            "session",
            ChatMessageSearchQuery {
                query: "北京烤鸭".to_string(),
                limit: 2,
                filters: None,
            },
        )
        .await
        .expect("search messages");

    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].index, 1);
    assert_eq!(hits[0].role, ChatMessageRole::Assistant);
    assert!(hits[0].text.contains("北京烤鸭"));
    assert!(hits[0].score > 0.9);

    let user_hits = repository
        .search_character_chat_messages(
            "alice",
            "session",
            ChatMessageSearchQuery {
                query: "北京烤鸭".to_string(),
                limit: 10,
                filters: Some(ChatMessageSearchFilters {
                    role: Some(ChatMessageRole::User),
                    start_index: None,
                    end_index: None,
                    scan_limit: None,
                }),
            },
        )
        .await
        .expect("search messages with role filter");

    assert_eq!(user_hits.len(), 1);
    assert_eq!(user_hits[0].index, 0);
    assert_eq!(user_hits[0].role, ChatMessageRole::User);

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn search_group_chat_messages_respects_scan_limit() {
    let (repository, root) = setup_repository().await;

    let payload = vec![
        json!({
            "chat_metadata": {
                "integrity": "group-search-a",
            },
            "user_name": "User",
            "character_name": "unused",
        }),
        json!({
            "name": "Narrator",
            "is_user": false,
            "is_system": false,
            "send_date": "2026-01-01T00:00:00.000Z",
            "mes": "dragon appears",
            "extra": {},
        }),
        json!({
            "name": "Narrator",
            "is_user": false,
            "is_system": false,
            "send_date": "2026-01-01T00:00:01.000Z",
            "mes": "unicorn appears",
            "extra": {},
        }),
    ];

    save_group_chat_payload_from_values(&repository, &root, "group-one", &payload, false)
        .await
        .expect("save group payload");

    let limited = repository
        .search_group_chat_messages(
            "group-one",
            ChatMessageSearchQuery {
                query: "dragon".to_string(),
                limit: 10,
                filters: Some(ChatMessageSearchFilters {
                    role: None,
                    start_index: None,
                    end_index: None,
                    scan_limit: Some(1),
                }),
            },
        )
        .await
        .expect("search group messages with scan limit");

    assert!(limited.is_empty());

    let full = repository
        .search_group_chat_messages(
            "group-one",
            ChatMessageSearchQuery {
                query: "dragon".to_string(),
                limit: 10,
                filters: Some(ChatMessageSearchFilters {
                    role: None,
                    start_index: None,
                    end_index: None,
                    scan_limit: Some(10),
                }),
            },
        )
        .await
        .expect("search group messages without scan limit");

    assert_eq!(full.len(), 1);
    assert_eq!(full[0].index, 0);

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
    save_chat_payload_from_values(
        &repository,
        &root,
        "alice",
        "session",
        &first_payload,
        false,
    )
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
    save_chat_payload_from_values(
        &repository,
        &root,
        "alice",
        "session",
        &updated_payload,
        true,
    )
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
    save_chat_payload_from_values(
        &repository,
        &root,
        "alice",
        "session-a",
        &first_payload,
        false,
    )
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
    save_chat_payload_from_values(
        &repository,
        &root,
        "alice",
        "session-b",
        &second_payload,
        false,
    )
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
    save_chat_payload_from_values(&repository, &root, "alice", "session", &payload, false)
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

    let payload = payload_with_integrity("normal-a");
    save_chat_payload_from_values(&repository, &root, "alice", "session", &payload, false)
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

#[tokio::test]
async fn list_recent_chat_summaries_limits_results_and_keeps_pinned() {
    let (repository, root) = setup_repository().await;
    let characters_dir = root.join("characters");
    fs::create_dir_all(&characters_dir)
        .await
        .expect("create characters directory");
    fs::write(characters_dir.join("alice.png"), b"")
        .await
        .expect("create alice card");
    fs::write(characters_dir.join("bob.png"), b"")
        .await
        .expect("create bob card");

    let old_payload =
        payload_with_message("recent-old", "2026-01-01T00:00:00.000Z", "old", "Alice");
    save_chat_payload_from_values(
        &repository,
        &root,
        "alice",
        "session-old",
        &old_payload,
        false,
    )
    .await
    .expect("save old chat");

    let mid_payload =
        payload_with_message("recent-mid", "2026-01-02T00:00:00.000Z", "mid", "Alice");
    save_chat_payload_from_values(
        &repository,
        &root,
        "alice",
        "session-mid",
        &mid_payload,
        false,
    )
    .await
    .expect("save middle chat");

    let new_payload = payload_with_message("recent-new", "2026-01-03T00:00:00.000Z", "new", "Bob");
    save_chat_payload_from_values(
        &repository,
        &root,
        "bob",
        "session-new",
        &new_payload,
        false,
    )
    .await
    .expect("save new chat");

    let pinned = vec![PinnedCharacterChat {
        character_name: "alice".to_string(),
        file_name: "session-old".to_string(),
    }];
    let results = repository
        .list_recent_chat_summaries(None, false, 2, &pinned)
        .await
        .expect("list recent summaries");

    assert_eq!(results.len(), 2);
    assert!(
        results
            .iter()
            .any(|entry| entry.file_name == "session-old.jsonl")
    );
    assert!(
        results
            .iter()
            .any(|entry| entry.file_name == "session-new.jsonl")
    );

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn list_recent_group_chat_summaries_limits_results_and_keeps_pinned() {
    let (repository, root) = setup_repository().await;

    let old_group_payload = payload_with_message(
        "group-recent-old",
        "2026-01-01T00:00:00.000Z",
        "old group",
        "Group",
    );
    save_group_chat_payload_from_values(&repository, &root, "group-old", &old_group_payload, false)
        .await
        .expect("save old group chat");

    let new_group_payload = payload_with_message(
        "group-recent-new",
        "2026-01-03T00:00:00.000Z",
        "new group",
        "Group",
    );
    save_group_chat_payload_from_values(&repository, &root, "group-new", &new_group_payload, false)
        .await
        .expect("save new group chat");

    let pinned = vec![PinnedGroupChat {
        chat_id: "group-old".to_string(),
    }];
    let results = repository
        .list_recent_group_chat_summaries(None, false, 2, &pinned)
        .await
        .expect("list recent group summaries");

    assert_eq!(results.len(), 2);
    assert!(
        results
            .iter()
            .any(|entry| entry.file_name == "group-old.jsonl")
    );
    assert!(
        results
            .iter()
            .any(|entry| entry.file_name == "group-new.jsonl")
    );

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn recent_summary_skips_fingerprint_and_search_builds_it_lazily() {
    let (repository, root) = setup_repository().await;

    let payload = payload_with_message(
        "lazy-fingerprint",
        "2026-01-05T00:00:00.000Z",
        "dragon keyword",
        "Alice",
    );
    save_chat_payload_from_values(&repository, &root, "alice", "session", &payload, false)
        .await
        .expect("save payload");

    let recent = repository
        .list_recent_chat_summaries(Some("alice"), false, 1, &[])
        .await
        .expect("list recent summaries");
    assert_eq!(recent.len(), 1);

    let index_path = root
        .join("user")
        .join("cache")
        .join("chat_summary_index_v1.json");
    let index_before_search = fs::read_to_string(&index_path)
        .await
        .expect("read summary index after recent list");
    let parsed_before: Value =
        serde_json::from_str(&index_before_search).expect("parse summary index before search");
    let before_entries = parsed_before
        .get("entries")
        .and_then(Value::as_array)
        .expect("entries should exist");
    assert_eq!(before_entries.len(), 1);
    assert!(
        before_entries[0]
            .get("fingerprint")
            .map(Value::is_null)
            .unwrap_or(true),
        "recent listing should not materialize fingerprint"
    );

    let search = repository
        .search_chats("dragon", Some("alice"))
        .await
        .expect("search chats");
    assert_eq!(search.len(), 1);

    let index_after_search = fs::read_to_string(&index_path)
        .await
        .expect("read summary index after search");
    let parsed_after: Value =
        serde_json::from_str(&index_after_search).expect("parse summary index after search");
    let after_entries = parsed_after
        .get("entries")
        .and_then(Value::as_array)
        .expect("entries should exist");
    assert_eq!(after_entries.len(), 1);
    assert!(
        after_entries[0]
            .get("fingerprint")
            .is_some_and(|value| !value.is_null()),
        "search should materialize fingerprint lazily"
    );

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn empty_character_search_uses_summary_without_fingerprint() {
    let (repository, root) = setup_repository().await;

    let payload = payload_with_message(
        "empty-search-character",
        "2026-01-05T00:00:00.000Z",
        "dragon keyword",
        "Alice",
    );
    save_chat_payload_from_values(&repository, &root, "alice", "session", &payload, false)
        .await
        .expect("save payload");

    let results = repository
        .search_chats("   ", Some("alice"))
        .await
        .expect("empty search should list summaries");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].file_name, "session.jsonl");

    let index_path = root
        .join("user")
        .join("cache")
        .join("chat_summary_index_v1.json");
    let index_after_empty_search = fs::read_to_string(&index_path)
        .await
        .expect("read summary index after empty search");
    let parsed: Value =
        serde_json::from_str(&index_after_empty_search).expect("parse summary index");
    let entries = parsed
        .get("entries")
        .and_then(Value::as_array)
        .expect("entries should exist");
    assert_eq!(entries.len(), 1);
    assert!(
        entries[0]
            .get("fingerprint")
            .map(Value::is_null)
            .unwrap_or(true),
        "empty search should not materialize fingerprint"
    );

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn empty_group_search_uses_summary_without_fingerprint() {
    let (repository, root) = setup_repository().await;

    let payload = payload_with_message(
        "empty-search-group",
        "2026-01-05T00:00:00.000Z",
        "dragon keyword",
        "Group",
    );
    save_group_chat_payload_from_values(&repository, &root, "group-session", &payload, false)
        .await
        .expect("save group payload");

    let chat_ids = vec!["group-session".to_string()];
    let results = repository
        .search_group_chats("", Some(&chat_ids))
        .await
        .expect("empty group search should list summaries");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].file_name, "group-session.jsonl");

    let index_path = root
        .join("user")
        .join("cache")
        .join("chat_summary_index_v1.json");
    let index_after_empty_search = fs::read_to_string(&index_path)
        .await
        .expect("read summary index after empty group search");
    let parsed: Value =
        serde_json::from_str(&index_after_empty_search).expect("parse summary index");
    let entries = parsed
        .get("entries")
        .and_then(Value::as_array)
        .expect("entries should exist");
    assert_eq!(entries.len(), 1);
    assert!(
        entries[0]
            .get("fingerprint")
            .map(Value::is_null)
            .unwrap_or(true),
        "empty group search should not materialize fingerprint"
    );

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn character_chat_store_update_json_merges_and_replaces_values() {
    let (repository, root) = setup_repository().await;

    save_chat_payload_from_values(
        &repository,
        &root,
        "alice",
        "session",
        &payload_with_integrity("store-merge-a"),
        false,
    )
    .await
    .expect("save chat payload");

    repository
        .set_character_chat_store_json(
            "alice",
            "session",
            "my-ext",
            "index",
            json!({
                "a": 1,
                "nested": { "x": 1 },
            }),
        )
        .await
        .expect("seed store json");

    repository
        .update_character_chat_store_json(
            "alice",
            "session",
            "my-ext",
            "index",
            json!({
                "b": 2,
                "nested": { "y": 2 },
            }),
        )
        .await
        .expect("merge-update store json");

    let merged = repository
        .get_character_chat_store_json("alice", "session", "my-ext", "index")
        .await
        .expect("read merged store json");
    assert_eq!(
        merged,
        json!({
            "a": 1,
            "b": 2,
            "nested": { "x": 1, "y": 2 },
        })
    );

    repository
        .update_character_chat_store_json("alice", "session", "my-ext", "index", json!(42))
        .await
        .expect("replace store json");

    let replaced = repository
        .get_character_chat_store_json("alice", "session", "my-ext", "index")
        .await
        .expect("read replaced store json");
    assert_eq!(replaced, json!(42));

    repository
        .update_character_chat_store_json(
            "alice",
            "session",
            "my-ext",
            "missing",
            json!({ "created": true }),
        )
        .await
        .expect("upsert store json");

    let created = repository
        .get_character_chat_store_json("alice", "session", "my-ext", "missing")
        .await
        .expect("read created store json");
    assert_eq!(created, json!({ "created": true }));

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn character_chat_store_update_key_renames_entry() {
    let (repository, root) = setup_repository().await;

    save_chat_payload_from_values(
        &repository,
        &root,
        "alice",
        "session",
        &payload_with_integrity("store-rename-a"),
        false,
    )
    .await
    .expect("save chat payload");

    repository
        .set_character_chat_store_json("alice", "session", "my-ext", "old", json!({ "ok": true }))
        .await
        .expect("seed store json");

    repository
        .rename_character_chat_store_key("alice", "session", "my-ext", "old", "new")
        .await
        .expect("rename store key");

    let err = repository
        .get_character_chat_store_json("alice", "session", "my-ext", "old")
        .await
        .expect_err("old key should be gone");
    assert!(
        matches!(err, DomainError::NotFound(_)),
        "expected not found for old key"
    );

    let value = repository
        .get_character_chat_store_json("alice", "session", "my-ext", "new")
        .await
        .expect("read renamed key");
    assert_eq!(value, json!({ "ok": true }));

    let keys = repository
        .list_character_chat_store_keys("alice", "session", "my-ext")
        .await
        .expect("list keys");
    assert_eq!(keys, vec![String::from("new")]);

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn group_chat_store_update_json_and_key_work() {
    let (repository, root) = setup_repository().await;

    save_group_chat_payload_from_values(
        &repository,
        &root,
        "group-session",
        &payload_with_integrity("store-group-a"),
        false,
    )
    .await
    .expect("save group chat payload");

    repository
        .update_group_chat_store_json(
            "group-session",
            "my-ext",
            "index",
            json!({ "hello": "world" }),
        )
        .await
        .expect("upsert group store json");

    repository
        .rename_group_chat_store_key("group-session", "my-ext", "index", "index-v2")
        .await
        .expect("rename group store key");

    let value = repository
        .get_group_chat_store_json("group-session", "my-ext", "index-v2")
        .await
        .expect("read renamed group key");
    assert_eq!(value, json!({ "hello": "world" }));

    let _ = fs::remove_dir_all(&root).await;
}

async fn save_chat_payload_from_values(
    repository: &FileChatRepository,
    root: &PathBuf,
    character_name: &str,
    file_name: &str,
    payload: &[Value],
    force: bool,
) -> Result<(), DomainError> {
    let source_path = root.join(format!("chat-payload-{}.jsonl", random::<u64>()));
    fs::write(&source_path, payload_to_jsonl(payload))
        .await
        .expect("write chat payload source file");

    repository
        .save_chat_payload_from_path(character_name, file_name, &source_path, force)
        .await
}

#[tokio::test]
async fn patch_chat_payload_windowed_appends_and_rewrites_tail() {
    let (repository, root) = setup_repository().await;

    let character_name = "alice";
    let file_name = "session";

    let payload = vec![
        payload_with_integrity("patch-a")[0].clone(),
        json!({
            "name": "User",
            "is_user": true,
            "send_date": "2026-01-01T00:00:00.000Z",
            "mes": "hello",
            "extra": {},
        }),
        json!({
            "name": "Alice",
            "is_user": false,
            "send_date": "2026-01-01T00:00:01.000Z",
            "mes": "hi",
            "extra": {},
        }),
    ];

    save_chat_payload_from_values(
        &repository,
        &root,
        character_name,
        file_name,
        &payload,
        false,
    )
    .await
    .expect("save initial payload");

    let tail = repository
        .get_chat_payload_tail_lines(character_name, file_name, 100)
        .await
        .expect("get tail");
    assert_eq!(tail.lines.len(), 2);

    let new_message = json!({
        "name": "User",
        "is_user": true,
        "send_date": "2026-01-01T00:00:02.000Z",
        "mes": "more",
        "extra": {},
    });
    let new_line = serde_json::to_string(&new_message).expect("serialize new line");

    let cursor = repository
        .patch_chat_payload_windowed(
            character_name,
            file_name,
            tail.cursor,
            tail.header.clone(),
            ChatPayloadPatchOp::Append {
                lines: vec![new_line.clone()],
            },
            false,
        )
        .await
        .expect("append patch");

    let bytes = repository
        .get_chat_payload_bytes(character_name, file_name)
        .await
        .expect("read patched payload bytes");
    let text = String::from_utf8(bytes).expect("payload should be utf8");
    let values = text
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("parse json line"))
        .collect::<Vec<_>>();
    assert_eq!(values.len(), 4);
    assert_eq!(values[3], new_message);

    let updated_message = json!({
        "name": "User",
        "is_user": true,
        "send_date": "2026-01-01T00:00:02.000Z",
        "mes": "more!",
        "extra": {},
    });
    let updated_line = serde_json::to_string(&updated_message).expect("serialize updated line");

    let cursor = repository
        .patch_chat_payload_windowed(
            character_name,
            file_name,
            cursor,
            tail.header,
            ChatPayloadPatchOp::RewriteFromIndex {
                start_index: 2,
                lines: vec![updated_line],
            },
            false,
        )
        .await
        .expect("rewrite tail from index");

    let bytes = repository
        .get_chat_payload_bytes(character_name, file_name)
        .await
        .expect("read rewritten payload bytes");
    let text = String::from_utf8(bytes).expect("payload should be utf8");
    let values = text
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("parse json line"))
        .collect::<Vec<_>>();
    assert_eq!(values.len(), 4);
    assert_eq!(values[3], updated_message);

    repository
        .patch_chat_payload_windowed(
            character_name,
            file_name,
            cursor,
            serde_json::to_string(&values[0]).expect("serialize header"),
            ChatPayloadPatchOp::RewriteFromIndex {
                start_index: 1,
                lines: Vec::new(),
            },
            false,
        )
        .await
        .expect("truncate tail from index");

    let bytes = repository
        .get_chat_payload_bytes(character_name, file_name)
        .await
        .expect("read truncated payload bytes");
    let text = String::from_utf8(bytes).expect("payload should be utf8");
    let values = text
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("parse json line"))
        .collect::<Vec<_>>();
    assert_eq!(values.len(), 2);

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn patch_chat_payload_windowed_rejects_missing_integrity_when_existing_has_one() {
    let (repository, root) = setup_repository().await;

    let character_name = "alice";
    let file_name = "session";
    let payload = payload_with_integrity("patch-a");

    save_chat_payload_from_values(
        &repository,
        &root,
        character_name,
        file_name,
        &payload,
        false,
    )
    .await
    .expect("save initial payload");

    let tail = repository
        .get_chat_payload_tail_lines(character_name, file_name, 100)
        .await
        .expect("get tail");
    let missing_integrity_header =
        serde_json::to_string(&payload_without_integrity()[0]).expect("serialize header");

    let error = repository
        .patch_chat_payload_windowed(
            character_name,
            file_name,
            tail.cursor,
            missing_integrity_header,
            ChatPayloadPatchOp::Append { lines: Vec::new() },
            false,
        )
        .await
        .expect_err("patch should fail when incoming integrity is missing");
    assert!(matches!(error, DomainError::InvalidData(message) if message == "integrity"));

    let _ = fs::remove_dir_all(&root).await;
}

async fn save_group_chat_payload_from_values(
    repository: &FileChatRepository,
    root: &PathBuf,
    chat_id: &str,
    payload: &[Value],
    force: bool,
) -> Result<(), DomainError> {
    let source_path = root.join(format!("group-chat-payload-{}.jsonl", random::<u64>()));
    fs::write(&source_path, payload_to_jsonl(payload))
        .await
        .expect("write group chat payload source file");

    repository
        .save_group_chat_payload_from_path(chat_id, &source_path, force)
        .await
}

fn payload_to_jsonl(payload: &[Value]) -> String {
    payload
        .iter()
        .map(|item| serde_json::to_string(item).expect("serialize line"))
        .collect::<Vec<_>>()
        .join("\n")
}
