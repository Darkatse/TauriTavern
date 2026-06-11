use std::path::{Path, PathBuf};
use std::sync::Arc;

use rand::random;
use serde_json::{Value, json};
use tokio::fs;

use crate::domain::errors::DomainError;
use crate::domain::models::filename::sanitize_filename;
use crate::domain::repositories::chat_repository::{
    ChatMessageRole, ChatMessageSearchFilters, ChatMessageSearchQuery, ChatPayloadPatchOp,
    ChatRepository, PinnedCharacterChat, PinnedGroupChat,
};
use crate::domain::repositories::group_chat_repository::GroupChatRepository;
use crate::infrastructure::repositories::chat_directory_identity::new_shared_chat_alias_store_for_user_dir;

use super::FileChatRepository;

fn unique_temp_root() -> PathBuf {
    std::env::temp_dir().join(format!("tauritavern-chat-repo-{}", random::<u64>()))
}

async fn setup_repository() -> (FileChatRepository, PathBuf) {
    let root = unique_temp_root();
    let repository = repository_for_root(&root);

    repository
        .ensure_directory_exists()
        .await
        .expect("create chat directories");

    (repository, root)
}

fn repository_for_root(root: &Path) -> FileChatRepository {
    FileChatRepository::with_chat_aliases(
        root.join("characters"),
        root.join("chats"),
        root.join("group chats"),
        root.join("backups"),
        new_shared_chat_alias_store_for_user_dir(root),
    )
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
fn group_backup_key_uses_committed_chat_file_stem() {
    assert_eq!(
        FileChatRepository::get_group_backup_key("group/name.jsonl").expect("group backup key"),
        "group:groupname"
    );
    assert_eq!(
        FileChatRepository::get_group_backup_key("Story.JSONL").expect("group backup key"),
        "group:Story.JSONL"
    );
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
async fn save_chat_payload_from_path_preserves_unicode_and_upstream_spacing() {
    let (repository, root) = setup_repository().await;

    let character_name = "角色";
    let file_name = " 中文会话 .jsonl";
    let raw_payload = payload_to_jsonl(&payload_with_integrity("unicode-file-name"));
    let source = root.join("unicode-chat-source.jsonl");
    fs::write(&source, &raw_payload)
        .await
        .expect("write unicode chat payload source");

    repository
        .save_chat_payload_from_path(character_name, file_name, &source, false)
        .await
        .expect("save payload with unicode chat file name");

    let expected_path = root
        .join("chats")
        .join(sanitize_filename(character_name))
        .join(" 中文会话 .jsonl");
    assert!(expected_path.exists());
    assert!(
        !root
            .join("chats")
            .join(sanitize_filename(character_name))
            .join("中文会话.jsonl")
            .exists()
    );

    let loaded_bytes = repository
        .get_chat_payload_bytes(character_name, " 中文会话 ")
        .await
        .expect("load unicode chat payload bytes");
    assert_eq!(loaded_bytes, raw_payload.as_bytes());

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn save_chat_payload_from_path_keeps_uppercase_jsonl_as_stem_text() {
    let (repository, root) = setup_repository().await;

    let raw_payload = payload_to_jsonl(&payload_with_integrity("uppercase-jsonl-stem"));
    let source = root.join("uppercase-jsonl-source.jsonl");
    fs::write(&source, &raw_payload)
        .await
        .expect("write uppercase jsonl chat payload source");

    repository
        .save_chat_payload_from_path("alice", "Story.JSONL", &source, false)
        .await
        .expect("save payload with uppercase JSONL in stem");

    assert!(
        root.join("chats")
            .join("alice")
            .join("Story.JSONL.jsonl")
            .exists()
    );
    assert!(
        !root
            .join("chats")
            .join("alice")
            .join("Story.jsonl")
            .exists()
    );

    let loaded_bytes = repository
        .get_chat_payload_bytes("alice", "Story.JSONL")
        .await
        .expect("load uppercase JSONL stem payload bytes");
    assert_eq!(loaded_bytes, raw_payload.as_bytes());

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn legacy_hash_truncated_chat_dir_is_read_through_alias() {
    let (repository, root) = setup_repository().await;

    let characters_dir = root.join("characters");
    fs::create_dir_all(&characters_dir)
        .await
        .expect("create characters directory");
    fs::write(characters_dir.join("Alice#1.png"), b"")
        .await
        .expect("create exact character card");

    let legacy_dir = root.join("chats").join("Alice");
    fs::create_dir_all(&legacy_dir)
        .await
        .expect("create legacy chat dir");
    let raw_payload = payload_to_jsonl(&payload_with_integrity("legacy-hash"));
    fs::write(legacy_dir.join("session.jsonl"), &raw_payload)
        .await
        .expect("write legacy chat payload");

    let loaded = repository
        .get_chat_payload_bytes("Alice#1", "session")
        .await
        .expect("read legacy payload through exact identity");
    assert_eq!(loaded, raw_payload.as_bytes());

    let aliases = fs::read_to_string(root.join("user").join("cache").join("chat_aliases_v1.json"))
        .await
        .expect("read alias file");
    assert!(aliases.contains("\"Alice#1\""));
    assert!(aliases.contains("\"dir\": \"Alice\""));

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn legacy_alias_keeps_new_saves_in_existing_physical_dir() {
    let (repository, root) = setup_repository().await;

    let legacy_dir = root.join("chats").join("Alice");
    fs::create_dir_all(&legacy_dir)
        .await
        .expect("create legacy chat dir");
    fs::write(
        legacy_dir.join("session.jsonl"),
        payload_to_jsonl(&payload_with_integrity("legacy-save-a")),
    )
    .await
    .expect("write legacy payload");

    let raw_payload = payload_to_jsonl(&payload_with_integrity("legacy-save-b"));
    let source = root.join("legacy-save-source.jsonl");
    fs::write(&source, &raw_payload)
        .await
        .expect("write new payload source");

    repository
        .save_chat_payload_from_path("Alice#1", "followup", &source, false)
        .await
        .expect("save through exact identity into legacy dir");

    assert!(legacy_dir.join("followup.jsonl").exists());
    assert!(
        !root
            .join("chats")
            .join("Alice#1")
            .join("followup.jsonl")
            .exists()
    );

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn legacy_percent_decoded_basename_dir_is_read_for_exact_stem() {
    let (repository, root) = setup_repository().await;

    let legacy_dir = root.join("chats").join("B");
    fs::create_dir_all(&legacy_dir)
        .await
        .expect("create legacy decoded basename dir");
    fs::write(
        legacy_dir.join("session.jsonl"),
        payload_to_jsonl(&payload_with_integrity("legacy-percent")),
    )
    .await
    .expect("write decoded basename payload");

    let summaries = repository
        .list_chat_summaries(Some("Alice%2FB"), false)
        .await
        .expect("list summaries through decoded legacy alias");
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].character_name, "Alice%2FB");
    assert_eq!(summaries[0].file_name, "session.jsonl");

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn alias_store_merges_concurrent_repository_instances() {
    let (repository_a, root) = setup_repository().await;
    let repository_b = repository_for_root(&root);
    repository_b
        .ensure_directory_exists()
        .await
        .expect("create second repository dirs");

    let _ = repository_b
        .get_chat_payload_bytes("Warm#1", "missing")
        .await
        .expect_err("warm stale alias store without writing");

    for (dir_name, integrity) in [("Alice", "alias-merge-a"), ("Bob", "alias-merge-b")] {
        let legacy_dir = root.join("chats").join(dir_name);
        fs::create_dir_all(&legacy_dir)
            .await
            .expect("create legacy chat dir");
        fs::write(
            legacy_dir.join("session.jsonl"),
            payload_to_jsonl(&payload_with_integrity(integrity)),
        )
        .await
        .expect("write legacy payload");
    }

    repository_a
        .get_chat_payload_bytes("Alice#1", "session")
        .await
        .expect("repository A writes first alias");
    repository_b
        .get_chat_payload_bytes("Bob#1", "session")
        .await
        .expect("repository B merges existing alias before writing");

    let aliases = fs::read_to_string(root.join("user").join("cache").join("chat_aliases_v1.json"))
        .await
        .expect("read alias file");
    assert!(aliases.contains("\"Alice#1\""));
    assert!(aliases.contains("\"dir\": \"Alice\""));
    assert!(aliases.contains("\"Bob#1\""));
    assert!(aliases.contains("\"dir\": \"Bob\""));

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn shared_alias_store_serializes_concurrent_repository_writes() {
    let root = unique_temp_root();
    let chat_aliases = new_shared_chat_alias_store_for_user_dir(&root);
    let repository_a = FileChatRepository::with_chat_aliases(
        root.join("characters"),
        root.join("chats"),
        root.join("group chats"),
        root.join("backups"),
        chat_aliases.clone(),
    );
    let repository_b = FileChatRepository::with_chat_aliases(
        root.join("characters"),
        root.join("chats"),
        root.join("group chats"),
        root.join("backups"),
        chat_aliases,
    );

    repository_a
        .ensure_directory_exists()
        .await
        .expect("create shared repository dirs");

    for (dir_name, integrity) in [("Alice", "shared-alias-a"), ("Bob", "shared-alias-b")] {
        let legacy_dir = root.join("chats").join(dir_name);
        fs::create_dir_all(&legacy_dir)
            .await
            .expect("create legacy chat dir");
        fs::write(
            legacy_dir.join("session.jsonl"),
            payload_to_jsonl(&payload_with_integrity(integrity)),
        )
        .await
        .expect("write legacy payload");
    }

    let (loaded_a, loaded_b) = tokio::try_join!(
        repository_a.get_chat_payload_bytes("Alice#1", "session"),
        repository_b.get_chat_payload_bytes("Bob#1", "session")
    )
    .expect("shared alias store writes both aliases");
    assert_eq!(
        loaded_a,
        payload_to_jsonl(&payload_with_integrity("shared-alias-a")).as_bytes()
    );
    assert_eq!(
        loaded_b,
        payload_to_jsonl(&payload_with_integrity("shared-alias-b")).as_bytes()
    );

    let aliases = fs::read_to_string(root.join("user").join("cache").join("chat_aliases_v1.json"))
        .await
        .expect("read alias file");
    assert!(aliases.contains("\"Alice#1\""));
    assert!(aliases.contains("\"dir\": \"Alice\""));
    assert!(aliases.contains("\"Bob#1\""));
    assert!(aliases.contains("\"dir\": \"Bob\""));

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn legacy_candidate_does_not_steal_an_existing_character_dir() {
    let (repository, root) = setup_repository().await;

    let characters_dir = root.join("characters");
    fs::create_dir_all(&characters_dir)
        .await
        .expect("create characters directory");
    fs::write(characters_dir.join("Alice.png"), b"")
        .await
        .expect("create legacy candidate character card");
    fs::write(characters_dir.join("Alice#1.png"), b"")
        .await
        .expect("create exact character card");

    let legacy_dir = root.join("chats").join("Alice");
    fs::create_dir_all(&legacy_dir)
        .await
        .expect("create candidate chat dir");
    fs::write(
        legacy_dir.join("session.jsonl"),
        payload_to_jsonl(&payload_with_integrity("legacy-conflict")),
    )
    .await
    .expect("write candidate payload");

    let error = repository
        .get_chat_payload_bytes("Alice#1", "session")
        .await
        .expect_err("conflicting legacy candidate should not be used");
    assert!(matches!(error, DomainError::NotFound(_)));
    assert!(
        !root
            .join("user")
            .join("cache")
            .join("chat_aliases_v1.json")
            .exists()
    );

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn save_chat_payload_from_path_rejects_chat_file_names_that_sanitize_to_empty() {
    let (repository, root) = setup_repository().await;

    let raw_payload = payload_to_jsonl(&payload_with_integrity("invalid-file-name"));
    let source = root.join("invalid-chat-name-source.jsonl");
    fs::write(&source, &raw_payload)
        .await
        .expect("write invalid chat payload source");

    let error = repository
        .save_chat_payload_from_path("alice", "*.jsonl", &source, false)
        .await
        .expect_err("empty sanitized chat file name should fail");

    assert!(
        matches!(error, DomainError::InvalidData(message) if message == "Invalid chat file name")
    );
    assert!(!root.join("chats").join("alice").join("chat.jsonl").exists());
    assert!(!root.join("chats").join("alice").join(".jsonl").exists());

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn save_chat_payload_from_path_rejects_names_that_lose_jsonl_suffix_after_truncation() {
    let (repository, root) = setup_repository().await;

    let raw_payload = payload_to_jsonl(&payload_with_integrity("truncated-extension"));
    let source = root.join("truncated-extension-source.jsonl");
    fs::write(&source, &raw_payload)
        .await
        .expect("write chat payload source");

    let overlong_file_name = "a".repeat(250);
    let error = repository
        .save_chat_payload_from_path("alice", &overlong_file_name, &source, false)
        .await
        .expect_err("chat file name must keep a complete jsonl suffix");

    assert!(
        matches!(error, DomainError::InvalidData(message) if message == "Invalid chat file name")
    );
    assert!(!root.join("chats").join("alice").exists());

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
async fn save_group_chat_payload_from_path_rejects_ids_that_sanitize_to_empty() {
    let (repository, root) = setup_repository().await;

    let raw_payload = payload_to_jsonl(&payload_with_integrity("group-invalid-id"));
    let source = root.join("group-invalid-id-source.jsonl");
    fs::write(&source, &raw_payload)
        .await
        .expect("write group payload source");

    let error = repository
        .save_group_chat_payload_from_path("*.jsonl", &source, false)
        .await
        .expect_err("empty sanitized group chat id should fail");

    assert!(
        matches!(error, DomainError::InvalidData(message) if message == "Invalid chat file name")
    );
    assert!(!root.join("group chats").join("chat.jsonl").exists());
    assert!(!root.join("group chats").join(".jsonl").exists());

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
async fn import_chat_payload_preserves_jsonl_suffix_for_long_character_names() {
    let (repository, root) = setup_repository().await;

    let import_path = root.join("long-import.jsonl");
    let import_content = payload_to_jsonl(&payload_with_integrity("import-long"));
    fs::write(&import_path, import_content)
        .await
        .expect("write long import file");

    let long_display_name = "角色".repeat(130);
    let first = repository
        .import_chat_payload("alice", &long_display_name, "User", &import_path, "jsonl")
        .await
        .expect("first import with long display name");
    let second = repository
        .import_chat_payload("alice", &long_display_name, "User", &import_path, "jsonl")
        .await
        .expect("second import with long display name");

    assert_eq!(first.len(), 1);
    assert_eq!(second.len(), 1);
    assert_ne!(first[0], second[0]);
    for file_name in [&first[0], &second[0]] {
        assert!(file_name.ends_with(".jsonl"));
        assert!(file_name.len() <= 255);
        assert!(root.join("chats").join("alice").join(file_name).exists());
    }

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

    let committed_file_name = repository
        .rename_chat("alice", "session", "session-renamed.jsonl")
        .await
        .expect("rename chat");
    assert_eq!(committed_file_name, "session-renamed");

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
async fn rename_chat_rejects_empty_sanitized_target_without_fallback() {
    let (repository, root) = setup_repository().await;

    save_chat_payload_from_values(
        &repository,
        &root,
        "alice",
        "session",
        &payload_with_integrity("rename-invalid-target"),
        false,
    )
    .await
    .expect("save payload");

    let error = repository
        .rename_chat("alice", "session", "*.jsonl")
        .await
        .expect_err("empty sanitized rename target should fail");

    assert!(
        matches!(error, DomainError::InvalidData(message) if message == "Invalid chat file name")
    );
    assert!(
        root.join("chats")
            .join("alice")
            .join("session.jsonl")
            .exists()
    );
    assert!(!root.join("chats").join("alice").join("chat.jsonl").exists());

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn rename_chat_rejects_existing_target_without_overwrite() {
    let (repository, root) = setup_repository().await;

    save_chat_payload_from_values(
        &repository,
        &root,
        "alice",
        "session",
        &payload_with_integrity("rename-source"),
        false,
    )
    .await
    .expect("save source payload");
    save_chat_payload_from_values(
        &repository,
        &root,
        "alice",
        "session-renamed",
        &payload_with_integrity("rename-target"),
        false,
    )
    .await
    .expect("save target payload");

    let error = repository
        .rename_chat("alice", "session", "session-renamed")
        .await
        .expect_err("existing target should fail");

    assert!(
        matches!(error, DomainError::InvalidData(message) if message.contains("Chat already exists"))
    );
    assert!(
        root.join("chats")
            .join("alice")
            .join("session.jsonl")
            .exists()
    );
    assert!(
        root.join("chats")
            .join("alice")
            .join("session-renamed.jsonl")
            .exists()
    );

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn rename_group_chat_returns_committed_file_stem() {
    let (repository, root) = setup_repository().await;
    let payload = payload_with_integrity("group-rename-a");

    save_group_chat_payload_from_values(&repository, &root, "group-session", &payload, false)
        .await
        .expect("save group payload");

    let committed_file_name = repository
        .rename_group_chat_payload("group-session", "group-session-renamed.jsonl")
        .await
        .expect("rename group chat");

    assert_eq!(committed_file_name, "group-session-renamed");
    assert!(
        root.join("group chats")
            .join("group-session-renamed.jsonl")
            .exists()
    );
    assert!(
        !root
            .join("group chats")
            .join("group-session.jsonl")
            .exists()
    );

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn rename_group_chat_rejects_existing_target_without_overwrite() {
    let (repository, root) = setup_repository().await;

    save_group_chat_payload_from_values(
        &repository,
        &root,
        "group-session",
        &payload_with_integrity("group-rename-source"),
        false,
    )
    .await
    .expect("save source group payload");
    save_group_chat_payload_from_values(
        &repository,
        &root,
        "group-session-renamed",
        &payload_with_integrity("group-rename-target"),
        false,
    )
    .await
    .expect("save target group payload");

    let error = repository
        .rename_group_chat_payload("group-session", "group-session-renamed")
        .await
        .expect_err("existing target should fail");

    assert!(
        matches!(error, DomainError::InvalidData(message) if message.contains("Group chat already exists"))
    );
    assert!(
        root.join("group chats")
            .join("group-session.jsonl")
            .exists()
    );
    assert!(
        root.join("group chats")
            .join("group-session-renamed.jsonl")
            .exists()
    );

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
async fn list_chat_summaries_counts_large_crlf_jsonl_without_fingerprint() {
    let (repository, root) = setup_repository().await;

    let chat_dir = root.join("chats").join("alice");
    fs::create_dir_all(&chat_dir)
        .await
        .expect("create character chat dir");

    let header = json!({
        "chat_metadata": {
            "integrity": "large-summary",
            "chat_id_hash": 77,
        },
        "user_name": "unused",
        "character_name": "unused",
    });
    let large_middle_message = json!({
        "name": "User",
        "is_user": true,
        "send_date": "2026-01-01T00:00:00.000Z",
        "mes": "x".repeat(70_000),
        "extra": {},
    });
    let tail_message = json!({
        "name": "Alice",
        "is_user": false,
        "send_date": "2026-01-02T00:00:00.000Z",
        "mes": "tail response",
        "extra": {},
    });

    let raw_jsonl = [
        serde_json::to_string(&header).expect("serialize header"),
        String::new(),
        serde_json::to_string(&large_middle_message).expect("serialize large message"),
        "   \t".to_string(),
        serde_json::to_string(&tail_message).expect("serialize tail message"),
    ]
    .join("\r\n");
    fs::write(chat_dir.join("session.jsonl"), raw_jsonl)
        .await
        .expect("write raw crlf jsonl");

    let summaries = repository
        .list_chat_summaries(Some("alice"), true)
        .await
        .expect("list chat summaries");

    assert_eq!(summaries.len(), 1);
    let summary = &summaries[0];
    assert_eq!(summary.character_name, "alice");
    assert_eq!(summary.file_name, "session.jsonl");
    assert_eq!(summary.message_count, 2);
    assert_eq!(summary.preview, "tail response");
    assert_eq!(summary.chat_id.as_deref(), Some("77"));

    let index_path = root
        .join("user")
        .join("cache")
        .join("chat_summary_index_v1.json");
    let index_after_summary = fs::read_to_string(&index_path)
        .await
        .expect("read summary index after summary list");
    let parsed: Value = serde_json::from_str(&index_after_summary).expect("parse summary index");
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
        "summary listing should not materialize fingerprint"
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
async fn read_character_chat_messages_returns_selected_messages_and_total_count() {
    let (repository, root) = setup_repository().await;

    let payload = vec![
        json!({
            "chat_metadata": {
                "integrity": "read-a",
            },
            "user_name": "unused",
            "character_name": "unused",
        }),
        json!({
            "name": "User",
            "is_user": true,
            "is_system": false,
            "send_date": "2026-01-01T00:00:00.000Z",
            "mes": "first message",
            "extra": {},
        }),
        json!({
            "name": "Alice",
            "is_user": false,
            "is_system": false,
            "send_date": "2026-01-01T00:00:01.000Z",
            "mes": "second message",
            "extra": {},
        }),
        json!({
            "name": "System",
            "is_user": false,
            "is_system": true,
            "send_date": "2026-01-01T00:00:02.000Z",
            "mes": "system message",
            "extra": {},
        }),
    ];

    save_chat_payload_from_values(&repository, &root, "alice", "session", &payload, false)
        .await
        .expect("save payload");

    let result = repository
        .read_character_chat_messages("alice", "session", &[2, 0])
        .await
        .expect("read messages");

    assert_eq!(result.total_messages, 3);
    assert_eq!(result.messages.len(), 2);
    assert_eq!(result.messages[0].index, 0);
    assert_eq!(result.messages[0].role, ChatMessageRole::User);
    assert_eq!(result.messages[0].text, "first message");
    assert_eq!(result.messages[1].index, 2);
    assert_eq!(result.messages[1].role, ChatMessageRole::System);

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn read_group_chat_messages_uses_message_indexes_without_header() {
    let (repository, root) = setup_repository().await;

    let payload = vec![
        json!({
            "chat_metadata": {},
            "user_name": "unused",
            "character_name": "unused",
        }),
        json!({
            "name": "User",
            "is_user": true,
            "is_system": false,
            "send_date": "2026-01-01T00:00:00.000Z",
            "mes": "group first",
            "extra": {},
        }),
        json!({
            "name": "Alice",
            "is_user": false,
            "is_system": false,
            "send_date": "2026-01-01T00:00:01.000Z",
            "mes": "group second",
            "extra": {},
        }),
    ];

    save_group_chat_payload_from_values(&repository, &root, "group-session", &payload, false)
        .await
        .expect("save group payload");

    let result = repository
        .read_group_chat_messages("group-session", &[1])
        .await
        .expect("read group message");

    assert_eq!(result.total_messages, 2);
    assert_eq!(result.messages.len(), 1);
    assert_eq!(result.messages[0].index, 1);
    assert_eq!(result.messages[0].text, "group second");

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

    let reloaded_repository = repository_for_root(&root);
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
async fn list_recent_chat_summaries_preserves_upstream_spacing_in_pinned_keys() {
    let (repository, root) = setup_repository().await;
    let characters_dir = root.join("characters");
    fs::create_dir_all(&characters_dir)
        .await
        .expect("create characters directory");
    fs::write(characters_dir.join("alice.png"), b"")
        .await
        .expect("create alice card");

    let plain_payload =
        payload_with_message("recent-plain", "2026-01-01T00:00:00.000Z", "plain", "Alice");
    save_chat_payload_from_values(
        &repository,
        &root,
        "alice",
        "session",
        &plain_payload,
        false,
    )
    .await
    .expect("save plain chat");

    let spaced_payload = payload_with_message(
        "recent-spaced",
        "2026-01-02T00:00:00.000Z",
        "spaced",
        "Alice",
    );
    save_chat_payload_from_values(
        &repository,
        &root,
        "alice",
        " session",
        &spaced_payload,
        false,
    )
    .await
    .expect("save spaced chat");

    let pinned = vec![PinnedCharacterChat {
        character_name: "alice".to_string(),
        file_name: " session".to_string(),
    }];
    let results = repository
        .list_recent_chat_summaries(None, false, 1, &pinned)
        .await
        .expect("list recent summaries");

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].file_name, " session.jsonl");

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
async fn list_recent_group_chat_summaries_preserves_upstream_spacing_in_pinned_keys() {
    let (repository, root) = setup_repository().await;

    let plain_payload = payload_with_message(
        "group-recent-plain",
        "2026-01-01T00:00:00.000Z",
        "plain group",
        "Group",
    );
    save_group_chat_payload_from_values(&repository, &root, "group", &plain_payload, false)
        .await
        .expect("save plain group chat");

    let spaced_payload = payload_with_message(
        "group-recent-spaced",
        "2026-01-02T00:00:00.000Z",
        "spaced group",
        "Group",
    );
    save_group_chat_payload_from_values(&repository, &root, " group", &spaced_payload, false)
        .await
        .expect("save spaced group chat");

    let pinned = vec![PinnedGroupChat {
        chat_id: " group".to_string(),
    }];
    let results = repository
        .list_recent_group_chat_summaries(None, false, 1, &pinned)
        .await
        .expect("list recent group summaries");

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].file_name, " group.jsonl");

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
            2,
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
            3,
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
            3,
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
            tail.lines.len(),
            false,
        )
        .await
        .expect_err("patch should fail when incoming integrity is missing");
    assert!(matches!(error, DomainError::InvalidData(message) if message == "integrity"));

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn patch_chat_payload_windowed_rejects_window_baseline_mismatch() {
    let (repository, root) = setup_repository().await;

    let character_name = "alice";
    let file_name = "session";

    let mut payload = vec![payload_with_integrity("baseline-a")[0].clone()];
    for index in 0..5 {
        payload.push(json!({
            "name": "User",
            "is_user": true,
            "send_date": format!("2026-01-01T00:00:0{}.000Z", index),
            "mes": format!("message {}", index),
            "extra": {},
        }));
    }

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

    let windowed_tail = repository
        .get_chat_payload_tail_lines(character_name, file_name, 2)
        .await
        .expect("get windowed tail");
    assert_eq!(windowed_tail.lines.len(), 2);

    let full_tail = repository
        .get_chat_payload_tail_lines(character_name, file_name, 100)
        .await
        .expect("get full tail");
    assert_eq!(full_tail.lines.len(), 5);

    let new_line = serde_json::to_string(&json!({
        "name": "User",
        "is_user": true,
        "send_date": "2026-01-01T00:00:09.000Z",
        "mes": "appended",
        "extra": {},
    }))
    .expect("serialize new line");

    // Append with a stale full-mode offset but a valid file signature: before the
    // baseline contract this passed silently and returned the bad offset re-signed
    // with the new file signature.
    let error = repository
        .patch_chat_payload_windowed(
            character_name,
            file_name,
            full_tail.cursor,
            windowed_tail.header.clone(),
            ChatPayloadPatchOp::Append {
                lines: vec![new_line.clone()],
            },
            windowed_tail.lines.len(),
            false,
        )
        .await
        .expect_err("append with stale offset must be rejected");
    assert!(
        format!("{}", error).contains("Window baseline mismatch"),
        "error should mention the window baseline, got: {}",
        error
    );

    // RewriteFromIndex with a wrong declared baseline is rejected too.
    let error = repository
        .patch_chat_payload_windowed(
            character_name,
            file_name,
            windowed_tail.cursor,
            windowed_tail.header.clone(),
            ChatPayloadPatchOp::RewriteFromIndex {
                start_index: 0,
                lines: vec![new_line.clone()],
            },
            4,
            false,
        )
        .await
        .expect_err("rewrite with wrong baseline must be rejected");
    assert!(
        format!("{}", error).contains("Window baseline mismatch"),
        "error should mention the window baseline, got: {}",
        error
    );

    // File untouched by the rejected writes.
    let bytes = repository
        .get_chat_payload_bytes(character_name, file_name)
        .await
        .expect("read payload bytes");
    assert_eq!(
        String::from_utf8(bytes).expect("utf8").lines().count(),
        6,
        "rejected writes must not modify the file"
    );

    // The same append with the correct baseline succeeds.
    repository
        .patch_chat_payload_windowed(
            character_name,
            file_name,
            windowed_tail.cursor,
            windowed_tail.header,
            ChatPayloadPatchOp::Append {
                lines: vec![new_line],
            },
            windowed_tail.lines.len(),
            false,
        )
        .await
        .expect("append with correct baseline");

    let bytes = repository
        .get_chat_payload_bytes(character_name, file_name)
        .await
        .expect("read payload bytes after append");
    assert_eq!(String::from_utf8(bytes).expect("utf8").lines().count(), 7);

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

// ============================================================================
// Data loss reproduction: mode switch (full ↔ windowed) with stale cursor
// ============================================================================

fn generate_large_payload(num_messages: usize, integrity: &str) -> Vec<Value> {
    let mut payload = vec![json!({
        "chat_metadata": {
            "integrity": integrity,
        },
        "user_name": "User",
        "character_name": "Character",
    })];

    for i in 0..num_messages {
        let is_user = i % 2 == 0;
        let large_content = format!("Message {} content. {}", i, "X".repeat(2000));
        payload.push(json!({
            "name": if is_user { "User" } else { "Character" },
            "is_user": is_user,
            "send_date": format!("2026-06-{:02}T{:02}:{:02}:00.000Z",
                1 + (i / 48).min(29),
                (i % 48) / 2,
                if i % 2 == 0 { 0 } else { 30 }
            ),
            "mes": large_content,
            "extra": {},
        }));
    }

    payload
}

/// TEST 1: Full → Windowed mode switch.
/// Full-mode cursor (offset=header_end) used with only 50 windowed messages.
/// Expected: 250 messages silently destroyed.
#[tokio::test]
async fn repro_full_to_windowed_mode_switch_data_loss() {
    let (repository, root) = setup_repository().await;

    let character_name = "test-char";
    let file_name = "session";
    let num_messages: usize = 300;
    let window_size: usize = 50;

    let payload = generate_large_payload(num_messages, "repro-test");
    save_chat_payload_from_values(
        &repository,
        &root,
        character_name,
        file_name,
        &payload,
        false,
    )
    .await
    .expect("save 300-message payload");

    // Load full mode
    let full_tail = repository
        .get_chat_payload_tail_lines(character_name, file_name, 10000)
        .await
        .expect("get full tail");
    let cursor_full = full_tail.cursor;
    assert_eq!(full_tail.lines.len(), num_messages);
    assert!(!full_tail.has_more_before);

    // Load windowed mode (without modifying file)
    let windowed_tail = repository
        .get_chat_payload_tail_lines(character_name, file_name, window_size)
        .await
        .expect("get windowed tail");
    assert_eq!(windowed_tail.lines.len(), window_size);
    assert!(windowed_tail.has_more_before);

    // Verify: same signature, different offsets
    assert_eq!(cursor_full.size, windowed_tail.cursor.size);
    assert_eq!(
        cursor_full.modified_millis,
        windowed_tail.cursor.modified_millis
    );
    assert!(
        cursor_full.offset < windowed_tail.cursor.offset,
        "full offset ({}) < windowed offset ({})",
        cursor_full.offset,
        windowed_tail.cursor.offset
    );

    println!("=== Full → Windowed ===");
    println!("Full cursor offset:     {}", cursor_full.offset);
    println!("Windowed cursor offset: {}", windowed_tail.cursor.offset);
    println!(
        "Data at risk: {} bytes",
        windowed_tail.cursor.offset - cursor_full.offset
    );

    // THE BUG: save with full cursor + only windowed messages
    let save_result = repository
        .save_chat_payload_windowed(
            character_name,
            file_name,
            cursor_full,
            full_tail.header,
            windowed_tail.lines,
            window_size,
            false,
        )
        .await;

    // Verify: save must be rejected to prevent data loss
    assert!(
        save_result.is_err(),
        "Save with mismatched cursor/lines should be rejected"
    );
    let err_msg = format!("{}", save_result.unwrap_err());
    assert!(
        err_msg.contains("Window baseline mismatch"),
        "Error should mention the window baseline, got: {}",
        err_msg
    );

    // Verify file is intact
    let bytes = repository
        .get_chat_payload_bytes(character_name, file_name)
        .await
        .expect("read post-rejected-save");
    let post_line_count = String::from_utf8(bytes).expect("utf8").lines().count();
    assert_eq!(
        post_line_count,
        num_messages + 1,
        "File should be untouched after rejected save"
    );

    println!(
        "PASS: Full→Windowed stale cursor rejected, all {} messages safe.",
        num_messages
    );

    let _ = fs::remove_dir_all(&root).await;
}

/// TEST 2: Windowed → Full mode switch (user's actual scenario).
/// Windowed-mode cursor (offset near file end) used with all 300 messages from full reload.
/// The full-mode save writes all messages but truncates from the windowed offset,
/// potentially duplicating or misplacing messages.
#[tokio::test]
async fn repro_windowed_to_full_mode_switch_data_loss() {
    let (repository, root) = setup_repository().await;

    let character_name = "test-char-w2f";
    let file_name = "session";
    let num_messages: usize = 300;
    let window_size: usize = 50;

    let payload = generate_large_payload(num_messages, "w2f-test");
    save_chat_payload_from_values(
        &repository,
        &root,
        character_name,
        file_name,
        &payload,
        false,
    )
    .await
    .expect("save 300-message payload");

    // Step 1: User is in windowed mode
    let windowed_tail = repository
        .get_chat_payload_tail_lines(character_name, file_name, window_size)
        .await
        .expect("get windowed tail");
    let cursor_windowed = windowed_tail.cursor;
    assert_eq!(windowed_tail.lines.len(), window_size);
    assert!(windowed_tail.has_more_before);

    // Step 2: User switches to full mode — frontend reloads all messages
    let full_tail = repository
        .get_chat_payload_tail_lines(character_name, file_name, 10000)
        .await
        .expect("get full tail");
    assert_eq!(full_tail.lines.len(), num_messages);

    println!("=== Windowed → Full ===");
    println!(
        "Windowed cursor offset: {} (near end of file)",
        cursor_windowed.offset
    );
    println!(
        "Full cursor offset:     {} (near header end)",
        full_tail.cursor.offset
    );
    println!("File size: {}", cursor_windowed.size);

    // THE BUG: save with windowed cursor + ALL 300 messages from full reload.
    // set_len(cursor_windowed.offset) keeps prefix up to windowed offset,
    // then writes ALL 300 messages after it.
    // Result: messages 1-250 exist TWICE (once in preserved prefix, once in written payload)
    // or the file becomes corrupted/oversized.
    let save_result = repository
        .save_chat_payload_windowed(
            character_name,
            file_name,
            cursor_windowed, // STALE: offset near file end
            full_tail.header,
            full_tail.lines, // ALL 300 messages
            num_messages,
            false,
        )
        .await;

    assert!(
        save_result.is_err(),
        "Save with mismatched cursor/lines should be rejected"
    );
    let err_msg = format!("{}", save_result.unwrap_err());
    assert!(
        err_msg.contains("Window baseline mismatch"),
        "Error should mention the window baseline, got: {}",
        err_msg
    );

    let bytes = repository
        .get_chat_payload_bytes(character_name, file_name)
        .await
        .expect("read post-rejected-save");
    let post_line_count = String::from_utf8(bytes).expect("utf8").lines().count();
    assert_eq!(
        post_line_count,
        num_messages + 1,
        "File should be untouched after rejected save"
    );

    println!(
        "PASS: Windowed->Full stale cursor rejected, all {} messages safe.",
        num_messages
    );

    let _ = fs::remove_dir_all(&root).await;
}
