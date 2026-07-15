use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use chrono::DateTime;
use rand::random;
use serde_json::{Value, json};
use tokio::fs;

use crate::chat_directory_identity::new_shared_chat_alias_store_for_user_dir;
use tt_domain::errors::DomainError;
use tt_domain::models::filename::sanitize_filename;
use tt_domain::models::settings::ChatBackupSettings;
use tt_ports::repositories::chat_repository::{
    ChatMessageRole, ChatMessageSearchFilters, ChatMessageSearchQuery, ChatPayloadCursor,
    ChatPayloadPatchOp, ChatPayloadWindowPatchRequest, ChatRepository, PinnedCharacterChat,
    PinnedGroupChat,
};
use tt_ports::repositories::group_chat_repository::GroupChatRepository;
use tt_ports::settings::ChatBackupRuntime;

use super::FileChatRepository;
use super::backup_inventory::BackupInventoryState;

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

fn backup_policy(
    max_files_per_prefix: i64,
    max_total_files: i64,
    max_total_bytes: i64,
) -> ChatBackupSettings {
    ChatBackupSettings {
        automatic_enabled: true,
        max_files_per_prefix,
        max_total_files,
        max_total_bytes,
    }
}

async fn apply_and_reconcile_backups(repository: &FileChatRepository, policy: ChatBackupSettings) {
    repository
        .apply_chat_backup_settings(policy)
        .await
        .expect("apply backup policy");
    repository
        .reconcile_chat_backups()
        .await
        .expect("reconcile backup inventory");
}

async fn backup_file_names(root: &Path) -> Vec<String> {
    let mut names = Vec::new();
    let mut entries = fs::read_dir(root.join("backups"))
        .await
        .expect("read backups directory");
    while let Some(entry) = entries.next_entry().await.expect("read backup entry") {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with("chat_") && name.ends_with(".jsonl") {
            names.push(name);
        }
    }
    names.sort();
    names
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

fn timestamp_millis(value: &str) -> i64 {
    DateTime::parse_from_rfc3339(value)
        .expect("parse timestamp")
        .timestamp_millis()
}

async fn modified_millis(path: &Path) -> i64 {
    fs::metadata(path)
        .await
        .expect("read file metadata")
        .modified()
        .expect("read modified time")
        .duration_since(std::time::UNIX_EPOCH)
        .expect("modified time after UNIX_EPOCH")
        .as_millis() as i64
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
    assert_eq!(unicode, "角色_a");
}

#[test]
fn backup_file_name_preserves_unicode_within_component_limit() {
    let name = FileChatRepository::backup_file_name(&"角色".repeat(200));

    assert!(name.starts_with("chat_角色"));
    assert!(name.len() <= 255);
    assert!(name.is_char_boundary(name.len()));
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

#[test]
fn normalize_backup_file_name_rejects_non_finalized_suffix() {
    let result =
        FileChatRepository::normalize_backup_file_name("chat_alice_20260101-000000.jsonl.partial");
    assert!(matches!(result, Err(DomainError::InvalidData(_))));
}

#[tokio::test]
async fn explicit_backups_keep_readable_format_without_same_second_overwrite() {
    let (repository, root) = setup_repository().await;
    apply_and_reconcile_backups(&repository, backup_policy(-1, -1, -1)).await;
    let source = root.join("source.jsonl");
    fs::write(&source, payload_to_jsonl(&payload_with_integrity("unique")))
        .await
        .expect("write source");

    repository
        .backup_chat_file_explicit(&source, "角色-A")
        .await
        .expect("first backup");
    repository
        .backup_chat_file_explicit(&source, "角色-A")
        .await
        .expect("second backup");

    let names = backup_file_names(&root).await;
    assert_eq!(names.len(), 2);
    assert_ne!(names[0], names[1]);
    assert!(names.iter().all(|name| name.starts_with("chat_角色_a_")));
}

#[tokio::test]
async fn inventory_enforces_prefix_and_global_file_limits() {
    let (repository, root) = setup_repository().await;
    apply_and_reconcile_backups(&repository, backup_policy(2, 3, -1)).await;
    let source = root.join("source.jsonl");
    fs::write(&source, payload_to_jsonl(&payload_with_integrity("quota")))
        .await
        .expect("write source");

    for _ in 0..3 {
        repository
            .backup_chat_file_explicit(&source, "Alice")
            .await
            .expect("backup Alice");
    }
    for _ in 0..2 {
        repository
            .backup_chat_file_explicit(&source, "Bob")
            .await
            .expect("backup Bob");
    }

    let names = backup_file_names(&root).await;
    assert_eq!(names.len(), 3);
    assert!(
        names
            .iter()
            .filter(|name| name.starts_with("chat_alice_"))
            .count()
            <= 2
    );
    assert!(
        names
            .iter()
            .filter(|name| name.starts_with("chat_bob_"))
            .count()
            <= 2
    );
}

#[tokio::test]
async fn reconcile_prunes_legacy_overage_and_zero_limit_clears_history() {
    let (repository, root) = setup_repository().await;
    for index in 0..4 {
        fs::write(
            root.join("backups")
                .join(format!("chat_alice_20260101-00000{index}.jsonl")),
            payload_to_jsonl(&payload_with_integrity("legacy")),
        )
        .await
        .expect("write legacy backup");
    }

    apply_and_reconcile_backups(&repository, backup_policy(2, 3, -1)).await;
    assert_eq!(backup_file_names(&root).await.len(), 2);

    apply_and_reconcile_backups(&repository, backup_policy(0, -1, -1)).await;
    assert!(backup_file_names(&root).await.is_empty());
}

#[tokio::test]
async fn reconcile_removes_only_reserved_stale_staging_names() {
    let (repository, root) = setup_repository().await;
    let staging = root.join("backups").join(format!(
        ".tmp-chat-backup-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let unrelated = root
        .join("backups")
        .join(".tmp-chat-backup-not-a-valid-uuid");
    fs::write(&staging, b"partial")
        .await
        .expect("write reserved staging");
    fs::write(&unrelated, b"keep")
        .await
        .expect("write unrelated temp");

    apply_and_reconcile_backups(&repository, backup_policy(-1, -1, -1)).await;

    assert!(!staging.exists());
    assert!(unrelated.exists());
}

#[tokio::test]
async fn automatic_quota_rejection_does_not_fail_current_save() {
    let (repository, root) = setup_repository().await;
    apply_and_reconcile_backups(&repository, backup_policy(-1, -1, 1)).await;
    let source = root.join("source.jsonl");
    let payload = payload_to_jsonl(&payload_with_integrity("too-large"));
    fs::write(&source, &payload).await.expect("write source");

    repository
        .save_chat_payload_from_path("Alice", "session", &source, false)
        .await
        .expect("current save must succeed");
    assert!(backup_file_names(&root).await.is_empty());

    let error = repository
        .backup_chat("Alice", "session")
        .await
        .expect_err("explicit backup must expose quota rejection");
    assert!(matches!(error, DomainError::Conflict(_)));
    assert_eq!(
        repository
            .get_chat_payload_bytes("Alice", "session")
            .await
            .expect("read committed current payload"),
        payload.as_bytes()
    );
}

#[tokio::test]
async fn automatic_toggle_does_not_disable_explicit_backups() {
    let (repository, root) = setup_repository().await;
    let mut policy = backup_policy(-1, -1, -1);
    policy.automatic_enabled = false;
    apply_and_reconcile_backups(&repository, policy).await;
    let source = root.join("source.jsonl");
    fs::write(&source, payload_to_jsonl(&payload_with_integrity("manual")))
        .await
        .expect("write source");

    repository
        .save_chat_payload_from_path("Alice", "session", &source, false)
        .await
        .expect("save current chat");
    assert!(backup_file_names(&root).await.is_empty());

    repository
        .backup_chat("Alice", "session")
        .await
        .expect("explicit backup remains enabled");
    assert_eq!(backup_file_names(&root).await.len(), 1);
}

#[tokio::test]
async fn manual_delete_updates_inventory_backed_list() {
    let (repository, root) = setup_repository().await;
    apply_and_reconcile_backups(&repository, backup_policy(-1, -1, -1)).await;
    let source = root.join("source.jsonl");
    fs::write(&source, payload_to_jsonl(&payload_with_integrity("delete")))
        .await
        .expect("write source");
    repository
        .backup_chat_file_explicit(&source, "Alice")
        .await
        .expect("create backup");
    let file_name = repository
        .list_chat_backup_files()
        .await
        .expect("list inventory")
        .pop()
        .expect("backup descriptor")
        .file_name;

    repository
        .delete_chat_backup(&file_name)
        .await
        .expect("delete backup");

    assert!(
        repository
            .list_chat_backup_files()
            .await
            .expect("list inventory after delete")
            .is_empty()
    );
}

#[tokio::test]
async fn backup_list_uses_inventory_until_an_explicit_reconcile() {
    let (repository, root) = setup_repository().await;
    let policy = backup_policy(-1, -1, -1);
    apply_and_reconcile_backups(&repository, policy).await;
    fs::write(
        root.join("backups/chat_external_20260101-000000.jsonl"),
        payload_to_jsonl(&payload_with_integrity("external")),
    )
    .await
    .expect("write external backup");

    assert!(
        repository
            .list_chat_backup_files()
            .await
            .expect("list cached inventory")
            .is_empty()
    );

    repository
        .reconcile_chat_backups()
        .await
        .expect("reconcile external change");
    assert_eq!(
        repository
            .list_chat_backup_files()
            .await
            .expect("list rebuilt inventory")
            .len(),
        1
    );
}

#[tokio::test]
async fn explicit_access_retries_a_failed_inventory_build() {
    let (repository, root) = setup_repository().await;
    fs::write(
        root.join("backups/chat_external_20260101-000000.jsonl"),
        payload_to_jsonl(&payload_with_integrity("external")),
    )
    .await
    .expect("write external backup");
    repository.backup_history.lock().await.inventory =
        BackupInventoryState::Failed("transient scan failure".into());

    assert_eq!(
        repository
            .list_chat_backup_files()
            .await
            .expect("retry inventory build")
            .len(),
        1
    );
}

#[tokio::test]
async fn quota_cleanup_tolerates_an_externally_removed_backup() {
    let (repository, root) = setup_repository().await;
    apply_and_reconcile_backups(&repository, backup_policy(1, -1, -1)).await;
    let source = root.join("source.jsonl");
    fs::write(
        &source,
        payload_to_jsonl(&payload_with_integrity("missing")),
    )
    .await
    .expect("write source");
    repository
        .backup_chat_file_explicit(&source, "Alice")
        .await
        .expect("create first backup");
    let old_name = backup_file_names(&root).await.pop().expect("first backup");
    fs::remove_file(root.join("backups").join(old_name))
        .await
        .expect("remove backup outside repository");

    repository
        .backup_chat_file_explicit(&source, "Alice")
        .await
        .expect("replace stale inventory entry");

    assert_eq!(backup_file_names(&root).await.len(), 1);
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
    let saved = crate::jsonl_utils::parse_jsonl_bytes(&bytes).expect("parse group payload");
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
async fn import_chat_payload_preserves_unchanged_jsonl_bytes() {
    let (repository, root) = setup_repository().await;

    let import_path = root.join("raw-import.jsonl");
    let import_content = concat!(
        "{ \"chat_metadata\" : { \"integrity\" : \"raw-import\" } }\r\n",
        "{ \"name\" : \"Alice\", \"is_user\" : false, \"mes\" : \"kept\", \"extra\" : { \"note\" : true } }\n",
        "not-json but SillyTavern keeps it\n"
    );
    fs::write(&import_path, import_content.as_bytes())
        .await
        .expect("write raw import file");

    let files = repository
        .import_chat_payload("alice", "Alice", "User", &import_path, "jsonl")
        .await
        .expect("import raw JSONL");

    let saved = fs::read(root.join("chats").join("alice").join(&files[0]))
        .await
        .expect("read imported raw JSONL");
    assert_eq!(saved, import_content.as_bytes());

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn import_chat_payload_flattens_chub_jsonl_without_normalizing_messages() {
    let (repository, root) = setup_repository().await;

    let import_path = root.join("chub-import.jsonl");
    let import_content = [
        r#"{"chat_metadata":{"integrity":"chub-import"}}"#,
        r#"{"is_user":false,"mes":{"message":"hello"},"swipes":[{"message":"alt"},{"message":""},{"other":"kept"}]}"#,
    ]
    .join("\n");
    fs::write(&import_path, import_content)
        .await
        .expect("write Chub import file");

    let files = repository
        .import_chat_payload("alice", "Alice", "User", &import_path, "jsonl")
        .await
        .expect("import Chub JSONL");

    let saved_bytes = fs::read(root.join("chats").join("alice").join(&files[0]))
        .await
        .expect("read imported Chub JSONL");
    let saved = crate::jsonl_utils::parse_jsonl_bytes(&saved_bytes).expect("parse Chub import");

    assert_eq!(saved.len(), 2);
    assert_eq!(saved[1].get("mes"), Some(&json!("hello")));
    assert_eq!(saved[1].pointer("/swipes/0"), Some(&json!("alt")));
    assert_eq!(saved[1].pointer("/swipes/1"), Some(&json!({"message": ""})));
    assert!(saved[1].get("name").is_none());
    assert!(saved[1].get("extra").is_none());

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
async fn calculate_character_chat_stats_uses_last_message_send_date() {
    let (repository, root) = setup_repository().await;
    let older = payload_with_message("stats-old", "2026-01-01T00:00:00.000Z", "older", "alice");
    let newer = payload_with_message("stats-new", "2026-01-03T00:00:00.000Z", "newer", "alice");

    save_chat_payload_from_values(&repository, &root, "alice", "older", &older, false)
        .await
        .expect("save older payload");
    save_chat_payload_from_values(&repository, &root, "alice", "newer", &newer, false)
        .await
        .expect("save newer payload");

    let (chat_size, date_last_chat) = repository
        .calculate_character_chat_stats("alice")
        .await
        .expect("calculate chat stats");

    assert!(chat_size > 0);
    assert_eq!(date_last_chat, timestamp_millis("2026-01-03T00:00:00.000Z"));

    let index_path = root
        .join("user")
        .join("cache")
        .join("chat_summary_index_v1.json");
    let parsed: Value = serde_json::from_str(
        &fs::read_to_string(&index_path)
            .await
            .expect("read summary index"),
    )
    .expect("parse summary index");
    assert_eq!(
        parsed
            .get("entries")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(0)
    );
    assert_eq!(
        parsed
            .get("stats_entries")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(2)
    );

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn calculate_character_chat_stats_counts_jsonl_files_only() {
    let (repository, root) = setup_repository().await;
    let payload = payload_with_message("stats-size", "2026-01-01T00:00:00.000Z", "hello", "alice");

    save_chat_payload_from_values(&repository, &root, "alice", "session", &payload, false)
        .await
        .expect("save payload");

    let chat_dir = root.join("chats").join("alice");
    let jsonl_size = fs::metadata(chat_dir.join("session.jsonl"))
        .await
        .expect("read jsonl metadata")
        .len();
    fs::write(chat_dir.join("sidecar.txt"), vec![b'x'; 4096])
        .await
        .expect("write sidecar");

    let (chat_size, _) = repository
        .calculate_character_chat_stats("alice")
        .await
        .expect("calculate chat stats");

    assert_eq!(chat_size, jsonl_size);

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
    let import_content = payload_to_jsonl(&[
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
async fn list_recent_chat_summaries_ranks_by_last_message_date_not_file_mtime() {
    let (repository, root) = setup_repository().await;
    let characters_dir = root.join("characters");
    fs::create_dir_all(&characters_dir)
        .await
        .expect("create characters directory");
    fs::write(characters_dir.join("alice.png"), b"")
        .await
        .expect("create alice card");

    let newer_payload = payload_with_message(
        "recent-newer-date",
        "2026-01-03T00:00:00.000Z",
        "newer date",
        "Alice",
    );
    save_chat_payload_from_values(
        &repository,
        &root,
        "alice",
        "session-newer-date",
        &newer_payload,
        false,
    )
    .await
    .expect("save newer-date chat");

    let newer_path = root
        .join("chats")
        .join("alice")
        .join("session-newer-date.jsonl");
    let older_path = root
        .join("chats")
        .join("alice")
        .join("session-newer-mtime.jsonl");
    let older_payload = payload_with_message(
        "recent-older-date",
        "2026-01-01T00:00:00.000Z",
        "older date",
        "Alice",
    );
    for _ in 0..50 {
        save_chat_payload_from_values(
            &repository,
            &root,
            "alice",
            "session-newer-mtime",
            &older_payload,
            false,
        )
        .await
        .expect("save newer-mtime chat");

        if modified_millis(&older_path).await > modified_millis(&newer_path).await {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        modified_millis(&older_path).await > modified_millis(&newer_path).await,
        "test precondition: older send_date file must have newer mtime"
    );

    let results = repository
        .list_recent_chat_summaries(None, false, 1, &[])
        .await
        .expect("list recent summaries");

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].file_name, "session-newer-date.jsonl");

    let index_path = root
        .join("user")
        .join("cache")
        .join("chat_summary_index_v1.json");
    let parsed: Value = serde_json::from_str(
        &fs::read_to_string(&index_path)
            .await
            .expect("read summary index"),
    )
    .expect("parse summary index");
    assert_eq!(
        parsed
            .get("entries")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(1)
    );
    assert_eq!(
        parsed
            .get("stats_entries")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(1)
    );

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn list_recent_chat_summaries_ignores_root_chats_without_character_identity() {
    let (repository, root) = setup_repository().await;
    let characters_dir = root.join("characters");
    fs::create_dir_all(&characters_dir)
        .await
        .expect("create characters directory");
    fs::write(characters_dir.join("alice.png"), b"")
        .await
        .expect("create alice card");

    let character_payload = payload_with_message(
        "recent-character",
        "2026-01-01T00:00:00.000Z",
        "character",
        "Alice",
    );
    save_chat_payload_from_values(
        &repository,
        &root,
        "alice",
        "character-session",
        &character_payload,
        false,
    )
    .await
    .expect("save character chat");

    let root_payload =
        payload_with_message("recent-root", "2026-01-03T00:00:00.000Z", "root", "Root");
    fs::write(
        root.join("chats").join("root-session.jsonl"),
        payload_to_jsonl(&root_payload),
    )
    .await
    .expect("write root chat");

    let results = repository
        .list_recent_chat_summaries(None, false, 1, &[])
        .await
        .expect("list recent summaries");

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].file_name, "character-session.jsonl");

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn list_recent_chat_summaries_zero_limit_skips_unpinned_stats() {
    let (repository, root) = setup_repository().await;
    let characters_dir = root.join("characters");
    fs::create_dir_all(&characters_dir)
        .await
        .expect("create characters directory");
    fs::write(characters_dir.join("alice.png"), b"")
        .await
        .expect("create alice card");

    let payload = payload_with_message("recent-zero", "2026-01-01T00:00:00.000Z", "zero", "Alice");
    save_chat_payload_from_values(&repository, &root, "alice", "session", &payload, false)
        .await
        .expect("save chat");

    let results = repository
        .list_recent_chat_summaries(None, false, 0, &[])
        .await
        .expect("list recent summaries");

    assert!(results.is_empty());
    assert!(
        !root
            .join("user")
            .join("cache")
            .join("chat_summary_index_v1.json")
            .exists()
    );

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
async fn list_recent_group_chat_summaries_ranks_by_last_message_date_not_file_mtime() {
    let (repository, root) = setup_repository().await;

    let newer_payload = payload_with_message(
        "group-recent-newer-date",
        "2026-01-03T00:00:00.000Z",
        "newer date",
        "Group",
    );
    save_group_chat_payload_from_values(
        &repository,
        &root,
        "group-newer-date",
        &newer_payload,
        false,
    )
    .await
    .expect("save newer-date group chat");

    let newer_path = root.join("group chats").join("group-newer-date.jsonl");
    let older_path = root.join("group chats").join("group-newer-mtime.jsonl");
    let older_payload = payload_with_message(
        "group-recent-older-date",
        "2026-01-01T00:00:00.000Z",
        "older date",
        "Group",
    );
    for _ in 0..50 {
        save_group_chat_payload_from_values(
            &repository,
            &root,
            "group-newer-mtime",
            &older_payload,
            false,
        )
        .await
        .expect("save newer-mtime group chat");

        if modified_millis(&older_path).await > modified_millis(&newer_path).await {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        modified_millis(&older_path).await > modified_millis(&newer_path).await,
        "test precondition: older send_date group file must have newer mtime"
    );

    let results = repository
        .list_recent_group_chat_summaries(None, false, 1, &[])
        .await
        .expect("list recent group summaries");

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].file_name, "group-newer-date.jsonl");

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
    root: &Path,
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
            ChatPayloadWindowPatchRequest {
                cursor: tail.cursor,
                header: tail.header.clone(),
                op: ChatPayloadPatchOp::Append {
                    lines: vec![new_line.clone()],
                },
                expected_window_line_count: 2,
                force: false,
            },
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
            ChatPayloadWindowPatchRequest {
                cursor,
                header: tail.header,
                op: ChatPayloadPatchOp::RewriteFromIndex {
                    start_index: 2,
                    lines: vec![updated_line],
                },
                expected_window_line_count: 3,
                force: false,
            },
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
            ChatPayloadWindowPatchRequest {
                cursor,
                header: serde_json::to_string(&values[0]).expect("serialize header"),
                op: ChatPayloadPatchOp::RewriteFromIndex {
                    start_index: 1,
                    lines: Vec::new(),
                },
                expected_window_line_count: 3,
                force: false,
            },
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
async fn hide_chat_payload_before_cursor_rewrites_only_lines_before_window() {
    let (repository, root) = setup_repository().await;

    let character_name = "alice";
    let file_name = "session";

    let mut payload = vec![payload_with_integrity("hide-a")[0].clone()];
    for index in 0..6 {
        let is_user = index % 2 == 0;
        payload.push(json!({
            "name": if is_user { "User" } else { "Alice" },
            "is_user": is_user,
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

    let tail = repository
        .get_chat_payload_tail_lines(character_name, file_name, 2)
        .await
        .expect("get tail");
    assert_eq!(tail.lines.len(), 2);
    assert!(tail.has_more_before);
    let window_lines = tail.lines.clone();
    let stale_cursor = tail.cursor;

    let cursor = repository
        .hide_chat_payload_before_cursor(character_name, file_name, tail.cursor, true, None, 2)
        .await
        .expect("hide before cursor");

    let bytes = repository
        .get_chat_payload_bytes(character_name, file_name)
        .await
        .expect("read payload bytes");
    let text = String::from_utf8(bytes).expect("payload should be utf8");
    let lines = text.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 7);
    for line in &lines[1..5] {
        let value = serde_json::from_str::<Value>(line).expect("parse json line");
        assert_eq!(value.get("is_system").and_then(Value::as_bool), Some(true));
    }
    assert_eq!(lines[5], window_lines[0]);
    assert_eq!(lines[6], window_lines[1]);

    let chunk = repository
        .get_chat_payload_before_lines(character_name, file_name, cursor, 100)
        .await
        .expect("read lines before returned cursor");
    assert_eq!(chunk.lines.len(), 4);
    assert!(!chunk.has_more_before);

    let stale = repository
        .hide_chat_payload_before_cursor(character_name, file_name, stale_cursor, true, None, 2)
        .await;
    assert!(stale.is_err(), "stale cursor should be rejected");

    let baseline_mismatch = repository
        .hide_chat_payload_before_cursor(character_name, file_name, cursor, true, None, 5)
        .await;
    assert!(
        baseline_mismatch.is_err(),
        "window baseline mismatch should be rejected"
    );

    let cursor = repository
        .hide_chat_payload_before_cursor(
            character_name,
            file_name,
            cursor,
            false,
            Some("User".to_string()),
            2,
        )
        .await
        .expect("unhide filtered by name");

    let bytes = repository
        .get_chat_payload_bytes(character_name, file_name)
        .await
        .expect("read payload bytes after filtered unhide");
    let text_after_filter = String::from_utf8(bytes).expect("payload should be utf8");
    let lines = text_after_filter.lines().collect::<Vec<_>>();
    for (message_index, line) in lines[1..5].iter().enumerate() {
        let value = serde_json::from_str::<Value>(line).expect("parse json line");
        let expected_hidden = message_index % 2 != 0;
        assert_eq!(
            value.get("is_system").and_then(Value::as_bool),
            Some(expected_hidden),
            "message {} hidden state",
            message_index
        );
    }

    repository
        .hide_chat_payload_before_cursor(
            character_name,
            file_name,
            cursor,
            false,
            Some("User".to_string()),
            2,
        )
        .await
        .expect("no-op unhide");

    let bytes = repository
        .get_chat_payload_bytes(character_name, file_name)
        .await
        .expect("read payload bytes after no-op");
    let text_after_noop = String::from_utf8(bytes).expect("payload should be utf8");
    assert_eq!(text_after_noop, text_after_filter);

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn hide_chat_payload_before_cursor_rejects_non_object_lines() {
    let (repository, root) = setup_repository().await;

    let character_name = "alice";
    let file_name = "session";

    let payload = vec![
        payload_with_integrity("hide-b")[0].clone(),
        json!(123),
        json!({
            "name": "User",
            "is_user": true,
            "send_date": "2026-01-01T00:00:00.000Z",
            "mes": "before window",
            "extra": {},
        }),
        json!({
            "name": "Alice",
            "is_user": false,
            "send_date": "2026-01-01T00:00:01.000Z",
            "mes": "in window",
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

    let original_bytes = repository
        .get_chat_payload_bytes(character_name, file_name)
        .await
        .expect("read original payload bytes");

    let tail = repository
        .get_chat_payload_tail_lines(character_name, file_name, 1)
        .await
        .expect("get tail");
    assert_eq!(tail.lines.len(), 1);

    let error = repository
        .hide_chat_payload_before_cursor(character_name, file_name, tail.cursor, true, None, 1)
        .await
        .expect_err("hide should reject a non-object payload line");
    assert!(matches!(error, DomainError::InvalidData(_)));

    let bytes_after = repository
        .get_chat_payload_bytes(character_name, file_name)
        .await
        .expect("read payload bytes after rejected hide");
    assert_eq!(
        bytes_after, original_bytes,
        "rejected hide must leave the file untouched"
    );

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
            ChatPayloadWindowPatchRequest {
                cursor: tail.cursor,
                header: missing_integrity_header,
                op: ChatPayloadPatchOp::Append { lines: Vec::new() },
                expected_window_line_count: tail.lines.len(),
                force: false,
            },
        )
        .await
        .expect_err("patch should fail when incoming integrity is missing");
    assert!(matches!(error, DomainError::InvalidData(message) if message == "integrity"));

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn patch_chat_payload_windowed_updates_header_without_rewriting_prefix() {
    let (repository, root) = setup_repository().await;

    let character_name = "alice";
    let file_name = "session";
    let payload = vec![
        payload_with_integrity("header-a")[0].clone(),
        json!({ "mes": "before window 1" }),
        json!({ "mes": "before window 2" }),
        json!({ "mes": "in window" }),
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

    let original_bytes = repository
        .get_chat_payload_bytes(character_name, file_name)
        .await
        .expect("read original payload bytes");
    let old_header_end = original_bytes
        .iter()
        .position(|byte| *byte == b'\n')
        .expect("header newline")
        + 1;
    let tail = repository
        .get_chat_payload_tail_lines(character_name, file_name, 1)
        .await
        .expect("get windowed tail");
    let old_cursor = tail.cursor;
    let prefix = original_bytes[old_header_end..old_cursor.offset as usize].to_vec();

    let mut updated_header = payload[0].clone();
    updated_header["chat_metadata"]["integrity"] = json!("header-b-with-a-new-length");
    let updated_header = serde_json::to_string(&updated_header).expect("serialize updated header");

    let new_cursor = repository
        .patch_chat_payload_windowed(
            character_name,
            file_name,
            ChatPayloadWindowPatchRequest {
                cursor: old_cursor,
                header: updated_header.clone(),
                op: ChatPayloadPatchOp::Append { lines: Vec::new() },
                expected_window_line_count: tail.lines.len(),
                force: true,
            },
        )
        .await
        .expect("force patch with changed integrity");

    let updated_bytes = repository
        .get_chat_payload_bytes(character_name, file_name)
        .await
        .expect("read updated payload bytes");
    let new_header_end = updated_bytes
        .iter()
        .position(|byte| *byte == b'\n')
        .expect("updated header newline")
        + 1;
    assert_eq!(
        new_cursor.offset,
        new_header_end as u64 + (old_cursor.offset - old_header_end as u64)
    );
    assert_eq!(
        &updated_bytes[new_header_end..new_cursor.offset as usize],
        prefix.as_slice()
    );

    let error = repository
        .patch_chat_payload_windowed(
            character_name,
            file_name,
            ChatPayloadWindowPatchRequest {
                cursor: old_cursor,
                header: updated_header,
                op: ChatPayloadPatchOp::Append { lines: Vec::new() },
                expected_window_line_count: tail.lines.len(),
                force: true,
            },
        )
        .await
        .expect_err("force must not bypass a stale cursor signature");
    assert!(format!("{}", error).contains("Cursor signature mismatch"));

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn patch_chat_payload_windowed_rejects_missing_payload() {
    let (repository, root) = setup_repository().await;

    let character_name = "alice";
    let file_name = "missing";
    let path = repository
        .resolve_character_chat_path(character_name, file_name)
        .await
        .expect("resolve missing payload path");
    let header =
        serde_json::to_string(&payload_with_integrity("missing-a")[0]).expect("serialize header");

    let error = repository
        .patch_chat_payload_windowed(
            character_name,
            file_name,
            ChatPayloadWindowPatchRequest {
                cursor: ChatPayloadCursor {
                    offset: 0,
                    size: 0,
                    modified_millis: 0,
                },
                header,
                op: ChatPayloadPatchOp::Append {
                    lines: vec![json!({ "mes": "partial" }).to_string()],
                },
                expected_window_line_count: 0,
                force: false,
            },
        )
        .await
        .expect_err("patch must not create a missing payload");

    assert!(matches!(error, DomainError::NotFound(_)));
    assert!(!path.exists(), "missing payload must remain absent");

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
            ChatPayloadWindowPatchRequest {
                cursor: full_tail.cursor,
                header: windowed_tail.header.clone(),
                op: ChatPayloadPatchOp::Append {
                    lines: vec![new_line.clone()],
                },
                expected_window_line_count: windowed_tail.lines.len(),
                force: false,
            },
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
            ChatPayloadWindowPatchRequest {
                cursor: windowed_tail.cursor,
                header: windowed_tail.header.clone(),
                op: ChatPayloadPatchOp::RewriteFromIndex {
                    start_index: 0,
                    lines: vec![new_line.clone()],
                },
                expected_window_line_count: 4,
                force: true,
            },
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
            ChatPayloadWindowPatchRequest {
                cursor: windowed_tail.cursor,
                header: windowed_tail.header,
                op: ChatPayloadPatchOp::Append {
                    lines: vec![new_line],
                },
                expected_window_line_count: windowed_tail.lines.len(),
                force: false,
            },
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
    root: &Path,
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
