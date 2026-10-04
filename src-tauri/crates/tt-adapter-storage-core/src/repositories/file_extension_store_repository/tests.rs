use super::*;
use serde_json::{Value, json};

struct Store {
    root: PathBuf,
    repository: FileExtensionStoreRepository,
}

impl Store {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("extension-store-{}", uuid::Uuid::new_v4()));
        let repository = FileExtensionStoreRepository::new(
            root.join("entries"),
            root.join(".staging"),
            Arc::default(),
            4 * 1024 * 1024,
        );
        Self { root, repository }
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

async fn stage(
    repo: &FileExtensionStoreRepository,
    key: &str,
    operation: WriteOperation,
    bytes: &[u8],
) -> String {
    let session = repo
        .begin_commit("example", "main", key, operation)
        .await
        .unwrap();
    if !bytes.is_empty() {
        repo.append_commit(&session.session_id, 0, bytes)
            .await
            .unwrap();
    }
    session.session_id
}

async fn write(
    repo: &FileExtensionStoreRepository,
    key: &str,
    operation: WriteOperation,
    bytes: &[u8],
) {
    let id = stage(repo, key, operation, bytes).await;
    repo.finish_commit(&id, bytes.len() as u64).await.unwrap();
}

async fn read(mut reader: Box<dyn ByteReader>) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut buffer = [0; 7];
    loop {
        let count = reader.read(&mut buffer).await.unwrap();
        if count == 0 {
            return bytes;
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
}

async fn read_json(repo: &FileExtensionStoreRepository, key: &str) -> Value {
    let reader = repo
        .open_entry("example", "main", key, EntryKind::Json)
        .await
        .unwrap()
        .unwrap();
    serde_json::from_slice(&read(reader).await).unwrap()
}

#[tokio::test]
async fn json_commits_merge_values_and_keep_failed_replacements_private() {
    let store = Store::new();
    let repo = &store.repository;
    let original = br#"{"nested":{"keep":true},"array":[1]}"#;
    write(repo, "settings", WriteOperation::SetJson, original).await;

    let patch = br#"{"nested":{"added":null},"array":[false]}"#;
    write(repo, "settings", WriteOperation::UpdateJson, patch).await;
    let expected = json!({
        "nested": { "keep": true, "added": null }, "array": [false],
    });
    assert_eq!(read_json(repo, "settings").await, expected);

    for operation in [WriteOperation::SetJson, WriteOperation::UpdateJson] {
        let id = stage(repo, "settings", operation, b"{").await;
        let error = repo.finish_commit(&id, 1).await.unwrap_err().to_string();
        let target = store
            .root
            .join("entries")
            .join("example")
            .join("kv")
            .join("main")
            .join("settings.json");
        assert!(error.contains(&target.display().to_string()), "{error}");
        assert_eq!(read_json(repo, "settings").await, expected);
    }
}

#[tokio::test]
async fn opened_readers_keep_their_versions_across_replacement_and_deletion() {
    let store = Store::new();
    let repo = &store.repository;
    let bytes = [0, 255, 1, 128, 3, 0, 4, 5, 6];
    write(repo, "data.bin", WriteOperation::SetBlob, &bytes).await;
    let old = repo
        .open_entry("example", "main", "data.bin", EntryKind::Blob)
        .await
        .unwrap()
        .unwrap();
    write(repo, "data.bin", WriteOperation::SetBlob, b"replacement").await;
    let updated = repo
        .open_entry("example", "main", "data.bin", EntryKind::Blob)
        .await
        .unwrap()
        .unwrap();
    repo.delete_entry("example", "main", "data.bin", EntryKind::Blob)
        .await
        .unwrap();
    assert_eq!(read(old).await, bytes);
    assert_eq!(read(updated).await, b"replacement");
}

#[tokio::test]
async fn enumeration_returns_only_addressable_keys_and_tables() {
    let store = Store::new();
    let repo = &store.repository;
    write(repo, "record", WriteOperation::SetJson, b"null").await;
    write(repo, "data.bin", WriteOperation::SetBlob, b"bytes").await;
    for directory in ["kv", "blobs"] {
        let directory = store.root.join("entries/example").join(directory);
        for name in [".hidden.json", " spaced.json"] {
            fs::write(directory.join("main").join(name), b"null")
                .await
                .unwrap();
        }
        fs::create_dir(directory.join(" invalid-table "))
            .await
            .unwrap();
    }
    assert_eq!(repo.list_tables("example").await.unwrap(), ["main"]);
    assert_eq!(
        repo.list_keys("example", "main", EntryKind::Json)
            .await
            .unwrap(),
        ["record"]
    );
    assert_eq!(
        repo.list_keys("example", "main", EntryKind::Blob)
            .await
            .unwrap(),
        ["data.bin"]
    );
}
