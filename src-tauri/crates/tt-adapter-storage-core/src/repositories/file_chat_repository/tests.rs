use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use chrono::DateTime;
use rand::random;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::fs;
use tokio::sync::Barrier;

use crate::chat_directory_identity::new_shared_chat_alias_store_for_user_dir;
use tt_domain::errors::DomainError;
use tt_domain::models::filename::sanitize_filename;
use tt_domain::models::settings::ChatBackupSettings;
use tt_ports::repositories::chat_payload_commit_repository::{
    ChatPayloadCommitRepository, ChatPayloadTarget, CommittedChatPayload,
};
use tt_ports::repositories::chat_repository::{
    ChatMessageRole, ChatMessageSearchFilters, ChatMessageSearchQuery, ChatRepository,
    PinnedCharacterChat, PinnedGroupChat,
};
use tt_ports::repositories::group_chat_repository::GroupChatRepository;
use tt_ports::settings::ChatBackupRuntime;

use super::FileChatRepository;
use super::backup_codec::set_backup_modified;
use super::backup_inventory::BackupInventoryState;
use super::chat_payload_commit::MAX_ACTIVE_CHAT_COMMIT_SESSIONS;

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

async fn commit_payload_bytes(
    repository: &FileChatRepository,
    target: ChatPayloadTarget,
    bytes: &[u8],
    force: bool,
) -> Result<CommittedChatPayload, DomainError> {
    let session = repository.begin(target, force).await?;
    let frame_bytes = session.max_frame_bytes as usize;
    let mut offset = 0;
    for frame in bytes.chunks(frame_bytes) {
        offset = repository
            .append(&session.session_id, offset, frame)
            .await?;
    }
    repository
        .finish(&session.session_id, bytes.len() as u64)
        .await
}

async fn commit_character_payload_file(
    repository: &FileChatRepository,
    character_id: &str,
    file_name: &str,
    source_path: &Path,
    force: bool,
) -> Result<CommittedChatPayload, DomainError> {
    let bytes = fs::read(source_path).await.map_err(|error| {
        DomainError::InternalError(format!("Failed to read test payload fixture: {error}"))
    })?;
    commit_payload_bytes(
        repository,
        ChatPayloadTarget::Character {
            character_id: character_id.to_string(),
            file_name: file_name.to_string(),
        },
        &bytes,
        force,
    )
    .await
}

async fn commit_group_payload_file(
    repository: &FileChatRepository,
    chat_id: &str,
    source_path: &Path,
    force: bool,
) -> Result<CommittedChatPayload, DomainError> {
    let bytes = fs::read(source_path).await.map_err(|error| {
        DomainError::InternalError(format!("Failed to read test payload fixture: {error}"))
    })?;
    commit_payload_bytes(
        repository,
        ChatPayloadTarget::Group {
            chat_id: chat_id.to_string(),
        },
        &bytes,
        force,
    )
    .await
}

fn character_target(character_id: &str, file_name: &str) -> ChatPayloadTarget {
    ChatPayloadTarget::Character {
        character_id: character_id.to_string(),
        file_name: file_name.to_string(),
    }
}

#[tokio::test]
async fn chat_commit_protocol_rejects_invalid_frames_and_abort_is_idempotent() {
    let (repository, root) = setup_repository().await;
    let session = repository
        .begin(character_target("alice", "session"), false)
        .await
        .expect("begin chat commit");

    assert!(matches!(
        repository.append(&session.session_id, 0, &[]).await,
        Err(DomainError::InvalidData(_))
    ));
    assert!(matches!(
        repository.append(&session.session_id, 1, b"{}").await,
        Err(DomainError::InvalidData(_))
    ));
    let oversized = vec![0; session.max_frame_bytes as usize + 1];
    assert!(matches!(
        repository.append(&session.session_id, 0, &oversized).await,
        Err(DomainError::InvalidData(_))
    ));

    assert_eq!(
        repository
            .append(&session.session_id, 0, b"{}")
            .await
            .expect("append valid frame"),
        2
    );
    repository
        .abort(&session.session_id)
        .await
        .expect("abort session");
    repository
        .abort(&session.session_id)
        .await
        .expect("repeat abort");
    repository
        .abort(&uuid::Uuid::new_v4().to_string())
        .await
        .expect("abort unknown session");
    assert!(matches!(
        repository.append(&session.session_id, 2, b"{}").await,
        Err(DomainError::NotFound(_))
    ));

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn chat_commit_preserves_exact_bytes_across_multiple_frames() {
    let (repository, root) = setup_repository().await;
    let payload = r#"{"user_name":"User"}
{"name":"Alice","mes":"你好"}"#
        .as_bytes();
    let session = repository
        .begin(character_target("alice", "multi-frame"), false)
        .await
        .expect("begin multi-frame commit");
    let boundaries = [1, 13, payload.len()];
    let mut start = 0;
    let mut offset = 0;

    for end in boundaries {
        offset = repository
            .append(&session.session_id, offset, &payload[start..end])
            .await
            .expect("append frame");
        start = end;
    }
    repository
        .finish(&session.session_id, payload.len() as u64)
        .await
        .expect("finish multi-frame commit");

    assert_eq!(
        repository
            .get_chat_payload_bytes("alice", "multi-frame")
            .await
            .expect("read multi-frame payload"),
        payload
    );
    let path = repository
        .get_chat_payload_path("alice", "multi-frame")
        .await
        .expect("resolve multi-frame payload path");
    let signature = repository
        .current_content_signature_for_size(&path, payload.len() as u64)
        .await
        .expect("successful commit records its content signature");
    let expected_digest: [u8; 32] = Sha256::digest(payload).into();
    assert_eq!(signature.sha256, expected_digest);

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn chat_commit_does_not_hash_when_automatic_history_is_disabled() {
    let (repository, root) = setup_repository().await;
    let payload = payload_to_jsonl(&payload_with_integrity("hash-policy"));

    let mut automatic_disabled = backup_policy(-1, -1, -1);
    automatic_disabled.automatic_enabled = false;
    apply_and_reconcile_backups(&repository, automatic_disabled).await;
    commit_payload_bytes(
        &repository,
        character_target("Alice", "automatic-disabled"),
        payload.as_bytes(),
        false,
    )
    .await
    .expect("commit with automatic history disabled");
    let disabled_path = repository
        .get_chat_payload_path("Alice", "automatic-disabled")
        .await
        .expect("resolve disabled current");
    assert!(
        repository
            .current_content_signature_for_size(&disabled_path, payload.len() as u64)
            .await
            .is_none()
    );

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn global_invalidation_prevents_an_inflight_commit_from_restoring_old_provenance() {
    let (repository, root) = setup_repository().await;
    let payload = payload_to_jsonl(&payload_with_integrity("signature-epoch"));
    let session = repository
        .begin(character_target("Alice", "session"), false)
        .await
        .expect("begin commit");
    repository
        .append(&session.session_id, 0, payload.as_bytes())
        .await
        .expect("append payload");

    repository.invalidate_all_payload_signatures().await;
    repository
        .finish(&session.session_id, payload.len() as u64)
        .await
        .expect("publish current after invalidation");

    let path = repository
        .get_chat_payload_path("Alice", "session")
        .await
        .expect("resolve current");
    assert!(
        repository
            .current_content_signature_for_size(&path, payload.len() as u64)
            .await
            .is_none()
    );

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn chat_commit_size_mismatch_preserves_current_and_consumes_session() {
    let (repository, root) = setup_repository().await;
    let old_payload = payload_to_jsonl(&payload_with_message(
        "same",
        "2026-01-01T00:00:00.000Z",
        "old",
        "Assistant",
    ));
    commit_payload_bytes(
        &repository,
        character_target("alice", "session"),
        old_payload.as_bytes(),
        false,
    )
    .await
    .expect("commit old current");
    let new_payload = payload_to_jsonl(&payload_with_message(
        "same",
        "2026-01-01T00:00:01.000Z",
        "new",
        "Assistant",
    ));
    let session = repository
        .begin(character_target("alice", "session"), false)
        .await
        .expect("begin replacement");
    repository
        .append(&session.session_id, 0, new_payload.as_bytes())
        .await
        .expect("append replacement");

    assert!(matches!(
        repository
            .finish(&session.session_id, new_payload.len() as u64 + 1)
            .await,
        Err(DomainError::InvalidData(_))
    ));
    assert_eq!(
        repository
            .get_chat_payload_bytes("alice", "session")
            .await
            .expect("read preserved current"),
        old_payload.as_bytes()
    );
    assert!(matches!(
        repository
            .finish(&session.session_id, new_payload.len() as u64)
            .await,
        Err(DomainError::NotFound(_))
    ));
    let mut staging_entries = fs::read_dir(&repository.chat_commit_staging_dir)
        .await
        .expect("read staging directory");
    assert!(
        staging_entries
            .next_entry()
            .await
            .expect("read staging entry")
            .is_none()
    );

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn streaming_chat_commit_keeps_old_current_visible_until_finish() {
    let (repository, root) = setup_repository().await;
    let old_payload = payload_to_jsonl(&payload_with_message(
        "same",
        "2026-01-01T00:00:00.000Z",
        "old",
        "Assistant",
    ));
    commit_payload_bytes(
        &repository,
        character_target("alice", "session"),
        old_payload.as_bytes(),
        false,
    )
    .await
    .expect("commit old current");
    let new_payload = payload_to_jsonl(&payload_with_message(
        "same",
        "2026-01-01T00:00:01.000Z",
        "new",
        "Assistant",
    ));
    let session = repository
        .begin(character_target("alice", "session"), false)
        .await
        .expect("begin replacement");
    let split = new_payload.len() / 2;
    repository
        .append(&session.session_id, 0, &new_payload.as_bytes()[..split])
        .await
        .expect("append partial replacement");

    assert_eq!(
        repository
            .get_chat_payload_bytes("alice", "session")
            .await
            .expect("read current during streaming"),
        old_payload.as_bytes()
    );
    repository
        .abort(&session.session_id)
        .await
        .expect("abort partial replacement");

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn forced_chat_commit_still_rejects_an_invalid_header() {
    let (repository, root) = setup_repository().await;
    let old_payload = payload_to_jsonl(&payload_with_integrity("old"));
    commit_payload_bytes(
        &repository,
        character_target("alice", "session"),
        old_payload.as_bytes(),
        false,
    )
    .await
    .expect("commit old current");

    let session = repository
        .begin(character_target("alice", "session"), true)
        .await
        .expect("begin forced replacement");
    repository
        .append(&session.session_id, 0, b"[]")
        .await
        .expect("append invalid header");
    assert!(matches!(
        repository.finish(&session.session_id, 2).await,
        Err(DomainError::InvalidData(_))
    ));
    assert_eq!(
        repository
            .get_chat_payload_bytes("alice", "session")
            .await
            .expect("read preserved current"),
        old_payload.as_bytes()
    );

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn same_target_sessions_are_complete_and_last_finish_wins() {
    let (repository, root) = setup_repository().await;
    let payload_a = payload_to_jsonl(&payload_with_message(
        "same",
        "2026-01-01T00:00:00.000Z",
        "a",
        "Assistant",
    ));
    let payload_b = payload_to_jsonl(&payload_with_message(
        "same",
        "2026-01-01T00:00:01.000Z",
        "b",
        "Assistant",
    ));
    let target = character_target("alice", "session");
    let session_a = repository
        .begin(target.clone(), false)
        .await
        .expect("begin a");
    let session_b = repository.begin(target, false).await.expect("begin b");
    repository
        .append(&session_a.session_id, 0, payload_a.as_bytes())
        .await
        .expect("append a");
    repository
        .append(&session_b.session_id, 0, payload_b.as_bytes())
        .await
        .expect("append b");

    repository
        .finish(&session_a.session_id, payload_a.len() as u64)
        .await
        .expect("finish a");
    assert_eq!(
        repository
            .get_chat_payload_bytes("alice", "session")
            .await
            .expect("read a"),
        payload_a.as_bytes()
    );
    repository
        .finish(&session_b.session_id, payload_b.len() as u64)
        .await
        .expect("finish b");
    assert_eq!(
        repository
            .get_chat_payload_bytes("alice", "session")
            .await
            .expect("read b"),
        payload_b.as_bytes()
    );

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn chat_commit_sessions_have_a_small_hard_limit() {
    let (repository, root) = setup_repository().await;
    let target = character_target("alice", "session");
    let mut sessions = Vec::new();

    for _ in 0..MAX_ACTIVE_CHAT_COMMIT_SESSIONS {
        sessions.push(
            repository
                .begin(target.clone(), false)
                .await
                .expect("begin within session limit"),
        );
    }

    assert!(matches!(
        repository.begin(target.clone(), false).await,
        Err(DomainError::Conflict(_))
    ));

    let released = sessions.pop().expect("session to release");
    repository
        .abort(&released.session_id)
        .await
        .expect("release session capacity");
    let replacement = repository
        .begin(target, false)
        .await
        .expect("begin after releasing capacity");

    for session in sessions.into_iter().chain(std::iter::once(replacement)) {
        repository
            .abort(&session.session_id)
            .await
            .expect("abort test session");
    }

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn startup_cleanup_removes_only_chat_commit_staging() {
    let (repository, root) = setup_repository().await;
    let unrelated = root.join(".staging").join("other-state");
    fs::create_dir_all(&repository.chat_commit_staging_dir)
        .await
        .expect("create commit staging");
    fs::create_dir_all(&unrelated)
        .await
        .expect("create unrelated staging");
    fs::write(
        repository.chat_commit_staging_dir.join("orphan.partial"),
        b"partial",
    )
    .await
    .expect("write orphan");
    fs::write(unrelated.join("keep"), b"keep")
        .await
        .expect("write unrelated file");

    repository
        .cleanup_orphaned_chat_commit_staging()
        .await
        .expect("clean orphan staging");

    assert!(!repository.chat_commit_staging_dir.exists());
    assert!(unrelated.join("keep").exists());
    let _ = fs::remove_dir_all(root).await;
}

fn backup_policy(
    max_files_per_prefix: i64,
    max_total_files: i64,
    max_total_bytes: i64,
) -> ChatBackupSettings {
    ChatBackupSettings {
        automatic_enabled: true,
        zstd_compression_enabled: false,
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
        if name.starts_with("chat_") && (name.ends_with(".jsonl") || name.ends_with(".jsonl.zst")) {
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
async fn zstd_setting_converts_all_backups_in_both_directions() {
    let (repository, root) = setup_repository().await;
    let source = root.join("source.jsonl");
    let payload = payload_to_jsonl(&payload_with_integrity("format"));
    fs::write(&source, &payload).await.expect("write source");

    apply_and_reconcile_backups(&repository, backup_policy(-1, -1, -1)).await;
    repository
        .backup_chat_file_explicit(&source, "Alice")
        .await
        .expect("create raw backup");
    let raw_name = backup_file_names(&root).await.pop().expect("raw backup");
    let logical_name = raw_name.clone();
    let original_modified = fs::metadata(root.join("backups").join(&raw_name))
        .await
        .expect("read raw metadata")
        .modified()
        .expect("read raw mtime");

    let mut compressed_policy = backup_policy(-1, -1, -1);
    compressed_policy.zstd_compression_enabled = true;
    apply_and_reconcile_backups(&repository, compressed_policy).await;
    let compressed_name = backup_file_names(&root)
        .await
        .pop()
        .expect("converted zstd backup");
    assert_eq!(compressed_name, format!("{logical_name}.zst"));
    let compressed_path = root.join("backups").join(&compressed_name);
    assert_eq!(
        fs::metadata(&compressed_path)
            .await
            .expect("read compressed metadata")
            .modified()
            .expect("read compressed mtime"),
        original_modified
    );
    let materialized = repository
        .materialize_chat_backup(&logical_name)
        .await
        .expect("materialize converted backup");
    assert_eq!(
        fs::read(&materialized)
            .await
            .expect("read converted backup"),
        payload.as_bytes()
    );
    repository
        .discard_chat_backup_materialization(&materialized)
        .await
        .expect("discard converted backup");

    apply_and_reconcile_backups(&repository, backup_policy(-1, -1, -1)).await;
    assert_eq!(backup_file_names(&root).await, vec![logical_name.clone()]);
    let restored_path = root.join("backups").join(&logical_name);
    assert_eq!(
        fs::read(&restored_path)
            .await
            .expect("read restored raw backup"),
        payload.as_bytes()
    );
    assert_eq!(
        fs::metadata(&restored_path)
            .await
            .expect("read restored metadata")
            .modified()
            .expect("read restored mtime"),
        original_modified
    );

    let logical_names: Vec<_> = repository
        .list_chat_backup_files()
        .await
        .expect("list converged backups")
        .into_iter()
        .map(|entry| entry.file_name)
        .collect();
    assert_eq!(logical_names, vec![logical_name]);
}

#[tokio::test]
async fn zstd_to_raw_convergence_prunes_oldest_before_decoding_it() {
    let (repository, root) = setup_repository().await;
    let payload = format!(
        "{{\"chat_metadata\":{{}},\"user_name\":\"User\"}}\n{{\"mes\":\"{}\"}}",
        "x".repeat(64 * 1024)
    );
    let source = root.join("source.jsonl");
    fs::write(&source, &payload).await.expect("write source");

    let mut compressed_policy = backup_policy(-1, -1, -1);
    compressed_policy.zstd_compression_enabled = true;
    apply_and_reconcile_backups(&repository, compressed_policy).await;
    repository
        .backup_chat_file_explicit(&source, "Alice")
        .await
        .expect("create old compressed backup");
    repository
        .backup_chat_file_explicit(&source, "Alice")
        .await
        .expect("create new compressed backup");

    let compressed_names = backup_file_names(&root).await;
    assert_eq!(compressed_names.len(), 2);
    let old_path = root.join("backups").join(&compressed_names[0]);
    let new_path = root.join("backups").join(&compressed_names[1]);
    fs::write(&old_path, b"corrupt old compressed backup")
        .await
        .expect("corrupt oldest backup");
    set_backup_modified(&old_path, UNIX_EPOCH + Duration::from_secs(10))
        .await
        .expect("set old backup mtime");
    set_backup_modified(&new_path, UNIX_EPOCH + Duration::from_secs(20))
        .await
        .expect("set new backup mtime");

    apply_and_reconcile_backups(&repository, backup_policy(-1, -1, payload.len() as i64)).await;

    let expected = compressed_names[1]
        .strip_suffix(".zst")
        .expect("compressed suffix")
        .to_string();
    assert_eq!(backup_file_names(&root).await, vec![expected.clone()]);
    assert_eq!(
        fs::read(root.join("backups").join(&expected))
            .await
            .expect("read retained raw backup"),
        payload.as_bytes()
    );
}

#[tokio::test]
async fn automatic_deduplication_survives_raw_zstd_runtime_toggles() {
    let (repository, root) = setup_repository().await;
    apply_and_reconcile_backups(&repository, backup_policy(-1, -1, -1)).await;
    let payload_a = payload_to_jsonl(&payload_with_message(
        "toggle",
        "2026-01-01T00:00:00.000Z",
        "raw",
        "Assistant",
    ));
    commit_payload_bytes(
        &repository,
        character_target("Alice", "session"),
        payload_a.as_bytes(),
        false,
    )
    .await
    .expect("commit raw state");
    repository
        .backup_chat_automatic("Alice", "session")
        .await
        .expect("create raw automatic backup");

    let mut compressed_policy = backup_policy(-1, -1, -1);
    compressed_policy.zstd_compression_enabled = true;
    apply_and_reconcile_backups(&repository, compressed_policy).await;
    repository
        .backup_chat_automatic("Alice", "session")
        .await
        .expect("skip unchanged state after raw to zstd toggle");
    let names = backup_file_names(&root).await;
    assert_eq!(names.len(), 1);
    assert!(names.iter().all(|name| name.ends_with(".jsonl.zst")));

    let payload_b = payload_to_jsonl(&payload_with_message(
        "toggle",
        "2026-01-01T00:00:01.000Z",
        "zstd",
        "Assistant",
    ));
    commit_payload_bytes(
        &repository,
        character_target("Alice", "session"),
        payload_b.as_bytes(),
        false,
    )
    .await
    .expect("commit zstd state");
    repository
        .backup_chat_automatic("Alice", "session")
        .await
        .expect("create zstd automatic backup");

    apply_and_reconcile_backups(&repository, backup_policy(-1, -1, -1)).await;
    repository
        .backup_chat_automatic("Alice", "session")
        .await
        .expect("skip unchanged state after zstd to raw toggle");
    let names = backup_file_names(&root).await;
    assert_eq!(names.len(), 2);
    assert!(names.iter().all(|name| name.ends_with(".jsonl")));
}

#[tokio::test]
async fn zstd_backup_materialize_and_restore_are_streamed_and_byte_exact() {
    let (repository, root) = setup_repository().await;
    let mut policy = backup_policy(-1, -1, -1);
    policy.zstd_compression_enabled = true;
    apply_and_reconcile_backups(&repository, policy).await;

    let payload = format!(
        "{{\"chat_metadata\":{{\"integrity\":\"zstd\"}},\"user_name\":\"User\",\"character_name\":\"Alice\"}}\n{{\"name\":\"Alice\",\"is_user\":false,\"send_date\":\"2026-01-01T00:00:00.000Z\",\"mes\":\"{}\",\"extra\":{{}}}}",
        "重复内容".repeat(32_768)
    );
    let source = root.join("large-source.jsonl");
    fs::write(&source, payload.as_bytes())
        .await
        .expect("write large source");
    repository
        .backup_chat_file_explicit(&source, "Alice")
        .await
        .expect("create zstd backup");

    let descriptor = repository
        .list_chat_backup_files()
        .await
        .expect("list zstd backup")
        .pop()
        .expect("zstd backup descriptor");
    assert!(descriptor.path.to_string_lossy().ends_with(".jsonl.zst"));
    assert!(
        fs::metadata(&descriptor.path)
            .await
            .expect("read compressed metadata")
            .len()
            < payload.len() as u64
    );

    let materialized = repository
        .materialize_chat_backup(&descriptor.file_name)
        .await
        .expect("materialize zstd backup");
    assert_eq!(
        fs::read(&materialized)
            .await
            .expect("read materialized backup"),
        payload.as_bytes()
    );
    repository
        .discard_chat_backup_materialization(&materialized)
        .await
        .expect("discard materialized backup");
    assert!(!materialized.exists());

    let restored_character = repository
        .restore_character_chat_backup(&descriptor.file_name, "alice", "Alice")
        .await
        .expect("restore character chat");
    assert_eq!(restored_character.len(), 1);
    assert_eq!(
        repository
            .get_chat_payload_bytes("alice", &restored_character[0])
            .await
            .expect("read restored character chat"),
        payload.as_bytes()
    );

    let restored_group = repository
        .restore_group_chat_backup(&descriptor.file_name)
        .await
        .expect("restore group chat");
    assert_eq!(
        fs::read(
            repository
                .get_group_chat_payload_path(&restored_group)
                .await
                .expect("resolve restored group chat")
        )
        .await
        .expect("read restored group chat"),
        payload.as_bytes()
    );

    repository
        .delete_chat_backup(&descriptor.file_name)
        .await
        .expect("delete zstd backup by logical name");
    assert!(!descriptor.path.exists());
    let mut staging_entries = fs::read_dir(&repository.chat_commit_staging_dir)
        .await
        .expect("read chat staging directory");
    assert!(
        staging_entries
            .next_entry()
            .await
            .expect("read chat staging entry")
            .is_none()
    );
}

#[tokio::test]
async fn zstd_quota_uses_actual_compressed_bytes() {
    let (repository, root) = setup_repository().await;
    let payload = format!(
        "{{\"chat_metadata\":{{}},\"user_name\":\"User\"}}\n{{\"mes\":\"{}\"}}",
        "x".repeat(256 * 1024)
    );
    let source = root.join("compressible-source.jsonl");
    fs::write(&source, payload.as_bytes())
        .await
        .expect("write compressible source");

    let mut policy = backup_policy(-1, -1, 4096);
    policy.zstd_compression_enabled = true;
    apply_and_reconcile_backups(&repository, policy).await;
    repository
        .backup_chat_file_explicit(&source, "Alice")
        .await
        .expect("compressed candidate should fit physical quota");

    let name = backup_file_names(&root)
        .await
        .pop()
        .expect("compressed backup");
    assert!(name.ends_with(".jsonl.zst"));
    assert!(
        fs::metadata(root.join("backups").join(name))
            .await
            .expect("read compressed backup metadata")
            .len()
            <= 4096
    );
}

#[tokio::test]
async fn zstd_storage_stats_use_frame_headers_without_waiting_for_history() {
    let (repository, root) = setup_repository().await;
    let payload = format!(
        "{{\"chat_metadata\":{{}},\"user_name\":\"User\"}}\n{{\"mes\":\"{}\"}}",
        "x".repeat(64 * 1024)
    );
    let source = root.join("stats-source.jsonl");
    fs::write(&source, &payload)
        .await
        .expect("write stats source");

    let mut policy = backup_policy(-1, -1, -1);
    policy.zstd_compression_enabled = true;
    apply_and_reconcile_backups(&repository, policy).await;
    repository
        .backup_chat_file_explicit(&source, "Alice")
        .await
        .expect("create first compressed backup");
    repository
        .backup_chat_file_explicit(&source, "Alice")
        .await
        .expect("create second compressed backup");

    let mut stored_bytes = 0;
    for name in backup_file_names(&root).await {
        stored_bytes += fs::metadata(root.join("backups").join(name))
            .await
            .expect("read compressed backup metadata")
            .len();
    }
    let stats = repository
        .get_chat_backup_storage_stats()
        .await
        .expect("read backup storage stats")
        .expect("stable compressed stats");
    assert_eq!(stats.original_bytes, payload.len() as u64 * 2);
    assert_eq!(stats.stored_bytes, stored_bytes);
    assert!(stats.stored_bytes < stats.original_bytes);

    let history = repository.backup_history.lock().await;
    let mut query = repository.get_chat_backup_storage_stats();
    let waker = std::task::Waker::noop();
    let mut context = std::task::Context::from_waker(waker);
    let busy_result = match std::future::Future::poll(query.as_mut(), &mut context) {
        std::task::Poll::Ready(result) => result.expect("busy stats query"),
        std::task::Poll::Pending => panic!("stats query waited for backup maintenance"),
    };
    drop(history);
    assert_eq!(busy_result, None);
}

#[tokio::test]
async fn rejected_raw_candidate_does_not_stage_or_delete_history() {
    let (repository, root) = setup_repository().await;
    let small_payload = payload_to_jsonl(&payload_with_integrity("retained"));
    let small_source = root.join("small.jsonl");
    fs::write(&small_source, &small_payload)
        .await
        .expect("write retained source");
    apply_and_reconcile_backups(
        &repository,
        backup_policy(-1, -1, small_payload.len() as i64 + 32),
    )
    .await;
    repository
        .backup_chat_file_explicit(&small_source, "Alice")
        .await
        .expect("create retained backup");
    let retained_names = backup_file_names(&root).await;

    let large_source = root.join("large.jsonl");
    fs::write(&large_source, "x".repeat(small_payload.len() + 1024))
        .await
        .expect("write oversized source");
    assert!(matches!(
        repository
            .backup_chat_file_explicit(&large_source, "Alice")
            .await,
        Err(DomainError::Conflict(_))
    ));
    assert_eq!(backup_file_names(&root).await, retained_names);

    let mut entries = fs::read_dir(root.join("backups"))
        .await
        .expect("read backup directory");
    while let Some(entry) = entries.next_entry().await.expect("read backup entry") {
        assert!(
            !entry
                .file_name()
                .to_string_lossy()
                .starts_with(".tmp-chat-backup-")
        );
    }
}

#[tokio::test]
async fn truncated_zstd_backup_does_not_poison_healthy_inventory_entries() {
    let (repository, root) = setup_repository().await;
    let mut policy = backup_policy(-1, -1, -1);
    policy.zstd_compression_enabled = true;
    apply_and_reconcile_backups(&repository, policy).await;

    let source = root.join("source.jsonl");
    let healthy_payload = payload_to_jsonl(&payload_with_integrity("healthy"));
    fs::write(&source, &healthy_payload)
        .await
        .expect("write healthy source");
    repository
        .backup_chat_file_explicit(&source, "Alice")
        .await
        .expect("create healthy zstd backup");
    let healthy = repository
        .list_chat_backup_files()
        .await
        .expect("list healthy zstd backup")
        .pop()
        .expect("healthy zstd descriptor");

    fs::write(
        &source,
        payload_to_jsonl(&payload_with_integrity("truncated")),
    )
    .await
    .expect("write source to corrupt");
    repository
        .backup_chat_file_explicit(&source, "Alice")
        .await
        .expect("create zstd backup to corrupt");
    let descriptor = repository
        .list_chat_backup_files()
        .await
        .expect("list zstd backups")
        .into_iter()
        .find(|candidate| candidate.file_name != healthy.file_name)
        .expect("zstd descriptor to corrupt");
    let compressed_len = fs::metadata(&descriptor.path)
        .await
        .expect("read zstd metadata")
        .len();
    fs::OpenOptions::new()
        .write(true)
        .open(&descriptor.path)
        .await
        .expect("open zstd backup for truncation")
        .set_len(compressed_len - 4)
        .await
        .expect("truncate zstd checksum");
    set_backup_modified(&healthy.path, UNIX_EPOCH + Duration::from_secs(1))
        .await
        .expect("age healthy backup");
    set_backup_modified(&descriptor.path, UNIX_EPOCH + Duration::from_secs(2))
        .await
        .expect("make corrupt backup newest");

    assert!(
        repository
            .materialize_chat_backup(&descriptor.file_name)
            .await
            .is_err()
    );
    assert!(
        repository
            .restore_character_chat_backup(&descriptor.file_name, "alice", "Alice")
            .await
            .is_err()
    );
    let mut staging_entries = fs::read_dir(&repository.chat_commit_staging_dir)
        .await
        .expect("read chat staging directory");
    assert!(
        staging_entries
            .next_entry()
            .await
            .expect("read chat staging entry")
            .is_none()
    );

    repository
        .apply_chat_backup_settings(backup_policy(-1, -1, -1))
        .await
        .expect("disable compression");
    assert!(
        repository.reconcile_chat_backups().await.is_err(),
        "corrupt zstd must fail background conversion"
    );
    assert!(descriptor.path.exists());
    assert!(!root.join("backups").join(&descriptor.file_name).exists());
    assert_eq!(
        repository
            .list_chat_backup_files()
            .await
            .expect("list usable inventory after conversion failure")
            .len(),
        2
    );
    assert!(root.join("backups").join(&healthy.file_name).exists());
    assert!(!healthy.path.exists());
    let materialized = repository
        .materialize_chat_backup(&healthy.file_name)
        .await
        .expect("materialize converted healthy backup");
    assert_eq!(
        fs::read(&materialized)
            .await
            .expect("read converted healthy backup"),
        healthy_payload.as_bytes()
    );
    repository
        .discard_chat_backup_materialization(&materialized)
        .await
        .expect("discard healthy materialization");
    let mut backup_entries = fs::read_dir(root.join("backups"))
        .await
        .expect("read backup directory");
    while let Some(entry) = backup_entries
        .next_entry()
        .await
        .expect("read backup entry")
    {
        assert!(
            !entry
                .file_name()
                .to_string_lossy()
                .starts_with(".tmp-chat-backup-")
        );
    }
    repository
        .delete_chat_backup(&descriptor.file_name)
        .await
        .expect("delete failed conversion source");
    repository
        .reconcile_chat_backups()
        .await
        .expect("finish convergence after removing corrupt backup");
    assert_eq!(backup_file_names(&root).await, vec![healthy.file_name]);

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn inventory_recovers_interrupted_conversion_using_the_selected_format() {
    let (repository, root) = setup_repository().await;
    let logical_name = "chat_alice_20260101-000000.jsonl";
    let raw_path = root.join("backups").join(logical_name);
    let compressed_path = root.join("backups").join(format!("{logical_name}.zst"));
    let payload = payload_to_jsonl(&payload_with_integrity("collision"));
    fs::write(&raw_path, payload.as_bytes())
        .await
        .expect("write raw backup");
    let compressed = zstd::stream::encode_all(payload.as_bytes(), 1).expect("compress backup");
    fs::write(&compressed_path, &compressed)
        .await
        .expect("write zstd backup");

    repository
        .reconcile_chat_backups()
        .await
        .expect("raw policy should finish interrupted conversion");
    assert!(raw_path.exists());
    assert!(!compressed_path.exists());

    let raw_modified = fs::metadata(&raw_path)
        .await
        .expect("read raw metadata")
        .modified()
        .expect("read raw mtime");
    fs::write(&compressed_path, compressed)
        .await
        .expect("recreate zstd backup");
    let mut compressed_policy = backup_policy(-1, -1, -1);
    compressed_policy.zstd_compression_enabled = true;
    repository
        .apply_chat_backup_settings(compressed_policy)
        .await
        .expect("enable compression");
    repository
        .reconcile_chat_backups()
        .await
        .expect("zstd policy should finish interrupted conversion");
    assert!(!raw_path.exists());
    assert!(compressed_path.exists());
    assert_eq!(
        fs::metadata(&compressed_path)
            .await
            .expect("read recovered zstd metadata")
            .modified()
            .expect("read recovered zstd mtime"),
        raw_modified
    );
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

    commit_character_payload_file(&repository, "Alice", "session", &source, false)
        .await
        .expect("current save must succeed");
    assert!(backup_file_names(&root).await.is_empty());

    repository
        .backup_chat_automatic("Alice", "session")
        .await
        .expect("automatic quota rejection is an expected skip");
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
async fn automatic_character_and_group_snapshots_run_only_when_requested() {
    let (repository, root) = setup_repository().await;
    apply_and_reconcile_backups(&repository, backup_policy(-1, -1, -1)).await;
    let source = root.join("source.jsonl");
    fs::write(
        &source,
        payload_to_jsonl(&payload_with_integrity("automatic")),
    )
    .await
    .expect("write source");

    commit_character_payload_file(&repository, "Alice", "session", &source, false)
        .await
        .expect("save character current");
    commit_group_payload_file(&repository, "group-session", &source, false)
        .await
        .expect("save group current");
    assert!(backup_file_names(&root).await.is_empty());

    repository
        .backup_chat_automatic("Alice", "session")
        .await
        .expect("automatic character snapshot");
    repository
        .backup_group_chat_automatic("group-session")
        .await
        .expect("automatic group snapshot");

    assert_eq!(backup_file_names(&root).await.len(), 2);
}

#[tokio::test]
async fn automatic_deduplication_preserves_prefix_and_latest_state() {
    let (repository, root) = setup_repository().await;
    apply_and_reconcile_backups(&repository, backup_policy(-1, -1, -1)).await;
    let payload_a = payload_to_jsonl(&payload_with_message(
        "timeline",
        "2026-01-01T00:00:00.000Z",
        "alpha",
        "Assistant",
    ));
    let payload_b = payload_to_jsonl(&payload_with_message(
        "timeline",
        "2026-01-01T00:00:00.000Z",
        "bravo",
        "Assistant",
    ));
    assert_eq!(payload_a.len(), payload_b.len());

    commit_payload_bytes(
        &repository,
        character_target("Alice", "session-one"),
        payload_a.as_bytes(),
        false,
    )
    .await
    .expect("commit first state");
    repository
        .backup_chat_automatic("Alice", "session-one")
        .await
        .expect("create first automatic snapshot");
    let first_names = backup_file_names(&root).await;
    repository
        .backup_chat_automatic("Alice", "session-one")
        .await
        .expect("skip unchanged automatic snapshot");
    assert_eq!(backup_file_names(&root).await, first_names);

    commit_payload_bytes(
        &repository,
        character_target("Alice", "session-two"),
        payload_a.as_bytes(),
        false,
    )
    .await
    .expect("commit same bytes to another source");
    repository
        .backup_chat_automatic("Alice", "session-two")
        .await
        .expect("same bytes under the latest prefix remain redundant");
    assert_eq!(backup_file_names(&root).await.len(), 1);

    commit_payload_bytes(
        &repository,
        character_target("Alice", "session-one"),
        payload_b.as_bytes(),
        false,
    )
    .await
    .expect("commit second state with the same length");
    repository
        .backup_chat_automatic("Alice", "session-one")
        .await
        .expect("changed digest creates a snapshot");
    commit_payload_bytes(
        &repository,
        character_target("Alice", "session-one"),
        payload_a.as_bytes(),
        false,
    )
    .await
    .expect("restore first state");
    repository
        .backup_chat_automatic("Alice", "session-one")
        .await
        .expect("A-B-A creates the third timeline state");
    assert_eq!(backup_file_names(&root).await.len(), 3);

    let current_path = repository
        .get_chat_payload_path("Alice", "session-one")
        .await
        .expect("resolve current path");
    let _snapshot_guard = repository
        .acquire_payload_snapshot_lock(&current_path)
        .await;
    repository
        .backup_chat_file_automatic(&current_path, "Bob")
        .await
        .expect("same source under a new prefix gets a snapshot");
    assert_eq!(backup_file_names(&root).await.len(), 4);

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn settings_reconciliation_preserves_tracked_backup_provenance() {
    let (repository, root) = setup_repository().await;
    apply_and_reconcile_backups(&repository, backup_policy(-1, -1, -1)).await;
    let payload = payload_to_jsonl(&payload_with_integrity("explicit"));
    commit_payload_bytes(
        &repository,
        character_target("Alice", "session"),
        payload.as_bytes(),
        false,
    )
    .await
    .expect("commit current");

    repository
        .backup_chat("Alice", "session")
        .await
        .expect("first explicit snapshot");
    repository
        .backup_chat("Alice", "session")
        .await
        .expect("second explicit snapshot");
    repository
        .backup_chat_automatic("Alice", "session")
        .await
        .expect("automatic snapshot reuses the latest explicit signature");
    assert_eq!(backup_file_names(&root).await.len(), 2);

    repository
        .apply_chat_backup_settings(backup_policy(-1, 10, -1))
        .await
        .expect("change backup quota");
    repository
        .reconcile_chat_backups()
        .await
        .expect("rebuild inventory after a settings change");
    repository
        .backup_chat_automatic("Alice", "session")
        .await
        .expect("tracked snapshot still suppresses a duplicate");
    assert_eq!(backup_file_names(&root).await.len(), 2);

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn cache_reconcile_forgets_backup_provenance_before_the_background_rescan() {
    let (repository, root) = setup_repository().await;
    apply_and_reconcile_backups(&repository, backup_policy(-1, -1, -1)).await;
    let payload = payload_to_jsonl(&payload_with_integrity("external-reconcile"));

    commit_payload_bytes(
        &repository,
        character_target("Alice", "session"),
        payload.as_bytes(),
        false,
    )
    .await
    .expect("commit current");
    repository
        .backup_chat_automatic("Alice", "session")
        .await
        .expect("create tracked snapshot");
    assert_eq!(backup_file_names(&root).await.len(), 1);

    <FileChatRepository as ChatRepository>::clear_cache(&repository)
        .await
        .expect("clear external content provenance");
    commit_payload_bytes(
        &repository,
        character_target("Alice", "session"),
        payload.as_bytes(),
        false,
    )
    .await
    .expect("recommit current after cache reconcile");
    repository
        .backup_chat_automatic("Alice", "session")
        .await
        .expect("unknown backup provenance creates conservatively");
    repository
        .backup_chat_automatic("Alice", "session")
        .await
        .expect("newly tracked snapshot suppresses the next duplicate");
    assert_eq!(backup_file_names(&root).await.len(), 2);

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn automatic_duplicate_skip_does_not_rotate_quota_entry() {
    let (repository, root) = setup_repository().await;
    apply_and_reconcile_backups(&repository, backup_policy(1, 1, -1)).await;
    let payload = payload_to_jsonl(&payload_with_integrity("quota-duplicate"));
    commit_payload_bytes(
        &repository,
        character_target("Alice", "session"),
        payload.as_bytes(),
        false,
    )
    .await
    .expect("commit current");
    repository
        .backup_chat_automatic("Alice", "session")
        .await
        .expect("create quota entry");
    let names = backup_file_names(&root).await;

    repository
        .backup_chat_automatic("Alice", "session")
        .await
        .expect("skip duplicate before eviction");
    assert_eq!(backup_file_names(&root).await, names);

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn non_c2_mutation_fails_closed_and_group_uses_the_same_guard() {
    let (repository, root) = setup_repository().await;
    apply_and_reconcile_backups(&repository, backup_policy(-1, -1, -1)).await;
    let payload = payload_with_integrity("mutation");
    let jsonl = format!("{}\n", payload_to_jsonl(&payload));
    commit_payload_bytes(
        &repository,
        character_target("Alice", "session"),
        jsonl.as_bytes(),
        false,
    )
    .await
    .expect("commit current");
    repository
        .backup_chat_automatic("Alice", "session")
        .await
        .expect("create tracked snapshot");

    let current_path = repository
        .get_chat_payload_path("Alice", "session")
        .await
        .expect("resolve current path");
    repository
        .write_payload_to_path(&current_path, &payload, false)
        .await
        .expect("rewrite the same bytes through a non-C2 mutation");
    assert_eq!(
        fs::read(&current_path)
            .await
            .expect("read rewritten current"),
        jsonl.as_bytes()
    );
    assert!(
        repository
            .current_content_signature_for_size(&current_path, jsonl.len() as u64)
            .await
            .is_none()
    );
    repository
        .backup_chat_automatic("Alice", "session")
        .await
        .expect("unknown digest creates a conservative snapshot");
    assert_eq!(backup_file_names(&root).await.len(), 2);

    commit_payload_bytes(
        &repository,
        ChatPayloadTarget::Group {
            chat_id: "group-session".to_string(),
        },
        jsonl.as_bytes(),
        false,
    )
    .await
    .expect("commit group current");
    repository
        .backup_group_chat_automatic("group-session")
        .await
        .expect("create group snapshot");
    repository
        .backup_group_chat_automatic("group-session")
        .await
        .expect("skip unchanged group snapshot");
    assert_eq!(backup_file_names(&root).await.len(), 3);

    let group_path = repository
        .get_group_chat_path("group-session")
        .expect("resolve group path");
    assert!(
        repository
            .current_content_signature_for_size(&group_path, jsonl.len() as u64)
            .await
            .is_some()
    );
    <FileChatRepository as ChatRepository>::clear_cache(&repository)
        .await
        .expect("clear chat runtime cache");
    assert!(
        repository
            .current_content_signature_for_size(&group_path, jsonl.len() as u64)
            .await
            .is_none()
    );

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn automatic_snapshot_defers_instead_of_waiting_for_a_busy_current() {
    let (repository, root) = setup_repository().await;
    apply_and_reconcile_backups(&repository, backup_policy(-1, -1, -1)).await;
    let source = root.join("source.jsonl");
    fs::write(
        &source,
        payload_to_jsonl(&payload_with_integrity("automatic-busy")),
    )
    .await
    .expect("write source");
    commit_character_payload_file(&repository, "Alice", "session", &source, false)
        .await
        .expect("save character current");

    let current_path = repository
        .get_chat_payload_path("Alice", "session")
        .await
        .expect("resolve current path");
    let current_guard = repository
        .acquire_payload_mutation_lock(&current_path)
        .await;
    let error = repository
        .backup_chat_automatic("Alice", "session")
        .await
        .expect_err("busy current should defer the automatic snapshot");
    assert!(matches!(error, DomainError::Transient(_)));
    assert!(backup_file_names(&root).await.is_empty());

    drop(current_guard);
    repository
        .backup_chat_automatic("Alice", "session")
        .await
        .expect("automatic snapshot after current writer finishes");
    assert_eq!(backup_file_names(&root).await.len(), 1);
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

    commit_character_payload_file(&repository, "Alice", "session", &source, false)
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
    commit_character_payload_file(&repository, "alice", "session", &source, false)
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
async fn chat_commit_sanitizes_windows_unsafe_path_segments() {
    let (repository, root) = setup_repository().await;

    let character_name = "ali:ce";
    let file_name = "session:2026/02*21?";
    let raw_payload = payload_to_jsonl(&payload_with_integrity("bytes-safe-path"));
    let source = root.join("unsafe-path-source.jsonl");
    fs::write(&source, &raw_payload)
        .await
        .expect("write unsafe chat payload source");

    commit_character_payload_file(&repository, character_name, file_name, &source, false)
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
async fn chat_commit_preserves_unicode_and_upstream_spacing() {
    let (repository, root) = setup_repository().await;

    let character_name = "角色";
    let file_name = " 中文会话 .jsonl";
    let raw_payload = payload_to_jsonl(&payload_with_integrity("unicode-file-name"));
    let source = root.join("unicode-chat-source.jsonl");
    fs::write(&source, &raw_payload)
        .await
        .expect("write unicode chat payload source");

    commit_character_payload_file(&repository, character_name, file_name, &source, false)
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
async fn chat_commit_keeps_uppercase_jsonl_as_stem_text() {
    let (repository, root) = setup_repository().await;

    let raw_payload = payload_to_jsonl(&payload_with_integrity("uppercase-jsonl-stem"));
    let source = root.join("uppercase-jsonl-source.jsonl");
    fs::write(&source, &raw_payload)
        .await
        .expect("write uppercase jsonl chat payload source");

    commit_character_payload_file(&repository, "alice", "Story.JSONL", &source, false)
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

    commit_character_payload_file(&repository, "Alice#1", "followup", &source, false)
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
async fn chat_commit_rejects_chat_file_names_that_sanitize_to_empty() {
    let (repository, root) = setup_repository().await;

    let raw_payload = payload_to_jsonl(&payload_with_integrity("invalid-file-name"));
    let source = root.join("invalid-chat-name-source.jsonl");
    fs::write(&source, &raw_payload)
        .await
        .expect("write invalid chat payload source");

    let error = commit_character_payload_file(&repository, "alice", "*.jsonl", &source, false)
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
async fn chat_commit_rejects_names_that_lose_jsonl_suffix_after_truncation() {
    let (repository, root) = setup_repository().await;

    let raw_payload = payload_to_jsonl(&payload_with_integrity("truncated-extension"));
    let source = root.join("truncated-extension-source.jsonl");
    fs::write(&source, &raw_payload)
        .await
        .expect("write chat payload source");

    let overlong_file_name = "a".repeat(250);
    let error =
        commit_character_payload_file(&repository, "alice", &overlong_file_name, &source, false)
            .await
            .expect_err("chat file name must keep a complete jsonl suffix");

    assert!(
        matches!(error, DomainError::InvalidData(message) if message == "Invalid chat file name")
    );
    assert!(!root.join("chats").join("alice").exists());

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn chat_commit_enforces_integrity() {
    let (repository, root) = setup_repository().await;

    let source_a = root.join("source-a.jsonl");
    let payload_a = payload_to_jsonl(&payload_with_integrity("path-a"));
    fs::write(&source_a, &payload_a)
        .await
        .expect("write first source payload");

    commit_character_payload_file(&repository, "alice", "session", &source_a, false)
        .await
        .expect("save payload from source file");

    let source_b = root.join("source-b.jsonl");
    let payload_b = payload_to_jsonl(&payload_with_integrity("path-b"));
    fs::write(&source_b, &payload_b)
        .await
        .expect("write second source payload");

    let error = commit_character_payload_file(&repository, "alice", "session", &source_b, false)
        .await
        .expect_err("save should fail on integrity mismatch");
    assert!(matches!(error, DomainError::InvalidData(message) if message == "integrity"));

    commit_character_payload_file(&repository, "alice", "session", &source_b, true)
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
async fn chat_commit_rejects_missing_integrity_when_existing_has_one() {
    let (repository, root) = setup_repository().await;

    let source_a = root.join("source-with-integrity.jsonl");
    let payload_a = payload_to_jsonl(&payload_with_integrity("path-a"));
    fs::write(&source_a, &payload_a)
        .await
        .expect("write source payload with integrity");

    commit_character_payload_file(&repository, "alice", "session", &source_a, false)
        .await
        .expect("save payload from source file");

    let source_b = root.join("source-without-integrity.jsonl");
    let payload_b = payload_to_jsonl(&payload_without_integrity());
    fs::write(&source_b, &payload_b)
        .await
        .expect("write source payload without integrity");

    let error = commit_character_payload_file(&repository, "alice", "session", &source_b, false)
        .await
        .expect_err("save should fail when incoming integrity is missing");
    assert!(matches!(error, DomainError::InvalidData(message) if message == "integrity"));

    commit_character_payload_file(&repository, "alice", "session", &source_b, true)
        .await
        .expect("forced save should bypass missing integrity check");

    let _ = fs::remove_dir_all(&root).await;
}

#[tokio::test]
async fn concurrent_chat_commits_publish_only_complete_payloads() {
    let (repository, root) = setup_repository().await;
    let repository = Arc::new(repository);
    let payload_a = payload_to_jsonl(&payload_with_message(
        "path-concurrent",
        "2026-01-01T00:00:00.000Z",
        "concurrent-a",
        "Assistant",
    ));
    let payload_b = payload_to_jsonl(&payload_with_message(
        "path-concurrent",
        "2026-01-01T00:00:00.000Z",
        "concurrent-b",
        "Assistant",
    ));
    let target = character_target("alice", "session");
    let session_a = repository
        .begin(target.clone(), false)
        .await
        .expect("begin concurrent a");
    let session_b = repository
        .begin(target, false)
        .await
        .expect("begin concurrent b");
    repository
        .append(&session_a.session_id, 0, payload_a.as_bytes())
        .await
        .expect("stage concurrent a");
    repository
        .append(&session_b.session_id, 0, payload_b.as_bytes())
        .await
        .expect("stage concurrent b");

    let barrier = Arc::new(Barrier::new(3));
    let repository_a = Arc::clone(&repository);
    let repository_b = Arc::clone(&repository);
    let barrier_a = Arc::clone(&barrier);
    let barrier_b = Arc::clone(&barrier);
    let size_a = payload_a.len() as u64;
    let size_b = payload_b.len() as u64;

    let save_a = tokio::spawn(async move {
        barrier_a.wait().await;
        repository_a.finish(&session_a.session_id, size_a).await
    });
    let save_b = tokio::spawn(async move {
        barrier_b.wait().await;
        repository_b.finish(&session_b.session_id, size_b).await
    });
    barrier.wait().await;

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
    commit_group_payload_file(&repository, "group-session", &source, false)
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
async fn group_chat_commit_sanitizes_windows_unsafe_id() {
    let (repository, root) = setup_repository().await;

    let group_id = "group:one/2026*02?21";
    let raw_payload = payload_to_jsonl(&payload_with_integrity("group-safe-path"));
    let source = root.join("group-unsafe-id-source.jsonl");
    fs::write(&source, &raw_payload)
        .await
        .expect("write group payload source");

    commit_group_payload_file(&repository, group_id, &source, false)
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
async fn group_chat_commit_rejects_ids_that_sanitize_to_empty() {
    let (repository, root) = setup_repository().await;

    let raw_payload = payload_to_jsonl(&payload_with_integrity("group-invalid-id"));
    let source = root.join("group-invalid-id-source.jsonl");
    fs::write(&source, &raw_payload)
        .await
        .expect("write group payload source");

    let error = commit_group_payload_file(&repository, "*.jsonl", &source, false)
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
async fn group_chat_commit_enforces_integrity() {
    let (repository, root) = setup_repository().await;

    let source_a = root.join("group-source-a.jsonl");
    let payload_a = payload_to_jsonl(&payload_with_integrity("group-path-a"));
    fs::write(&source_a, &payload_a)
        .await
        .expect("write first group source payload");

    commit_group_payload_file(&repository, "group-session", &source_a, false)
        .await
        .expect("save group payload from source file");

    let source_b = root.join("group-source-b.jsonl");
    let payload_b = payload_to_jsonl(&payload_with_integrity("group-path-b"));
    fs::write(&source_b, &payload_b)
        .await
        .expect("write second group source payload");

    let error = commit_group_payload_file(&repository, "group-session", &source_b, false)
        .await
        .expect_err("save should fail on integrity mismatch");
    assert!(matches!(error, DomainError::InvalidData(message) if message == "integrity"));

    commit_group_payload_file(&repository, "group-session", &source_b, true)
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
    commit_group_payload_file(&repository, "group-session", &source, false)
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
    let cached_chat = repository
        .get_chat("alice", "session")
        .await
        .expect("prime chat memory cache");
    assert_eq!(cached_chat.messages[0].mes, "old message");

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
    let refreshed_chat = repository
        .get_chat("alice", "session")
        .await
        .expect("read chat after commit");
    assert_eq!(refreshed_chat.messages[0].mes, "new message");

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

    commit_character_payload_file(repository, character_name, file_name, &source_path, force)
        .await
        .map(|_| ())
}

#[tokio::test]
async fn chat_payload_paging_returns_tail_and_before_for_character_and_group() {
    let (repository, root) = setup_repository().await;
    let mut payload = vec![payload_with_integrity("paging-a")[0].clone()];
    for index in 0..4 {
        payload.push(json!({ "mes": format!("message {index}") }));
    }

    save_chat_payload_from_values(&repository, &root, "alice", "session", &payload, false)
        .await
        .expect("save character payload");
    save_group_chat_payload_from_values(&repository, &root, "group-session", &payload, false)
        .await
        .expect("save group payload");

    let character_tail = repository
        .get_chat_payload_tail_lines("alice", "session", 2)
        .await
        .expect("read character tail");
    assert_eq!(
        character_tail
            .lines
            .iter()
            .map(|line| serde_json::from_str::<Value>(line).expect("parse tail line"))
            .collect::<Vec<_>>(),
        payload[3..5]
    );
    assert!(character_tail.has_more_before);

    let character_before = repository
        .get_chat_payload_before_lines("alice", "session", character_tail.cursor, 2)
        .await
        .expect("read character prefix");
    assert_eq!(
        character_before
            .lines
            .iter()
            .map(|line| serde_json::from_str::<Value>(line).expect("parse prefix line"))
            .collect::<Vec<_>>(),
        payload[1..3]
    );
    assert!(!character_before.has_more_before);

    let group_tail = repository
        .get_group_chat_payload_tail_lines("group-session", 2)
        .await
        .expect("read group tail");
    assert_eq!(group_tail.lines, character_tail.lines);
    assert!(group_tail.has_more_before);

    let group_before = repository
        .get_group_chat_payload_before_lines("group-session", group_tail.cursor, 2)
        .await
        .expect("read group prefix");
    assert_eq!(group_before.lines, character_before.lines);
    assert!(!group_before.has_more_before);

    let stale_cursor = character_tail.cursor;
    payload.push(json!({ "mes": "message 4" }));
    save_chat_payload_from_values(&repository, &root, "alice", "session", &payload, false)
        .await
        .expect("replace character payload");
    let stale = repository
        .get_chat_payload_before_lines("alice", "session", stale_cursor, 1)
        .await;
    assert!(stale.is_err(), "stale paging cursor must be rejected");

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

    commit_group_payload_file(repository, chat_id, &source_path, force)
        .await
        .map(|_| ())
}

fn payload_to_jsonl(payload: &[Value]) -> String {
    payload
        .iter()
        .map(|item| serde_json::to_string(item).expect("serialize line"))
        .collect::<Vec<_>>()
        .join("\n")
}
