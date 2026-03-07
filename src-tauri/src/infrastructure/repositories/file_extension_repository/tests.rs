use std::path::PathBuf;

use rand::random;
use serde_json::json;
use tokio::fs;

use crate::domain::repositories::extension_repository::ExtensionRepository;

use super::FileExtensionRepository;

fn unique_temp_root() -> PathBuf {
    std::env::temp_dir().join(format!("tauritavern-extension-repo-{}", random::<u64>()))
}

async fn setup_paths() -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let root = unique_temp_root();
    let user_extensions_dir = root.join("default-user").join("extensions");
    let global_extensions_dir = root.join("extensions").join("third-party");
    let source_store_root = root.join("_tauritavern").join("extension-sources");

    fs::create_dir_all(&user_extensions_dir)
        .await
        .expect("create local extensions dir");
    fs::create_dir_all(&global_extensions_dir)
        .await
        .expect("create global extensions dir");
    fs::create_dir_all(source_store_root.join("local"))
        .await
        .expect("create local source state dir");
    fs::create_dir_all(source_store_root.join("global"))
        .await
        .expect("create global source state dir");

    (
        root,
        user_extensions_dir,
        global_extensions_dir,
        source_store_root,
    )
}

fn legacy_source_metadata() -> serde_json::Value {
    json!({
        "owner": "N0VI028",
        "repo": "JS-Slash-Runner",
        "reference": "main",
        "remote_url": "https://github.com/N0VI028/JS-Slash-Runner",
        "installed_commit": "abcdef1234567890"
    })
}

#[tokio::test]
async fn startup_migration_moves_legacy_source_state_into_new_store() {
    let (root, user_extensions_dir, global_extensions_dir, source_store_root) = setup_paths().await;
    let extension_dir = user_extensions_dir.join("legacy-ext");
    fs::create_dir_all(&extension_dir)
        .await
        .expect("create extension dir");
    fs::write(
        extension_dir.join(".tauritavern-source.json"),
        serde_json::to_vec_pretty(&legacy_source_metadata()).expect("serialize legacy source"),
    )
    .await
    .expect("write legacy source state");

    let repository = FileExtensionRepository::new(
        user_extensions_dir.clone(),
        global_extensions_dir,
        source_store_root.clone(),
    )
    .expect("create extension repository");

    let migrated_path = source_store_root.join("local").join("legacy-ext.json");
    assert!(migrated_path.exists(), "migrated state file should exist");
    assert!(
        !extension_dir.join(".tauritavern-source.json").exists(),
        "legacy state file should be deleted after migration"
    );

    let extensions = repository
        .discover_extensions()
        .await
        .expect("discover extensions");
    let extension = extensions
        .into_iter()
        .find(|extension| extension.name == "third-party/legacy-ext")
        .expect("migrated extension should be discoverable");
    assert_eq!(
        extension.remote_url.as_deref(),
        Some("https://github.com/N0VI028/JS-Slash-Runner")
    );

    fs::remove_dir_all(root).await.expect("cleanup temp root");
}

#[tokio::test]
async fn startup_migration_rebuilds_missing_source_state_from_git_dir() {
    let (root, user_extensions_dir, global_extensions_dir, source_store_root) = setup_paths().await;
    let extension_dir = user_extensions_dir.join("git-ext");
    fs::create_dir_all(extension_dir.join(".git").join("refs").join("heads"))
        .await
        .expect("create git refs directory");

    let config = r#"[remote "origin"]
    url = git@github.com:N0VI028/JS-Slash-Runner.git
"#;
    fs::write(extension_dir.join(".git").join("config"), config)
        .await
        .expect("write git config");

    let commit = "abcdef1234567890abcdef1234567890abcdef12\n";
    fs::write(
        extension_dir.join(".git").join("HEAD"),
        "ref: refs/heads/main\n",
    )
    .await
    .expect("write git HEAD");
    fs::write(
        extension_dir
            .join(".git")
            .join("refs")
            .join("heads")
            .join("main"),
        commit,
    )
    .await
    .expect("write git ref commit");

    let repository = FileExtensionRepository::new(
        user_extensions_dir.clone(),
        global_extensions_dir,
        source_store_root.clone(),
    )
    .expect("create extension repository");

    assert!(
        source_store_root
            .join("local")
            .join("git-ext.json")
            .exists(),
        "recovered state file should exist"
    );

    let extensions = repository
        .discover_extensions()
        .await
        .expect("discover extensions");
    let extension = extensions
        .into_iter()
        .find(|extension| extension.name == "third-party/git-ext")
        .expect("git extension should be discoverable");
    assert_eq!(
        extension.remote_url.as_deref(),
        Some("https://github.com/N0VI028/JS-Slash-Runner")
    );

    fs::remove_dir_all(root).await.expect("cleanup temp root");
}

#[tokio::test]
async fn startup_migration_rebuilds_missing_source_state_from_gitfile_commondir_layout() {
    let (root, user_extensions_dir, global_extensions_dir, source_store_root) = setup_paths().await;
    let extension_dir = user_extensions_dir.join("gitfile-ext");
    fs::create_dir_all(&extension_dir)
        .await
        .expect("create extension dir");

    fs::write(extension_dir.join(".git"), "gitdir: .git-worktree\n")
        .await
        .expect("write gitdir file");

    let worktree_dir = extension_dir.join(".git-worktree");
    let common_dir = extension_dir.join(".git-common");
    fs::create_dir_all(worktree_dir.join("refs").join("heads"))
        .await
        .expect("create worktree refs directory");
    fs::create_dir_all(common_dir.join("refs").join("heads"))
        .await
        .expect("create common refs directory");

    fs::write(worktree_dir.join("HEAD"), "ref: refs/heads/main\n")
        .await
        .expect("write worktree HEAD");
    fs::write(worktree_dir.join("commondir"), "../.git-common\n")
        .await
        .expect("write commondir");

    let config = r#"[remote "origin"]
    url = https://github.com/N0VI028/JS-Slash-Runner.git
"#;
    fs::write(common_dir.join("config"), config)
        .await
        .expect("write common git config");

    let commit = "abcdef1234567890abcdef1234567890abcdef12\n";
    fs::write(common_dir.join("refs").join("heads").join("main"), commit)
        .await
        .expect("write common git ref commit");

    let repository = FileExtensionRepository::new(
        user_extensions_dir.clone(),
        global_extensions_dir,
        source_store_root.clone(),
    )
    .expect("create extension repository");

    assert!(
        source_store_root
            .join("local")
            .join("gitfile-ext.json")
            .exists(),
        "recovered state file should exist"
    );

    let extensions = repository
        .discover_extensions()
        .await
        .expect("discover extensions");
    let extension = extensions
        .into_iter()
        .find(|extension| extension.name == "third-party/gitfile-ext")
        .expect("gitfile extension should be discoverable");
    assert_eq!(
        extension.remote_url.as_deref(),
        Some("https://github.com/N0VI028/JS-Slash-Runner")
    );

    fs::remove_dir_all(root).await.expect("cleanup temp root");
}

#[tokio::test]
async fn move_extension_moves_source_state_between_scopes() {
    let (root, user_extensions_dir, global_extensions_dir, source_store_root) = setup_paths().await;
    let extension_dir = user_extensions_dir.join("movable-ext");
    fs::create_dir_all(&extension_dir)
        .await
        .expect("create extension dir");
    fs::write(
        extension_dir.join(".tauritavern-source.json"),
        serde_json::to_vec_pretty(&legacy_source_metadata()).expect("serialize legacy source"),
    )
    .await
    .expect("write legacy source state");

    let repository = FileExtensionRepository::new(
        user_extensions_dir.clone(),
        global_extensions_dir.clone(),
        source_store_root.clone(),
    )
    .expect("create extension repository");

    repository
        .move_extension("third-party/movable-ext", "local", "global")
        .await
        .expect("move extension");

    assert!(
        !user_extensions_dir.join("movable-ext").exists(),
        "source extension directory should be removed"
    );
    assert!(
        global_extensions_dir.join("movable-ext").exists(),
        "destination extension directory should exist"
    );
    assert!(
        !source_store_root
            .join("local")
            .join("movable-ext.json")
            .exists(),
        "local state file should be removed"
    );
    assert!(
        source_store_root
            .join("global")
            .join("movable-ext.json")
            .exists(),
        "global state file should exist"
    );

    fs::remove_dir_all(root).await.expect("cleanup temp root");
}

#[tokio::test]
async fn delete_extension_removes_source_state_file() {
    let (root, user_extensions_dir, global_extensions_dir, source_store_root) = setup_paths().await;
    let extension_dir = user_extensions_dir.join("delete-ext");
    fs::create_dir_all(&extension_dir)
        .await
        .expect("create extension dir");
    fs::write(
        extension_dir.join(".tauritavern-source.json"),
        serde_json::to_vec_pretty(&legacy_source_metadata()).expect("serialize legacy source"),
    )
    .await
    .expect("write legacy source state");

    let repository = FileExtensionRepository::new(
        user_extensions_dir.clone(),
        global_extensions_dir,
        source_store_root.clone(),
    )
    .expect("create extension repository");

    repository
        .delete_extension("third-party/delete-ext", false)
        .await
        .expect("delete extension");

    assert!(
        !extension_dir.exists(),
        "extension directory should be removed"
    );
    assert!(
        !source_store_root
            .join("local")
            .join("delete-ext.json")
            .exists(),
        "source state file should be removed"
    );

    fs::remove_dir_all(root).await.expect("cleanup temp root");
}

#[tokio::test]
async fn discover_extensions_deletes_extensions_without_source_state() {
    let (root, user_extensions_dir, global_extensions_dir, source_store_root) = setup_paths().await;
    let extension_dir = user_extensions_dir.join("orphan-ext");
    fs::create_dir_all(&extension_dir)
        .await
        .expect("create extension dir");

    let repository = FileExtensionRepository::new(
        user_extensions_dir.clone(),
        global_extensions_dir,
        source_store_root,
    )
    .expect("create extension repository");

    let extensions = repository
        .discover_extensions()
        .await
        .expect("discover extensions");

    assert!(
        !extension_dir.exists(),
        "extension directory should be removed when source state is missing"
    );
    assert!(
        !extensions
            .iter()
            .any(|extension| extension.name == "third-party/orphan-ext"),
        "orphan extension should not be returned after cleanup"
    );

    fs::remove_dir_all(root).await.expect("cleanup temp root");
}
