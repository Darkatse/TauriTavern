use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use chrono::DateTime;
use rand::random;
use serde_json::{Value, json};
use tokio::fs;

use crate::chat_directory_identity::new_shared_chat_alias_store_for_user_dir;
use tt_domain::errors::DomainError;
use tt_domain::models::settings::ChatBackupSettings;
use tt_ports::repositories::chat_payload_commit_repository::{
    ChatPayloadCommitRepository, ChatPayloadTarget, CommittedChatPayload,
};
use tt_ports::repositories::chat_repository::{
    ChatMessageRole, ChatMessageSearchFilters, ChatMessageSearchQuery, ChatRepository,
    FindLastMessageQuery,
};
use tt_ports::repositories::group_chat_repository::GroupChatRepository;
use tt_ports::settings::ChatBackupRuntime;

use super::FileChatRepository;
use super::backup_codec::set_backup_modified;
use super::chat_payload_commit::MAX_ACTIVE_CHAT_COMMIT_SESSIONS;

mod format_contract;

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

async fn cleanup_repository(repository: FileChatRepository, root: PathBuf) {
    // All mutations have finished. Flush waits for active index I/O; queued flushes
    // then see a clean cache and cannot write after the directory is removed.
    FileChatRepository::flush_backup_summary_cache(&repository.backup_summary_cache)
        .await
        .expect("finish backup index writes");
    drop(repository);
    fs::remove_dir_all(root)
        .await
        .expect("remove chat repository fixture");
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

async fn read_backup_payload(
    repository: &FileChatRepository,
    name: &str,
    chunk_bytes: usize,
) -> Result<Vec<u8>, DomainError> {
    let mut reader = repository.open_chat_backup_download(name).await?;
    let mut chunk = vec![0; chunk_bytes];
    let mut payload = Vec::new();
    loop {
        let bytes_read = reader.read(&mut chunk).await?;
        if bytes_read == 0 {
            return Ok(payload);
        }
        payload.extend_from_slice(&chunk[..bytes_read]);
    }
}

async fn commit_payload_bytes(
    repository: &FileChatRepository,
    target: ChatPayloadTarget,
    bytes: &[u8],
    force: bool,
) -> Result<CommittedChatPayload, DomainError> {
    let session = repository.begin(target, force, None).await?;
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
        .begin(character_target("alice", "session"), false, None)
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

    cleanup_repository(repository, root).await;
}

#[tokio::test]
async fn chat_commit_publishes_exact_bytes_only_after_all_frames_finish() {
    let (repository, root) = setup_repository().await;
    let target = character_target("alice", "multi-frame");
    let original = b"{}\n{\"mes\":\"old\"}";
    commit_payload_bytes(&repository, target.clone(), original, false)
        .await
        .unwrap();
    let payload = "{}\n{\"mes\":\"你好\"}".as_bytes();
    let session = repository.begin(target, false, None).await.unwrap();
    let mut start = 0;
    let mut offset = 0;
    for end in [1, 13, payload.len()] {
        offset = repository
            .append(&session.session_id, offset, &payload[start..end])
            .await
            .unwrap();
        start = end;
        assert_eq!(
            repository
                .get_chat_payload_bytes("alice", "multi-frame")
                .await
                .unwrap(),
            original
        );
    }
    repository
        .finish(&session.session_id, payload.len() as u64)
        .await
        .unwrap();
    assert_eq!(
        repository
            .get_chat_payload_bytes("alice", "multi-frame")
            .await
            .unwrap(),
        payload
    );
    cleanup_repository(repository, root).await;
}

#[tokio::test]
async fn metadata_commit_preserves_header_fields_and_exact_body_bytes() {
    let (repository, root) = setup_repository().await;
    let header = json!({
        "user_name": "Original user", "character_name": "Original character",
        "create_date": "legacy date", "unknown": { "keep": [1, true] },
        "chat_metadata": { "integrity": "92698fe9-2d05-54b0-9734-a55213d002d5", "deleted": true },
    });
    let mut body = format!(
        "\r\n{{ \"mes\": \"你好{}\", \"unknown\": [1, 2] }}\r\n\n{{\"mes\":\"last\"}}",
        "x".repeat(20_000)
    )
    .into_bytes();
    // A metadata edit must preserve even a body that cannot be decoded as UTF-8.
    body.push(0xff);
    for target in [
        character_target("Alice", "metadata"),
        ChatPayloadTarget::Group {
            chat_id: "metadata".into(),
        },
    ] {
        let path = repository
            .resolve_chat_commit_target(&target)
            .await
            .unwrap();
        for (original, expected_body) in [
            (
                [format!("\r\n\u{feff}\r\n{header}\r\n").as_bytes(), &body].concat(),
                body.as_slice(),
            ),
            (header.to_string().into_bytes(), b"".as_slice()),
        ] {
            commit_payload_bytes(&repository, target.clone(), &original, false)
                .await
                .unwrap();
            let metadata = json!({ "integrity": "92698fe9-2d05-54b0-9734-a55213d002d5", "variables": { "new": "value" } });
            repository
                .commit_metadata(target.clone(), metadata.clone())
                .await
                .unwrap();
            let updated = fs::read(&path).await.unwrap();
            let split = updated.iter().position(|byte| *byte == b'\n').unwrap();
            let mut expected_header = header.clone();
            expected_header["chat_metadata"] = metadata;
            assert_eq!(
                serde_json::from_slice::<Value>(&updated[..split]).unwrap(),
                expected_header
            );
            assert_eq!(&updated[split + 1..], expected_body);
        }
    }
    cleanup_repository(repository, root).await;
}

#[tokio::test]
async fn metadata_commit_rejects_invalid_updates_without_changing_the_file() {
    let (repository, root) = setup_repository().await;
    for target in [
        character_target("Alice", "metadata"),
        ChatPayloadTarget::Group {
            chat_id: "metadata".into(),
        },
    ] {
        let path = repository
            .resolve_chat_commit_target(&target)
            .await
            .unwrap();
        let missing = repository
            .commit_metadata(target.clone(), json!({}))
            .await
            .unwrap_err();
        assert!(matches!(missing, DomainError::NotFound(_)));
        assert!(!path.exists());

        let original = payload_to_jsonl(&payload_with_integrity(
            "92698fe9-2d05-54b0-9734-a55213d002d5",
        ));
        commit_payload_bytes(&repository, target.clone(), original.as_bytes(), false)
            .await
            .unwrap();
        for metadata in [
            json!({ "integrity": "55ca7f51-3c7c-5dca-980a-0d89bd5d038f" }),
            json!({}),
            Value::Null,
            json!([]),
            json!(1),
        ] {
            let error = repository
                .commit_metadata(target.clone(), metadata.clone())
                .await
                .unwrap_err();
            assert!(matches!(&error, DomainError::InvalidData(_)));
            if metadata.is_object() {
                assert!(
                    matches!(error, DomainError::InvalidData(message) if message == "integrity")
                );
            }
            assert_eq!(fs::read(&path).await.unwrap(), original.as_bytes());
        }
        for malformed in ["", "\n", "{broken}\n", "[]\n"] {
            fs::write(&path, malformed).await.unwrap();
            assert!(matches!(
                repository
                    .commit_metadata(target.clone(), json!({}))
                    .await
                    .unwrap_err(),
                DomainError::InvalidData(_)
            ));
            assert_eq!(fs::read(&path).await.unwrap(), malformed.as_bytes());
        }
        fs::write(&path, "{\"chat_metadata\":{}}\n").await.unwrap();
        repository
            .commit_metadata(
                target.clone(),
                json!({ "integrity": "7f565cd0-b165-5376-82e9-dc222dc3d5cd" }),
            )
            .await
            .unwrap();
        assert_eq!(
            repository
                .read_chat_metadata_from_path(&path)
                .await
                .unwrap(),
            json!({ "integrity": "7f565cd0-b165-5376-82e9-dc222dc3d5cd" })
        );
        let mut files = fs::read_dir(path.parent().unwrap()).await.unwrap();
        while let Some(file) = files.next_entry().await.unwrap() {
            assert_eq!(
                file.path(),
                path,
                "rejected updates must not leave staging files"
            );
        }
    }
    cleanup_repository(repository, root).await;
}

#[tokio::test]
async fn metadata_writers_invalidate_cached_reads_and_preserve_extension_semantics() {
    let (repository, root) = setup_repository().await;
    let target = character_target("Alice", "metadata");
    let original = payload_to_jsonl(&payload_with_integrity(
        "92698fe9-2d05-54b0-9734-a55213d002d5",
    ));
    commit_payload_bytes(&repository, target.clone(), original.as_bytes(), false)
        .await
        .unwrap();
    repository.get_chat("Alice", "metadata").await.unwrap();
    repository
        .get_character_chat_summary("Alice", "metadata", true)
        .await
        .unwrap();

    let metadata = json!({ "integrity": "92698fe9-2d05-54b0-9734-a55213d002d5", "variables": { "score": "1" } });
    repository
        .commit_metadata(target, metadata.clone())
        .await
        .unwrap();
    assert_eq!(
        repository
            .get_chat("Alice", "metadata")
            .await
            .unwrap()
            .chat_metadata
            .variables["score"],
        "1"
    );
    assert_eq!(
        repository
            .get_character_chat_summary("Alice", "metadata", true)
            .await
            .unwrap()
            .chat_metadata,
        Some(metadata)
    );

    for value in [json!({ "floor": 42 }), Value::Null] {
        repository
            .set_character_chat_metadata_extension("Alice", "metadata", "example", value.clone())
            .await
            .unwrap();
        let cached = repository.get_chat("Alice", "metadata").await.unwrap();
        assert_eq!(
            cached.chat_metadata.extensions.unwrap().get("example"),
            value.as_object().map(|_| &value)
        );
        let summary = repository
            .get_character_chat_summary("Alice", "metadata", true)
            .await
            .unwrap()
            .chat_metadata
            .unwrap();
        assert_eq!(summary["variables"]["score"], "1");
        assert_eq!(
            summary["extensions"].get("example"),
            value.as_object().map(|_| &value)
        );
    }

    cleanup_repository(repository, root).await;
}

#[tokio::test]
async fn metadata_commit_does_not_skip_backup_after_an_equal_length_change() {
    let (repository, root) = setup_repository().await;
    apply_and_reconcile_backups(&repository, backup_policy(-1, -1, -1)).await;
    let target = character_target("Alice", "metadata");
    let mut payload = payload_with_integrity("92698fe9-2d05-54b0-9734-a55213d002d5");
    payload[0]["chat_metadata"]["variables"] = json!({ "score": "1" });
    let original = payload_to_jsonl(&payload);
    commit_payload_bytes(&repository, target.clone(), original.as_bytes(), false)
        .await
        .unwrap();
    repository
        .backup_chat_automatic("Alice", "metadata")
        .await
        .unwrap();
    let before = backup_file_names(&root).await;

    payload[0]["chat_metadata"]["variables"]["score"] = json!("2");
    repository
        .commit_metadata(target, payload[0]["chat_metadata"].clone())
        .await
        .unwrap();
    let updated = repository
        .get_chat_payload_bytes("Alice", "metadata")
        .await
        .unwrap();
    assert_eq!(updated.len(), original.len());
    repository
        .backup_chat_automatic("Alice", "metadata")
        .await
        .unwrap();
    let after = backup_file_names(&root).await;
    assert_eq!(after.len(), before.len() + 1);
    let added = after.iter().find(|file| !before.contains(file)).unwrap();
    assert_eq!(
        read_backup_payload(&repository, added, 1024).await.unwrap(),
        updated
    );
    cleanup_repository(repository, root).await;
}

#[tokio::test]
async fn cache_clear_during_commit_keeps_automatic_backups_conservative() {
    let (repository, root) = setup_repository().await;
    apply_and_reconcile_backups(&repository, backup_policy(-1, -1, -1)).await;
    let payload = b"{}";
    let session = repository
        .begin(character_target("Alice", "session"), false, None)
        .await
        .unwrap();
    repository
        .append(&session.session_id, 0, payload)
        .await
        .unwrap();
    <FileChatRepository as ChatRepository>::clear_cache(&repository)
        .await
        .unwrap();
    repository
        .finish(&session.session_id, payload.len() as u64)
        .await
        .unwrap();

    for _ in 0..2 {
        repository
            .backup_chat_automatic("Alice", "session")
            .await
            .unwrap();
    }
    assert_eq!(backup_file_names(&root).await.len(), 2);
    cleanup_repository(repository, root).await;
}

#[tokio::test]
async fn chat_commit_size_mismatch_preserves_current_and_consumes_session() {
    let (repository, root) = setup_repository().await;
    let old_payload = payload_to_jsonl(&payload_with_message(
        "d6ddecec-7267-5279-a976-56f8762960a0",
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
        "d6ddecec-7267-5279-a976-56f8762960a0",
        "2026-01-01T00:00:01.000Z",
        "new",
        "Assistant",
    ));
    let session = repository
        .begin(character_target("alice", "session"), false, None)
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

    cleanup_repository(repository, root).await;
}

#[tokio::test]
async fn same_target_sessions_are_complete_and_last_finish_wins() {
    let (repository, root) = setup_repository().await;
    let payload_a = b"{}\n{\"mes\":\"a\"}";
    let payload_b = b"{}\n{\"mes\":\"b\"}";
    let target = character_target("alice", "session");
    let session_a = repository.begin(target.clone(), false, None).await.unwrap();
    let session_b = repository.begin(target.clone(), false, None).await.unwrap();
    repository
        .append(&session_a.session_id, 0, payload_a)
        .await
        .unwrap();
    repository
        .append(&session_b.session_id, 0, payload_b)
        .await
        .unwrap();

    tokio::try_join!(
        repository.finish(&session_a.session_id, payload_a.len() as u64),
        repository.finish(&session_b.session_id, payload_b.len() as u64),
    )
    .expect("both concurrent commits succeed");
    let published = repository
        .get_chat_payload_bytes("alice", "session")
        .await
        .unwrap();
    assert!(published == payload_a || published == payload_b);

    let last = b"{}\n{\"mes\":\"last\"}";
    commit_payload_bytes(&repository, target, last, false)
        .await
        .unwrap();
    assert_eq!(
        repository
            .get_chat_payload_bytes("alice", "session")
            .await
            .unwrap(),
        last
    );
    cleanup_repository(repository, root).await;
}

#[tokio::test]
async fn chat_commit_sessions_have_a_small_hard_limit() {
    let (repository, root) = setup_repository().await;
    let target = character_target("alice", "session");
    let mut sessions = Vec::new();

    for _ in 0..MAX_ACTIVE_CHAT_COMMIT_SESSIONS {
        sessions.push(
            repository
                .begin(target.clone(), false, None)
                .await
                .expect("begin within session limit"),
        );
    }

    assert!(matches!(
        repository.begin(target.clone(), false, None).await,
        Err(DomainError::Conflict(_))
    ));

    let released = sessions.pop().expect("session to release");
    repository
        .abort(&released.session_id)
        .await
        .expect("release session capacity");
    let replacement = repository
        .begin(target, false, None)
        .await
        .expect("begin after releasing capacity");

    for session in sessions.into_iter().chain(std::iter::once(replacement)) {
        repository
            .abort(&session.session_id)
            .await
            .expect("abort test session");
    }

    cleanup_repository(repository, root).await;
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
    cleanup_repository(repository, root).await;
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

#[tokio::test]
async fn zstd_setting_converts_all_backups_in_both_directions() {
    let (repository, root) = setup_repository().await;
    let source = root.join("source.jsonl");
    let payload = payload_to_jsonl(&payload_with_integrity(
        "945cfdae-6502-5126-a0ef-783d446b8b44",
    ));
    fs::write(&source, &payload).await.expect("write source");

    apply_and_reconcile_backups(&repository, backup_policy(-1, -1, -1)).await;
    repository
        .backup_chat_file_explicit(&source, "Alice")
        .await
        .expect("create raw backup");
    let raw_name = backup_file_names(&root).await.pop().expect("raw backup");
    let logical_name = raw_name.clone();
    // Exercise count backfill from conversion rather than signature-only preservation.
    repository.remove_backup_summary(&logical_name).await;
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
    assert_eq!(
        repository
            .list_chat_backup_catalog()
            .await
            .expect("list compressed backup catalog")[0]
            .message_count,
        Some(1)
    );
    let downloaded = read_backup_payload(&repository, &logical_name, 11)
        .await
        .expect("stream converted backup");
    assert_eq!(downloaded, payload.as_bytes());

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
    assert_eq!(
        repository
            .list_chat_backup_catalog()
            .await
            .expect("list raw backup catalog")[0]
            .message_count,
        Some(1)
    );

    let logical_names: Vec<_> = repository
        .list_chat_backup_entries()
        .await
        .expect("list converged backups")
        .into_iter()
        .map(|entry| entry.logical_file_name)
        .collect();
    assert_eq!(logical_names, vec![logical_name]);

    cleanup_repository(repository, root).await;
}

#[tokio::test]
async fn raw_and_zstd_backups_download_restore_and_delete_by_logical_name() {
    for compression in [false, true] {
        let (repository, root) = setup_repository().await;
        let mut policy = backup_policy(-1, -1, -1);
        policy.zstd_compression_enabled = compression;
        apply_and_reconcile_backups(&repository, policy).await;

        let payload = format!(
            "\r\n\u{feff}\r\n{{\"chat_metadata\":{{\"integrity\":\"1a5043ab-06bc-58d8-9dcf-f751842e32e9\"}},\"user_name\":\"User\",\"character_name\":\"Alice\"}}\n{{\"name\":\"Alice\",\"is_user\":false,\"send_date\":\"2026-01-01T00:00:00.000Z\",\"mes\":\"{}\",\"extra\":{{}}}}",
            "重复内容".repeat(32_768)
        );
        let source = root.join("large-source.jsonl");
        fs::write(&source, payload.as_bytes())
            .await
            .expect("write large source");
        let backup_name = "角色👩🏽‍💻ก่าか\u{3099}हिन्दीe\u{301}☕\u{fe0f}";
        repository
            .backup_chat_file_explicit(&source, backup_name)
            .await
            .expect("create backup");

        FileChatRepository::flush_backup_summary_cache(&repository.backup_summary_cache)
            .await
            .expect("flush before reopening repository");
        drop(repository);
        let repository = repository_for_root(&root);
        apply_and_reconcile_backups(&repository, policy).await;

        let descriptor = repository
            .list_chat_backup_entries()
            .await
            .expect("list backup")
            .pop()
            .expect("backup descriptor");
        assert!(
            descriptor
                .logical_file_name
                .starts_with(&format!("chat_{backup_name}_"))
        );
        assert!(matches!(
            repository
                .open_chat_backup_download(&format!("{}.partial", descriptor.logical_file_name))
                .await,
            Err(DomainError::InvalidData(_))
        ));

        let summaries = repository.list_chat_backups().await.unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].character_name, "Alice");
        assert_eq!(summaries[0].message_count, 1);
        assert_eq!(
            summaries[0].preview,
            format!("...{}", "重复内容".repeat(100))
        );

        let files_before = backup_file_names(&root).await;
        let downloaded = read_backup_payload(&repository, &descriptor.logical_file_name, 4096)
            .await
            .expect("stream backup");
        assert_eq!(downloaded, payload.as_bytes());
        assert_eq!(backup_file_names(&root).await, files_before);
        assert!(!repository.chat_commit_staging_dir.exists());

        let restored_character = repository
            .restore_character_chat_backup(&descriptor.logical_file_name, "alice", "Alice")
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
            .restore_group_chat_backup(&descriptor.logical_file_name)
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
            .delete_chat_backup(&descriptor.logical_file_name)
            .await
            .expect("delete backup by logical name");
        assert!(!root.join("backups").join(&descriptor.file_name).exists());
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
        cleanup_repository(repository, root).await;
    }
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

    cleanup_repository(repository, root).await;
}

#[tokio::test]
async fn rejected_raw_candidate_does_not_stage_or_delete_history() {
    let (repository, root) = setup_repository().await;
    let small_payload = payload_to_jsonl(&payload_with_integrity(
        "938c7216-88af-52ae-a02f-8dea9d7f3fba",
    ));
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

    cleanup_repository(repository, root).await;
}

#[tokio::test]
async fn truncated_zstd_backup_does_not_poison_healthy_inventory_entries() {
    let (repository, root) = setup_repository().await;
    let mut policy = backup_policy(-1, -1, -1);
    policy.zstd_compression_enabled = true;
    apply_and_reconcile_backups(&repository, policy).await;

    let source = root.join("source.jsonl");
    let healthy_payload = payload_to_jsonl(&payload_with_integrity(
        "976a0bc9-79c7-5c79-befd-8e43897b2a2b",
    ));
    fs::write(&source, &healthy_payload)
        .await
        .expect("write healthy source");
    repository
        .backup_chat_file_explicit(&source, "Alice")
        .await
        .expect("create healthy zstd backup");
    let healthy = repository
        .list_chat_backup_entries()
        .await
        .expect("list healthy zstd backup")
        .pop()
        .expect("healthy zstd descriptor");

    fs::write(
        &source,
        payload_to_jsonl(&payload_with_integrity(
            "fe4b8528-255a-5743-b06f-a46512fe87ca",
        )),
    )
    .await
    .expect("write source to corrupt");
    repository
        .backup_chat_file_explicit(&source, "Alice")
        .await
        .expect("create zstd backup to corrupt");
    let descriptor = repository
        .list_chat_backup_entries()
        .await
        .expect("list zstd backups")
        .into_iter()
        .find(|candidate| candidate.logical_file_name != healthy.logical_file_name)
        .expect("zstd descriptor to corrupt");
    let compressed_len = fs::metadata(&root.join("backups").join(&descriptor.file_name))
        .await
        .expect("read zstd metadata")
        .len();
    fs::OpenOptions::new()
        .write(true)
        .open(&root.join("backups").join(&descriptor.file_name))
        .await
        .expect("open zstd backup for truncation")
        .set_len(compressed_len - 4)
        .await
        .expect("truncate zstd checksum");
    set_backup_modified(
        &root.join("backups").join(&healthy.file_name),
        UNIX_EPOCH + Duration::from_secs(1),
    )
    .await
    .expect("age healthy backup");
    set_backup_modified(
        &root.join("backups").join(&descriptor.file_name),
        UNIX_EPOCH + Duration::from_secs(2),
    )
    .await
    .expect("make corrupt backup newest");

    assert!(
        read_backup_payload(&repository, &descriptor.logical_file_name, 4096)
            .await
            .is_err()
    );
    assert!(
        repository
            .restore_character_chat_backup(&descriptor.logical_file_name, "alice", "Alice")
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
    assert!(root.join("backups").join(&descriptor.file_name).exists());
    assert!(
        !root
            .join("backups")
            .join(&descriptor.logical_file_name)
            .exists()
    );
    assert_eq!(
        repository
            .list_chat_backup_entries()
            .await
            .expect("list usable inventory after conversion failure")
            .len(),
        2
    );
    assert!(
        root.join("backups")
            .join(&healthy.logical_file_name)
            .exists()
    );
    assert!(!root.join("backups").join(&healthy.file_name).exists());
    let downloaded = read_backup_payload(&repository, &healthy.logical_file_name, 13)
        .await
        .expect("stream converted healthy backup");
    assert_eq!(downloaded, healthy_payload.as_bytes());
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
        .delete_chat_backup(&descriptor.logical_file_name)
        .await
        .expect("delete failed conversion source");
    repository
        .reconcile_chat_backups()
        .await
        .expect("finish convergence after removing corrupt backup");
    assert_eq!(
        backup_file_names(&root).await,
        vec![healthy.logical_file_name]
    );

    cleanup_repository(repository, root).await;
}

#[tokio::test]
async fn inventory_recovers_interrupted_conversion_using_the_selected_format() {
    let (repository, root) = setup_repository().await;
    let logical_name = "chat_alice_20260101-000000.jsonl";
    let raw_path = root.join("backups").join(logical_name);
    let compressed_path = root.join("backups").join(format!("{logical_name}.zst"));
    let payload = payload_to_jsonl(&payload_with_integrity(
        "2da3e6dc-7e74-548a-99e0-a818cd1063ee",
    ));
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

    cleanup_repository(repository, root).await;
}

#[tokio::test]
async fn inventory_enforces_prefix_and_global_file_limits() {
    let (repository, root) = setup_repository().await;
    let mut policy = backup_policy(2, 3, -1);
    policy.zstd_compression_enabled = true;
    apply_and_reconcile_backups(&repository, policy).await;
    let source = root.join("source.jsonl");
    fs::write(
        &source,
        payload_to_jsonl(&payload_with_integrity(
            "f00f6c0b-3a51-55e8-bffa-4d63f0c6c9da",
        )),
    )
    .await
    .expect("write source");

    let keys = [
        ("角:色-A* Nameก่า👩🏽‍💻", "chat_角色_a_nameก่า👩🏽‍💻", 3),
        ("角:色-A* Nameก้า👩🏿‍💻", "chat_角色_a_nameก้า👩🏿‍💻", 2),
    ];
    for (name, _, count) in keys {
        let long_name = name.repeat(100);
        for _ in 0..count {
            repository
                .backup_chat_file_explicit(&source, &long_name)
                .await
                .expect("backup Unicode name without overwriting an existing file");
        }
    }

    let names = backup_file_names(&root).await;
    assert_eq!(names.len(), 3);
    assert!(names.iter().all(|name| name.len() <= 255));
    for (_, prefix, _) in keys {
        let retained = names.iter().filter(|name| name.starts_with(prefix)).count();
        assert!(
            (1..=2).contains(&retained),
            "retained {retained} backups for {prefix}"
        );
    }

    cleanup_repository(repository, root).await;
}

#[tokio::test]
async fn reconcile_prunes_legacy_overage_and_zero_limit_clears_history() {
    let (repository, root) = setup_repository().await;
    for index in 0..4 {
        fs::write(
            root.join("backups")
                .join(format!("chat_alice_20260101-00000{index}.jsonl")),
            payload_to_jsonl(&payload_with_integrity(
                "23c20f0a-d721-56f4-a070-93b0e4fc04d2",
            )),
        )
        .await
        .expect("write legacy backup");
    }

    apply_and_reconcile_backups(&repository, backup_policy(2, 3, -1)).await;
    assert_eq!(backup_file_names(&root).await.len(), 2);

    apply_and_reconcile_backups(&repository, backup_policy(0, -1, -1)).await;
    assert!(backup_file_names(&root).await.is_empty());

    cleanup_repository(repository, root).await;
}

#[tokio::test]
async fn automatic_quota_rejection_does_not_fail_current_save() {
    let (repository, root) = setup_repository().await;
    apply_and_reconcile_backups(&repository, backup_policy(-1, -1, 1)).await;
    let payload = payload_to_jsonl(&payload_with_integrity(
        "39b864b4-8182-5c5c-83eb-2cc4c27d44d1",
    ));

    commit_payload_bytes(
        &repository,
        character_target("Alice", "session"),
        payload.as_bytes(),
        false,
    )
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

    cleanup_repository(repository, root).await;
}

#[tokio::test]
async fn automatic_deduplication_preserves_prefix_and_latest_state() {
    let (repository, root) = setup_repository().await;
    apply_and_reconcile_backups(&repository, backup_policy(-1, -1, -1)).await;
    let payload_a = payload_to_jsonl(&payload_with_message(
        "0f62c7c2-a8a9-56a3-ac00-90277833bf63",
        "2026-01-01T00:00:00.000Z",
        "alpha",
        "Assistant",
    ));
    let payload_b = payload_to_jsonl(&payload_with_message(
        "0f62c7c2-a8a9-56a3-ac00-90277833bf63",
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

    cleanup_repository(repository, root).await;
}

#[tokio::test]
async fn non_c2_mutation_fails_closed_and_group_uses_the_same_guard() {
    let (repository, root) = setup_repository().await;
    apply_and_reconcile_backups(&repository, backup_policy(-1, -1, -1)).await;
    let payload = payload_with_integrity("8f145f8a-090d-5fe0-888f-5a6abd9bceca");
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
        .commit_metadata(
            character_target("Alice", "session"),
            payload[0]["chat_metadata"].clone(),
        )
        .await
        .expect("rewrite the same bytes through a non-C2 mutation");
    assert_eq!(
        fs::read(&current_path)
            .await
            .expect("read rewritten current"),
        jsonl.as_bytes()
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

    <FileChatRepository as ChatRepository>::clear_cache(&repository)
        .await
        .expect("clear chat runtime cache");
    repository
        .backup_group_chat_automatic("group-session")
        .await
        .unwrap();
    assert_eq!(backup_file_names(&root).await.len(), 4);

    cleanup_repository(repository, root).await;
}

#[tokio::test]
async fn automatic_snapshot_defers_instead_of_waiting_for_a_busy_current() {
    let (repository, root) = setup_repository().await;
    apply_and_reconcile_backups(&repository, backup_policy(-1, -1, -1)).await;
    commit_payload_bytes(
        &repository,
        character_target("Alice", "session"),
        b"{}",
        false,
    )
    .await
    .unwrap();

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

    cleanup_repository(repository, root).await;
}

#[tokio::test]
async fn explicit_access_retries_a_failed_inventory_build() {
    let (repository, root) = setup_repository().await;
    let backups = root.join("backups");
    fs::remove_dir(&backups).await.unwrap();
    fs::write(&backups, b"blocks directory access")
        .await
        .unwrap();
    assert!(repository.list_chat_backup_catalog().await.is_err());

    fs::remove_file(&backups).await.unwrap();
    fs::create_dir(&backups).await.unwrap();
    fs::write(backups.join("chat_external_20260101-000000.jsonl"), b"{}")
        .await
        .unwrap();
    assert_eq!(
        repository.list_chat_backup_catalog().await.unwrap().len(),
        1
    );
    cleanup_repository(repository, root).await;
}

#[tokio::test]
async fn chat_commit_sanitizes_paths_and_preserves_unicode_spacing() {
    let (repository, root) = setup_repository().await;
    for (character, name, read_name, expected) in [
        (
            "ali:ce",
            "session:2026/02*21?",
            "session:2026/02*21?",
            "alice/session20260221.jsonl",
        ),
        (
            "角色",
            " 中文会话 .jsonl",
            " 中文会话 ",
            "角色/ 中文会话 .jsonl",
        ),
    ] {
        commit_payload_bytes(&repository, character_target(character, name), b"{}", false)
            .await
            .unwrap();
        assert_eq!(
            repository
                .get_chat_payload_path(character, read_name)
                .await
                .unwrap(),
            root.join("chats").join(expected)
        );
        assert_eq!(
            repository
                .get_chat_payload_bytes(character, read_name)
                .await
                .unwrap(),
            b"{}"
        );
    }
    cleanup_repository(repository, root).await;
}

#[tokio::test]
async fn shared_legacy_aliases_survive_concurrent_discovery_and_restart() {
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

    fs::create_dir_all(root.join("characters")).await.unwrap();
    for (dir_name, integrity) in [
        ("Alice", "88d48985-6eec-55bf-97d3-497203d5a324"),
        ("Bob", "a2402e7d-fc18-501b-ba6c-71eb088ac1ee"),
    ] {
        fs::write(
            root.join("characters").join(format!("{dir_name}#1.png")),
            b"",
        )
        .await
        .unwrap();
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
        payload_to_jsonl(&payload_with_integrity(
            "88d48985-6eec-55bf-97d3-497203d5a324"
        ))
        .as_bytes()
    );
    assert_eq!(
        loaded_b,
        payload_to_jsonl(&payload_with_integrity(
            "a2402e7d-fc18-501b-ba6c-71eb088ac1ee"
        ))
        .as_bytes()
    );

    // Saved aliases must still win after restart, even if canonical directories now exist.
    let reopened = repository_for_root(&root);
    for (character, expected) in [("Alice#1", loaded_a), ("Bob#1", loaded_b)] {
        fs::create_dir(root.join("chats").join(character))
            .await
            .unwrap();
        assert_eq!(
            reopened
                .get_chat_payload_bytes(character, "session")
                .await
                .unwrap(),
            expected
        );
    }

    drop(repository_a);
    drop(repository_b);
    cleanup_repository(reopened, root).await;
}

#[tokio::test]
async fn chat_commit_rejects_empty_names_and_truncated_jsonl_suffixes() {
    let (repository, root) = setup_repository().await;
    for name in ["*.jsonl".to_string(), "a".repeat(250)] {
        assert!(matches!(
            repository
                .begin(character_target("alice", &name), false, None)
                .await,
            Err(DomainError::InvalidData(_))
        ));
    }
    assert!(!root.join("chats/alice").exists());
    cleanup_repository(repository, root).await;
}

#[tokio::test]
async fn chat_commit_requires_force_to_replace_or_remove_existing_integrity() {
    let (repository, root) = setup_repository().await;
    let original = br#"{"chat_metadata":{"integrity":" identity "}}"#;
    for (target, path) in [
        (
            character_target("alice", "session"),
            root.join("chats/alice/session.jsonl"),
        ),
        (
            ChatPayloadTarget::Group {
                chat_id: "group-session".into(),
            },
            root.join("group chats/group-session.jsonl"),
        ),
    ] {
        for incoming in [
            br#"{"chat_metadata":{"integrity":"identity"}}"#.as_slice(),
            b"{}",
        ] {
            commit_payload_bytes(&repository, target.clone(), original, true)
                .await
                .unwrap();
            let error = commit_payload_bytes(&repository, target.clone(), incoming, false)
                .await
                .unwrap_err();
            assert!(matches!(error, DomainError::InvalidData(message) if message == "integrity"));
            assert_eq!(fs::read(&path).await.unwrap(), original);
            commit_payload_bytes(&repository, target.clone(), incoming, true)
                .await
                .unwrap();
            assert_eq!(fs::read(&path).await.unwrap(), incoming);
        }
    }
    cleanup_repository(repository, root).await;
}

#[tokio::test]
async fn save_and_load_chat_preserves_additional_fields() {
    let (repository, root) = setup_repository().await;

    let payload = vec![
        json!({
            "chat_metadata": {
                "integrity": "987c74ae-cd97-51d8-847b-a5dc2f6cdf84",
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

    commit_payload_bytes(
        &repository,
        character_target("alice", "session"),
        payload_to_jsonl(&payload).as_bytes(),
        false,
    )
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

    cleanup_repository(repository, root).await;
}

#[tokio::test]
async fn group_chat_payload_roundtrip_and_delete() {
    let (repository, root) = setup_repository().await;
    let payload = payload_with_integrity("241855c8-7d0f-5c84-9b6c-949b417f0743");

    commit_payload_bytes(
        &repository,
        ChatPayloadTarget::Group {
            chat_id: "group-session".into(),
        },
        payload_to_jsonl(&payload).as_bytes(),
        false,
    )
    .await
    .unwrap();

    let payload_path = repository
        .get_group_chat_payload_path("group-session")
        .await
        .expect("get group payload path");
    let saved = crate::chat_jsonl::read_payload(&payload_path)
        .await
        .expect("read group payload");
    assert_eq!(saved, payload);

    repository
        .delete_group_chat_payload("group-session")
        .await
        .expect("delete group chat payload");

    let deleted = repository
        .get_group_chat_payload_path("group-session")
        .await;
    assert!(matches!(deleted, Err(DomainError::NotFound(_))));

    cleanup_repository(repository, root).await;
}

#[tokio::test]
async fn import_chat_payload_creates_unique_files() {
    let (repository, root) = setup_repository().await;

    let import_path = root.join("import.jsonl");
    let import_content = payload_to_jsonl(&payload_with_integrity(
        "de03bd09-1535-5105-b5d3-37baf9e0a476",
    ));
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

    cleanup_repository(repository, root).await;
}

#[tokio::test]
async fn import_chat_payload_preserves_unchanged_jsonl_bytes() {
    let (repository, root) = setup_repository().await;

    let import_path = root.join("raw-import.jsonl");
    let import_content = concat!(
        "\r\n\u{feff}\r\n",
        "{ \"chat_metadata\" : { \"integrity\" : \"3c3f2f06-e625-52df-ba40-57e6600145ad\" } }\r\n",
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

    cleanup_repository(repository, root).await;
}

#[tokio::test]
async fn import_chat_payload_flattens_chub_jsonl_without_normalizing_messages() {
    let (repository, root) = setup_repository().await;

    let import_path = root.join("chub-import.jsonl");
    let import_content = [
        r#"{"chat_metadata":{"integrity":"a558cfca-42e3-5b78-b480-e7f8a253f52a"}}"#,
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

    let saved = repository
        .get_chat_payload("alice", &files[0])
        .await
        .expect("read Chub import");

    assert_eq!(saved.len(), 2);
    assert_eq!(saved[1].get("mes"), Some(&json!("hello")));
    assert_eq!(saved[1].pointer("/swipes/0"), Some(&json!("alt")));
    assert_eq!(saved[1].pointer("/swipes/1"), Some(&json!({"message": ""})));
    assert!(saved[1].get("name").is_none());
    assert!(saved[1].get("extra").is_none());

    cleanup_repository(repository, root).await;
}

#[tokio::test]
async fn rename_chat_preserves_bytes_and_rejects_invalid_or_occupied_targets() {
    let (repository, root) = setup_repository().await;
    let original = b"{\"custom_header\":{\"keep\":true}}\n{\"mes\":\"hello\"}";
    let occupied = b"{}\n{\"mes\":\"occupied\"}";
    commit_payload_bytes(
        &repository,
        character_target("alice", "session"),
        original,
        false,
    )
    .await
    .unwrap();
    commit_payload_bytes(
        &repository,
        character_target("alice", "occupied"),
        occupied,
        false,
    )
    .await
    .unwrap();

    for name in ["*.jsonl", "occupied"] {
        assert!(matches!(
            repository.rename_chat("alice", "session", name).await,
            Err(DomainError::InvalidData(_))
        ));
        assert_eq!(
            repository
                .get_chat_payload_bytes("alice", "session")
                .await
                .unwrap(),
            original
        );
        assert_eq!(
            repository
                .get_chat_payload_bytes("alice", "occupied")
                .await
                .unwrap(),
            occupied
        );
    }
    assert!(!root.join("chats/alice/chat.jsonl").exists());
    assert_eq!(
        repository
            .rename_chat("alice", "session", "renamed.jsonl")
            .await
            .unwrap(),
        "renamed"
    );
    assert_eq!(
        repository
            .get_chat_payload_bytes("alice", "renamed")
            .await
            .unwrap(),
        original
    );
    assert!(matches!(
        repository.get_chat_payload_bytes("alice", "session").await,
        Err(DomainError::NotFound(_))
    ));
    cleanup_repository(repository, root).await;
}

#[tokio::test]
async fn chat_directory_dates_survive_unavailable_previews_and_cache_reuse() {
    let (repository, root) = setup_repository().await;
    let older = payload_with_message(
        "eac4edce-753b-5b13-9579-0d1644e26a1c",
        "2026-01-01T00:00:00.000Z",
        "older",
        "alice",
    );
    // Repeated fields follow JSON's last-value rule; unavailable text must
    // not discard a valid date or make it change after the summary is cached.
    let newer = concat!(
        "{}\n",
        r#"{"mes":"discarded","mes":{},"send_date":"2026-01-02T00:00:00.000Z","send_date":"2026-01-03T00:00:00.000Z"}"#,
    );

    commit_payload_bytes(
        &repository,
        character_target("alice", "older"),
        payload_to_jsonl(&older).as_bytes(),
        false,
    )
    .await
    .expect("save older payload");
    commit_payload_bytes(
        &repository,
        character_target("alice", "newer"),
        newer.as_bytes(),
        false,
    )
    .await
    .expect("save newer payload");

    let (chat_size, date_last_chat) = repository
        .calculate_character_chat_stats("alice")
        .await
        .expect("calculate chat stats");

    assert!(chat_size > 0);
    assert_eq!(date_last_chat, timestamp_millis("2026-01-03T00:00:00.000Z"));

    let summaries = repository
        .list_chat_summaries(Some("alice"), false)
        .await
        .unwrap();
    let newer = summaries
        .iter()
        .find(|entry| entry.file_name == "newer.jsonl")
        .unwrap();
    assert_eq!(newer.preview, "Preview unavailable");
    assert_eq!(newer.date, date_last_chat);
    assert_eq!(
        repository
            .calculate_character_chat_stats("alice")
            .await
            .unwrap()
            .1,
        date_last_chat
    );

    cleanup_repository(repository, root).await;
}

#[tokio::test]
async fn damaged_directory_queries_refresh_after_repair() {
    let (repository, root) = setup_repository().await;
    let chat_dir = root.join("chats/alice");
    fs::create_dir_all(&chat_dir).await.unwrap();
    let path = chat_dir.join("damaged.jsonl");
    fs::write(&path, "{}\n{\"mes\":\"earlier\"}\n{\"mes\":")
        .await
        .unwrap();
    let modified =
        FileChatRepository::file_signature_from_metadata(&fs::metadata(&path).await.unwrap())
            .modified_millis;

    // Cold recents must keep a file whose final record cannot supply a preview.
    let recent = repository
        .list_recent_chat_summaries(Some("alice"), false, 10, &[])
        .await
        .unwrap();
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].preview, "Preview unavailable");
    assert_eq!(recent[0].message_count, 2);
    assert_eq!(recent[0].date, modified);

    // Both a matching result and an empty result must notice external repair.
    for (query, count) in [("damaged", 1), ("repaired", 0)] {
        assert_eq!(
            repository
                .search_chats(query, Some("alice"))
                .await
                .unwrap()
                .len(),
            count
        );
    }
    // A different byte length changes the file signature without timing assumptions.
    fs::write(
        &path,
        "{}\n{\"mes\":\"earlier\"}\n{\"mes\":\"repaired message\"}",
    )
    .await
    .unwrap();
    for query in ["damaged", "repaired"] {
        let results = repository.search_chats(query, Some("alice")).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].preview, "repaired message");
    }
    cleanup_repository(repository, root).await;
}

#[tokio::test]
async fn finds_remaining_character_chat_with_integrity() {
    let (repository, root) = setup_repository().await;
    for name in ["first", "second"] {
        commit_payload_bytes(
            &repository,
            character_target("alice", name),
            payload_to_jsonl(&payload_with_integrity(" shared identity ")).as_bytes(),
            false,
        )
        .await
        .expect("save shared chat");
    }

    repository
        .delete_chat("alice", "first")
        .await
        .expect("delete first chat");
    assert!(
        repository
            .has_character_chat_with_integrity("alice", " shared identity ")
            .await
            .expect("find remaining chat")
    );
    repository
        .delete_chat("alice", "second")
        .await
        .expect("delete second chat");
    assert!(
        !repository
            .has_character_chat_with_integrity("alice", " shared identity ")
            .await
            .expect("find no remaining chat")
    );

    cleanup_repository(repository, root).await;
}

#[tokio::test]
async fn summary_index_write_failure_does_not_change_query_or_delete_outcome() {
    let (repository, root) = setup_repository().await;
    commit_payload_bytes(
        &repository,
        character_target("alice", "session"),
        payload_to_jsonl(&payload_with_integrity(
            "9f263ea6-8826-5eaa-a035-da50568229ce",
        ))
        .as_bytes(),
        false,
    )
    .await
    .expect("save chat payload");

    let cache_parent = root.join("user").join("cache");
    fs::create_dir_all(cache_parent.parent().expect("cache parent"))
        .await
        .expect("create user directory");
    fs::write(&cache_parent, b"not a directory")
        .await
        .expect("block summary index directory");

    let summaries = repository
        .list_chat_summaries(Some("alice"), false)
        .await
        .expect("cache persistence must not block summary query");
    assert_eq!(summaries.len(), 1);

    repository
        .delete_chat("alice", "session")
        .await
        .expect("cache persistence must not reverse committed deletion");
    assert!(!root.join("chats/alice/session.jsonl").exists());

    cleanup_repository(repository, root).await;
}

#[tokio::test]
async fn aggregate_chat_queries_skip_an_invalid_sibling() {
    let (repository, root) = setup_repository().await;
    commit_payload_bytes(
        &repository,
        character_target("alice", "valid"),
        payload_to_jsonl(&payload_with_message(
            "133cbadf-2d0c-5f69-aa32-ab9c9f757080",
            "2026-01-01T00:00:00.000Z",
            "search needle",
            "Alice",
        ))
        .as_bytes(),
        false,
    )
    .await
    .expect("save valid chat payload");
    fs::write(
        root.join("chats").join("alice").join("invalid.jsonl"),
        [0xff, b'\n'],
    )
    .await
    .expect("write invalid sibling chat");

    let summaries = repository
        .list_chat_summaries(Some("alice"), false)
        .await
        .expect("list valid summaries");
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].file_name, "valid.jsonl");

    let recent = repository
        .list_recent_chat_summaries(Some("alice"), false, 10, &[])
        .await
        .expect("list valid recent summaries");
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].file_name, "valid.jsonl");

    let search = repository
        .search_chats("needle", Some("alice"))
        .await
        .expect("search valid chats");
    assert_eq!(search.len(), 1);
    assert_eq!(search[0].file_name, "valid.jsonl");

    cleanup_repository(repository, root).await;
}

#[tokio::test]
async fn search_character_chat_messages_returns_scored_hits_and_respects_role_filter() {
    let (repository, root) = setup_repository().await;

    let payload = vec![
        json!({
            "chat_metadata": {
                "integrity": "ebb522b7-d6f9-5855-87fc-d676dfefe8ee",
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

    commit_payload_bytes(
        &repository,
        character_target("alice", "session"),
        payload_to_jsonl(&payload).as_bytes(),
        false,
    )
    .await
    .expect("save payload");

    let hits = repository
        .search_character_chat_messages(
            "alice",
            "session",
            ChatMessageSearchQuery {
                frozen_macros: None,
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
                frozen_macros: None,
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

    cleanup_repository(repository, root).await;
}

#[tokio::test]
async fn read_character_chat_messages_returns_selected_messages_and_total_count() {
    let (repository, root) = setup_repository().await;

    let payload = [
        json!({
            "chat_metadata": {
                "integrity": "695e7861-f59b-519d-8712-4a40f8bef7a5",
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

    let raw = format!(
        "\r\n\u{feff}\r\n{}",
        payload
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\r\n\r\n")
    );
    commit_payload_bytes(
        &repository,
        character_target("alice", "session"),
        raw.as_bytes(),
        false,
    )
    .await
    .unwrap();

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

    cleanup_repository(repository, root).await;
}

#[tokio::test]
async fn tool_role_roundtrips_and_is_distinct_from_system() {
    let (repository, root) = setup_repository().await;
    let payload = vec![
        json!({
            "chat_metadata": { "integrity": "fd0b8038-8ff6-54b9-95f7-e451f488b239" },
            "user_name": "User",
            "character_name": "Alice",
        }),
        json!({
            "name": "Alice",
            "is_user": false,
            "is_system": false,
            "send_date": "2026-01-01T00:00:00.000Z",
            "mes": "",
            "tool_calls": [{
                "id": "call_weather",
                "name": "lookup_weather",
                "parameters": "{\"city\":\"Paris\"}",
            }],
            "extra": {},
        }),
        json!({
            "role": "tool",
            "name": "lookup_weather",
            "is_user": false,
            "is_system": true,
            "send_date": "2026-01-01T00:00:01.000Z",
            "mes": "weather result: sunny",
            "tool_call_id": "call_weather",
            "error": false,
            "extra": {},
        }),
    ];

    commit_payload_bytes(
        &repository,
        character_target("alice", "session"),
        payload_to_jsonl(&payload).as_bytes(),
        false,
    )
    .await
    .expect("save payload");

    let chat = repository
        .get_chat("alice", "session")
        .await
        .expect("load typed chat");
    let tool_message = &chat.messages[1];
    assert_eq!(
        tool_message.additional.get("role").and_then(Value::as_str),
        Some("tool")
    );
    assert_eq!(
        tool_message
            .additional
            .get("tool_call_id")
            .and_then(Value::as_str),
        Some("call_weather")
    );

    repository
        .save_with_options(&chat, true)
        .await
        .expect("rewrite typed chat");
    let rewritten = repository
        .get_chat_payload("alice", "session")
        .await
        .expect("read rewritten payload");
    assert_eq!(rewritten[2]["role"], json!("tool"));
    assert_eq!(rewritten[2]["tool_call_id"], json!("call_weather"));
    assert_eq!(rewritten[1]["tool_calls"][0]["id"], json!("call_weather"));

    let read = repository
        .read_character_chat_messages("alice", "session", &[1])
        .await
        .expect("read tool message");
    assert_eq!(read.messages[0].role, ChatMessageRole::Tool);

    let tool_hits = repository
        .search_character_chat_messages(
            "alice",
            "session",
            ChatMessageSearchQuery {
                frozen_macros: None,
                query: "weather result".to_string(),
                limit: 10,
                filters: Some(ChatMessageSearchFilters {
                    role: Some(ChatMessageRole::Tool),
                    start_index: None,
                    end_index: None,
                    scan_limit: None,
                }),
            },
        )
        .await
        .expect("search tool messages");
    assert_eq!(tool_hits.len(), 1);
    assert_eq!(tool_hits[0].index, 1);
    assert_eq!(tool_hits[0].role, ChatMessageRole::Tool);

    let system_hits = repository
        .search_character_chat_messages(
            "alice",
            "session",
            ChatMessageSearchQuery {
                frozen_macros: None,
                query: "weather result".to_string(),
                limit: 10,
                filters: Some(ChatMessageSearchFilters {
                    role: Some(ChatMessageRole::System),
                    start_index: None,
                    end_index: None,
                    scan_limit: None,
                }),
            },
        )
        .await
        .expect("search system messages");
    assert!(system_hits.is_empty());

    let located = repository
        .find_last_character_chat_message(
            "alice",
            "session",
            FindLastMessageQuery {
                role: Some(ChatMessageRole::Tool),
                has_top_level_keys: None,
                has_extra_keys: None,
                scan_limit: None,
            },
        )
        .await
        .expect("locate tool message")
        .expect("tool message should exist");
    assert_eq!(located.index, 1);

    let system = repository
        .find_last_character_chat_message(
            "alice",
            "session",
            FindLastMessageQuery {
                role: Some(ChatMessageRole::System),
                has_top_level_keys: None,
                has_extra_keys: None,
                scan_limit: None,
            },
        )
        .await
        .expect("locate system message");
    assert!(system.is_none());

    cleanup_repository(repository, root).await;
}

#[tokio::test]
async fn search_group_chat_messages_respects_scan_limit() {
    let (repository, root) = setup_repository().await;

    let payload = vec![
        json!({
            "chat_metadata": {
                "integrity": "b4bbd40a-46b0-5a1d-a73f-5cb96857417d",
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

    commit_payload_bytes(
        &repository,
        ChatPayloadTarget::Group {
            chat_id: "group-one".into(),
        },
        payload_to_jsonl(&payload).as_bytes(),
        false,
    )
    .await
    .expect("save group payload");

    let limited = repository
        .search_group_chat_messages(
            "group-one",
            ChatMessageSearchQuery {
                frozen_macros: None,
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
                frozen_macros: None,
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

    cleanup_repository(repository, root).await;
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

    let chat_dir = root.join("chats/alice");
    fs::create_dir_all(&chat_dir).await.unwrap();
    for (name, date, modified) in [
        ("session-newer-date.jsonl", "2026-01-03T00:00:00Z", 1),
        ("session-newer-mtime.jsonl", "2026-01-01T00:00:00Z", 2),
    ] {
        let path = chat_dir.join(name);
        fs::write(
            &path,
            format!("{{}}\n{}", json!({"send_date": date, "mes": name})),
        )
        .await
        .unwrap();
        set_backup_modified(&path, UNIX_EPOCH + Duration::from_secs(modified))
            .await
            .unwrap();
    }

    let results = repository
        .list_recent_chat_summaries(None, false, 1, &[])
        .await
        .expect("list recent summaries");

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].file_name, "session-newer-date.jsonl");

    cleanup_repository(repository, root).await;
}

#[tokio::test]
async fn character_chat_store_merges_replaces_upserts_and_renames_entries() {
    let (repository, root) = setup_repository().await;

    commit_payload_bytes(
        &repository,
        character_target("alice", "session"),
        payload_to_jsonl(&payload_with_integrity(" legacy-聊天")).as_bytes(),
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
    assert!(
        root.join("chats/alice/.tauritavern/ legacy-聊天/my-ext/index.json")
            .exists()
    );

    let path = repository
        .get_chat_payload_path("alice", "session")
        .await
        .unwrap();
    let bytes = fs::read(&path).await.unwrap();
    fs::write(&path, ["\r\n\u{feff}\r\n".as_bytes(), &bytes].concat())
        .await
        .unwrap();

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

    repository
        .rename_character_chat_store_key("alice", "session", "my-ext", "missing", "renamed")
        .await
        .unwrap();
    assert!(matches!(
        repository
            .get_character_chat_store_json("alice", "session", "my-ext", "missing")
            .await,
        Err(DomainError::NotFound(_))
    ));
    assert_eq!(
        repository
            .list_character_chat_store_keys("alice", "session", "my-ext")
            .await
            .unwrap(),
        ["index", "renamed"]
    );
    let created = repository
        .get_character_chat_store_json("alice", "session", "my-ext", "renamed")
        .await
        .expect("read created store json");
    assert_eq!(created, json!({ "created": true }));

    commit_payload_bytes(
        &repository,
        character_target("alice", "unsafe"),
        br#"{"chat_metadata":{"integrity":"../outside"}}"#,
        false,
    )
    .await
    .unwrap();
    assert!(matches!(
        repository
            .set_character_chat_store_json("alice", "unsafe", "my-ext", "index", json!({}))
            .await,
        Err(DomainError::InvalidData(_))
    ));
    assert!(!root.join("chats/alice/outside").exists());

    cleanup_repository(repository, root).await;
}

#[tokio::test]
async fn group_chat_store_update_json_and_key_work() {
    let (repository, root) = setup_repository().await;

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

    cleanup_repository(repository, root).await;
}

#[tokio::test]
async fn chat_payload_paging_returns_tail_and_before_for_character_and_group() {
    let (repository, root) = setup_repository().await;
    let mut payload =
        vec![payload_with_integrity("a7f82e6b-7dbd-51f7-8e4a-57966e2a3071")[0].clone()];
    for index in 0..4 {
        payload.push(json!({ "mes": format!("message {index}") }));
    }

    let blank_gap = " \t\r\n".repeat(20_000);
    let raw = format!(
        "\r\n\u{feff}\r\n{}",
        payload
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join(&blank_gap)
    );
    for target in [
        character_target("alice", "session"),
        ChatPayloadTarget::Group {
            chat_id: "group-session".into(),
        },
    ] {
        commit_payload_bytes(&repository, target, raw.as_bytes(), false)
            .await
            .unwrap();
    }

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
    commit_payload_bytes(
        &repository,
        character_target("alice", "session"),
        payload_to_jsonl(&payload).as_bytes(),
        false,
    )
    .await
    .expect("replace character payload");
    let stale = repository
        .get_chat_payload_before_lines("alice", "session", stale_cursor, 1)
        .await;
    assert!(stale.is_err(), "stale paging cursor must be rejected");

    cleanup_repository(repository, root).await;
}

fn payload_to_jsonl(payload: &[Value]) -> String {
    payload
        .iter()
        .map(|item| serde_json::to_string(item).expect("serialize line"))
        .collect::<Vec<_>>()
        .join("\n")
}

async fn read_chat_stream_bytes(
    mut reader: Box<dyn tt_ports::repositories::chat_repository::ChatByteReader>,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut buffer = [0; 37];
    loop {
        let n = reader.read(&mut buffer).await.expect("read chat stream");
        if n == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..n]);
    }
    bytes
}

async fn read_chat_stream(
    reader: Box<dyn tt_ports::repositories::chat_repository::ChatByteReader>,
) -> Vec<Value> {
    String::from_utf8(read_chat_stream_bytes(reader).await)
        .unwrap()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn swipe_fixture() -> Vec<Value> {
    vec![
        json!({"chat_metadata":{"integrity":"38aedc23-2370-574f-8194-ff3a25bdfad3"},"user_name":"User"}),
        json!({"mes":"current body", "is_user":false, "swipe_id":2,
            "swipes":["历史零", "history one", "active two"],
            "swipe_info":[{"extra":{"tauritavern":{"agent":{"persistStateId":"old"}}}}, {"extra":{}}, {"extra":{"live":true}}],
            "variables":[{"v":0},{"v":1},{"v":2}], "escaped\"key":{"unchanged":true}, "extra":{"live":"body"}}),
        json!({"mes":"user", "is_user":true, "swipe_id":0,"swipes":["user","alternate"],"swipe_info":[{},{}]}),
        json!({"mes":"irregular", "swipe_id":0,"swipes":["irregular","alternate"],"swipe_info":[{}]}),
        json!({"mes":"tail", "swipe_id":1,"swipes":["tail old","tail"],"swipe_info":[{},{}]}),
    ]
}

#[tokio::test]
async fn cold_swipes_round_trip_retains_source_across_metadata_publish_reorder_and_copy() {
    use tt_ports::repositories::chat_payload_commit_repository::ColdSwipeCommitSource;
    for target in [
        character_target("Alice", "cold"),
        ChatPayloadTarget::Group {
            chat_id: "cold".into(),
        },
    ] {
        let (repository, root) = setup_repository().await;
        let mut original = swipe_fixture();
        original[0]["tt_swipe_cold"] = json!({ "opaque_header_field": true });
        let input = format!(
            "\r\n\u{feff}\r\n{}\r\n \t\r\n",
            original
                .iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\r\n\r\n")
        );
        commit_payload_bytes(&repository, target.clone(), input.as_bytes(), false)
            .await
            .unwrap();
        let source = repository.open_swipe_source(target.clone()).await.unwrap();
        let projected = read_chat_stream(source.clone().projection(7)).await;
        assert_eq!(projected[1]["swipes"], json!([null, null, "active two"]));
        assert_eq!(
            projected[1]["tt_swipe_cold"],
            json!({"sourceId":7,"record":1})
        );
        assert_eq!(projected[1]["variables"], original[1]["variables"]);
        assert_eq!(projected[2]["swipes"], json!(["user", null]));
        assert_eq!(&projected[3..], &original[3..]);

        repository
            .commit_metadata(
                target.clone(),
                json!({"integrity":"38aedc23-2370-574f-8194-ff3a25bdfad3","note":"longer metadata"}),
            )
            .await
            .unwrap();
        let mut staged = vec![
            projected[0].clone(),
            projected[2].clone(),
            projected[1].clone(),
            projected[1].clone(),
        ];
        staged[2]["mes"] = json!("edited body");
        staged[2]["swipes"][2] = json!("edited active swipe");
        staged[2]["swipe_info"][2] = json!({"extra":{"edited":true}});
        staged[2]["variables"][2] = json!({"v":99});
        staged[2]["escaped\"key"] = json!({"edited":true});
        staged[2]["swipes"][0] = json!("edited historical slot");
        staged[2]["swipe_info"][1] = json!({"extra":{"historicalEdit":true}});
        staged[2]["swipes"]
            .as_array_mut()
            .unwrap()
            .extend([json!("appended one"), json!("appended two")]);
        staged[2]["swipe_info"]
            .as_array_mut()
            .unwrap()
            .extend([json!({}), json!({"extra":{"appended":true}})]);
        staged[2]["swipe_id"] = json!(4);
        staged[3]["swipe_id"] = json!(0);
        let bytes = payload_to_jsonl(&staged).into_bytes();
        let session = repository
            .begin(
                target.clone(),
                false,
                Some(ColdSwipeCommitSource {
                    id: 7,
                    source: source.clone(),
                }),
            )
            .await
            .unwrap();
        // A started commit and an opened record remain valid after the page releases its source owner.
        let record = source.clone().record(1).await.unwrap();
        drop(source);
        repository
            .append(&session.session_id, 0, &bytes)
            .await
            .unwrap();
        let committed = repository
            .finish(&session.session_id, bytes.len() as u64)
            .await
            .unwrap();
        assert_eq!(committed.accepted_size, bytes.len() as u64);
        assert_eq!(read_chat_stream(record).await, vec![original[1].clone()]);
        let path = repository
            .resolve_chat_commit_target(&target)
            .await
            .unwrap();
        let saved = fs::read(&path).await.unwrap();
        assert_eq!(committed.size, saved.len() as u64);
        let restored: Vec<Value> = std::str::from_utf8(&saved)
            .unwrap()
            .lines()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect();
        let mut expected = staged;
        expected[1] = original[2].clone();
        expected[2]["swipes"] = json!([
            "edited historical slot",
            "history one",
            "edited active swipe",
            "appended one",
            "appended two"
        ]);
        expected[2]["swipe_info"][0] = original[1]["swipe_info"][0].clone();
        expected[2].as_object_mut().unwrap().remove("tt_swipe_cold");
        expected[3] = original[1].clone();
        expected[3]["swipe_id"] = json!(0);
        assert_eq!(restored, expected);
        if let ChatPayloadTarget::Character {
            character_id,
            file_name,
        } = &target
        {
            apply_and_reconcile_backups(&repository, backup_policy(-1, -1, -1)).await;
            repository
                .backup_chat_automatic(character_id, file_name)
                .await
                .unwrap();
            let backups = backup_file_names(&root).await;
            // A full commit of the restored bytes must remain the same backup state.
            commit_payload_bytes(&repository, target.clone(), &saved, false)
                .await
                .unwrap();
            repository
                .backup_chat_automatic(character_id, file_name)
                .await
                .unwrap();
            assert_eq!(backup_file_names(&root).await, backups);
        }

        cleanup_repository(repository, root).await;
    }
}

#[tokio::test]
async fn cold_swipes_reject_invalid_merges_without_publishing_or_leaving_stages() {
    use tt_ports::repositories::chat_payload_commit_repository::ColdSwipeCommitSource;
    let (repository, root) = setup_repository().await;
    let target = ChatPayloadTarget::Group {
        chat_id: "reject-cold".into(),
    };
    let original = payload_to_jsonl(&swipe_fixture());
    commit_payload_bytes(&repository, target.clone(), original.as_bytes(), false)
        .await
        .unwrap();
    let source = repository.open_swipe_source(target.clone()).await.unwrap();
    let projected = read_chat_stream(source.clone().projection(8)).await;
    for mutation in ["shortened", "info", "record", "source", "index"] {
        let mut staged = projected.clone();
        match mutation {
            "shortened" => {
                staged[1]["swipes"].as_array_mut().unwrap().pop();
                staged[1]["swipe_info"].as_array_mut().unwrap().pop();
                staged[1]["swipe_id"] = json!(1);
            }
            "info" => {
                staged[1]["swipe_info"].as_array_mut().unwrap().pop();
            }
            "record" => staged[1]["tt_swipe_cold"]["record"] = json!(99),
            "source" => staged[1]["tt_swipe_cold"]["sourceId"] = json!(99),
            "index" => staged[1]["swipe_id"] = json!(99),
            _ => unreachable!(),
        }
        let bytes = payload_to_jsonl(&staged);
        let session = repository
            .begin(
                target.clone(),
                true,
                Some(ColdSwipeCommitSource {
                    id: 8,
                    source: source.clone(),
                }),
            )
            .await
            .unwrap();
        repository
            .append(&session.session_id, 0, bytes.as_bytes())
            .await
            .unwrap();
        assert!(
            repository
                .finish(&session.session_id, bytes.len() as u64)
                .await
                .is_err()
        );
        let path = repository
            .resolve_chat_commit_target(&target)
            .await
            .unwrap();
        assert_eq!(fs::read(&path).await.unwrap(), original.as_bytes());
        assert!(
            fs::read_dir(&repository.chat_commit_staging_dir)
                .await
                .unwrap()
                .next_entry()
                .await
                .unwrap()
                .is_none()
        );
    }
    drop(source);
    cleanup_repository(repository, root).await;
}

#[tokio::test]
async fn cold_swipes_lookahead_handles_empty_header_only_and_unterminated_tail() {
    let (repository, root) = setup_repository().await;
    let target = ChatPayloadTarget::Group {
        chat_id: "boundaries".into(),
    };
    let path = repository
        .resolve_chat_commit_target(&target)
        .await
        .unwrap();
    let fixture = swipe_fixture();
    for messages in [
        vec![],
        vec![fixture[0].clone()],
        vec![fixture[0].clone(), fixture[1].clone()],
        fixture,
    ] {
        for (prefix, suffix) in [("", ""), ("\r\n\u{feff}", "\n \t\n")] {
            fs::write(
                &path,
                format!("{}{}{}", prefix, payload_to_jsonl(&messages), suffix),
            )
            .await
            .unwrap();
            let source = repository.open_swipe_source(target.clone()).await.unwrap();
            let projected = read_chat_stream(source.clone().projection(9)).await;
            assert_eq!(projected.len(), messages.len());
            assert_eq!(projected.last(), messages.last());
            drop(source);
        }
    }
    cleanup_repository(repository, root).await;
}

#[tokio::test]
async fn cold_swipes_preserve_record_key_order_and_project_all_message_roles() {
    use tt_ports::repositories::chat_payload_commit_repository::ColdSwipeCommitSource;
    let (repository, root) = setup_repository().await;
    let target = ChatPayloadTarget::Group {
        chat_id: "ordered-cold".into(),
    };
    let message = r#"{"z-extension":{"z":1,"a":2},"mes":"active","swipe_id":1,"swipes":["old","active"],"swipe_info":[{},{}],"a-extension":true}"#;
    let records: Vec<_> = [
        r#""is_user":true"#,
        r#""role":"tool","tool_calls":[]"#,
        r#""extra":{"isSmallSys":true}"#,
    ]
    .into_iter()
    .map(|fields| format!("{{{fields},{}", &message[1..]))
    .collect();
    let input = format!("{{}}\n{}\n{}", records.join("\n"), message);
    commit_payload_bytes(&repository, target.clone(), input.as_bytes(), false)
        .await
        .unwrap();
    let source = repository.open_swipe_source(target.clone()).await.unwrap();
    let projected = read_chat_stream_bytes(source.clone().projection(0)).await;
    let projected_text = std::str::from_utf8(&projected).unwrap();
    for (line, original) in projected_text.lines().skip(1).zip(&records) {
        let fields: indexmap::IndexMap<String, Value> = serde_json::from_str(line).unwrap();
        let original: indexmap::IndexMap<String, Value> = serde_json::from_str(original).unwrap();
        assert_eq!(fields["swipes"], json!([null, "active"]));
        assert_eq!(
            fields.keys().take(original.len()).collect::<Vec<_>>(),
            original.keys().collect::<Vec<_>>()
        );
    }
    let session = repository
        .begin(
            target.clone(),
            false,
            Some(ColdSwipeCommitSource { id: 0, source }),
        )
        .await
        .unwrap();
    repository
        .append(&session.session_id, 0, &projected)
        .await
        .unwrap();
    repository
        .finish(&session.session_id, projected.len() as u64)
        .await
        .unwrap();
    let path = repository
        .resolve_chat_commit_target(&target)
        .await
        .unwrap();
    assert_eq!(
        fs::read_to_string(path).await.unwrap(),
        format!("{input}\n")
    );
    cleanup_repository(repository, root).await;
}
