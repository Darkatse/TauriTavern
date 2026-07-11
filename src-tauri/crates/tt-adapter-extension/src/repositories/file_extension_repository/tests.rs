use std::path::PathBuf;
use std::sync::Arc;

use rand::random;
use serde_json::json;
use tokio::fs;

use tt_adapter_http::HttpClientPool;
use tt_domain::errors::DomainError;
use tt_ports::repositories::extension_repository::ExtensionRepository;

use super::FileExtensionRepository;
use super::git_test_server::GitTestServer;
use super::git_worktree::{ManagedRef, read_managed_state};

const TEST_USER_AGENT: &str = "TauriTavern/test";

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

fn test_http_clients() -> Arc<HttpClientPool> {
    Arc::new(HttpClientPool::new(TEST_USER_AGENT))
}

#[tokio::test]
async fn embedded_install_version_and_update_round_trip_over_smart_http() {
    let (root, user_extensions_dir, global_extensions_dir, source_store_root) = setup_paths().await;
    let mut server = GitTestServer::start(root.join("git-origin"));
    let first = server.write_main("1.0.0");
    let remote_url = server.remote_url();
    let repository = FileExtensionRepository::new(
        user_extensions_dir.clone(),
        global_extensions_dir.clone(),
        source_store_root.clone(),
        test_http_clients(),
    )
    .expect("create extension repository");

    let installed = repository
        .install_extension(&remote_url, false, None)
        .await
        .expect("install embedded extension");
    assert_eq!(installed.folder_name, "repo");
    assert_eq!(installed.version, "1.0.0");
    let extension_path = user_extensions_dir.join("repo");
    assert!(extension_path.join(".git").is_dir());
    assert!(!source_store_root.join("local/repo.json").exists());

    drop(repository);
    let repository = FileExtensionRepository::new(
        user_extensions_dir.clone(),
        global_extensions_dir.clone(),
        source_store_root.clone(),
        test_http_clients(),
    )
    .expect("restart extension repository");
    assert!(!source_store_root.join("local/repo.json").exists());

    let discovered = repository
        .discover_extensions()
        .await
        .expect("discover embedded extension")
        .into_iter()
        .find(|extension| extension.name == "third-party/repo")
        .expect("installed extension projection");
    let first_hex = first.to_string();
    assert!(discovered.managed);
    assert_eq!(discovered.commit_hash.as_deref(), Some(first_hex.as_str()));
    assert_eq!(discovered.branch_name.as_deref(), Some("main"));
    assert_eq!(discovered.remote_url.as_deref(), Some(remote_url.as_str()));

    server.write_annotated_tag("main");
    let explicit = repository
        .install_extension(&remote_url, true, Some("main".to_string()))
        .await
        .expect("install explicit embedded branch");
    assert_eq!(explicit.version, "1.0.0");
    assert!(global_extensions_dir.join("repo/.git").is_dir());
    assert!(!source_store_root.join("global/repo.json").exists());
    let explicit_repo = super::git_remote::open_embedded(&global_extensions_dir.join("repo"))
        .expect("open explicit branch installation");
    assert!(matches!(
        read_managed_state(&explicit_repo).unwrap().selected,
        ManagedRef::Branch { .. }
    ));

    fs::create_dir(user_extensions_dir.join("legacy"))
        .await
        .expect("create legacy extension");
    fs::write(
        source_store_root.join("local/legacy.json"),
        serde_json::to_vec_pretty(&json!({
            "host": "legacy.invalid",
            "repo_path": "unused/repo",
            "reference": "main",
            "remote_url": remote_url,
            "installed_commit": first.to_string(),
        }))
        .unwrap(),
    )
    .await
    .expect("write legacy source state");
    assert!(
        repository
            .get_extension_version("legacy", false)
            .await
            .expect("legacy Git advertisement version")
            .is_up_to_date
    );

    let current = repository
        .get_extension_version("repo", false)
        .await
        .expect("read current version");
    assert!(current.is_up_to_date);
    assert_eq!(current.current_commit_hash, first.to_string());

    fs::write(extension_path.join("obsolete.txt"), "obsolete")
        .await
        .expect("write obsolete payload");
    let second = server.write_main("2.0.0");
    let behind = repository
        .get_extension_version("repo", false)
        .await
        .expect("read behind version");
    assert!(!behind.is_up_to_date);
    assert!(
        !repository
            .get_extension_version("legacy", false)
            .await
            .expect("legacy Git advertisement detects update")
            .is_up_to_date
    );

    let updated = repository
        .update_extension("repo", false)
        .await
        .expect("update embedded extension");
    assert!(!updated.is_up_to_date);
    assert_eq!(updated.short_commit_hash, second.to_string()[..7]);
    assert!(!extension_path.join("obsolete.txt").exists());
    assert_eq!(
        fs::read_to_string(extension_path.join("payload.txt"))
            .await
            .unwrap(),
        "payload-2.0.0"
    );

    let no_op = repository
        .update_extension("repo", false)
        .await
        .expect("no-op embedded update");
    assert!(no_op.is_up_to_date);

    let status = std::process::Command::new("git")
        .args([
            "-C",
            extension_path.to_str().unwrap(),
            "status",
            "--porcelain",
        ])
        .output()
        .expect("run system Git status");
    assert!(
        status.status.success(),
        "{}",
        String::from_utf8_lossy(&status.stderr)
    );
    assert!(status.stdout.is_empty(), "worktree must be clean");
    let fsck = std::process::Command::new("git")
        .args([
            "-C",
            extension_path.to_str().unwrap(),
            "fsck",
            "--no-dangling",
        ])
        .output()
        .expect("run system Git fsck");
    assert!(
        fsck.status.success(),
        "{}",
        String::from_utf8_lossy(&fsck.stderr)
    );

    drop(explicit_repo);
    drop(repository);
    drop(server);
    fs::remove_dir_all(root).await.expect("cleanup temp root");
}

#[tokio::test]
async fn embedded_annotated_tag_install_and_moving_tag_update() {
    let (root, user_extensions_dir, global_extensions_dir, source_store_root) = setup_paths().await;
    let mut server = GitTestServer::start(root.join("git-origin"));
    let first = server.write_main("1.0.0");
    server.write_annotated_tag("v1");
    let remote_url = server.remote_url();
    let repository = FileExtensionRepository::new(
        user_extensions_dir.clone(),
        global_extensions_dir,
        source_store_root,
        test_http_clients(),
    )
    .expect("create extension repository");

    repository
        .install_extension(&remote_url, false, Some("v1".to_string()))
        .await
        .expect("install annotated tag");
    let version = repository
        .get_extension_version("repo", false)
        .await
        .expect("read installed tag version");
    assert_eq!(version.current_branch_name, "v1");
    assert_eq!(version.current_commit_hash, first.to_string());
    assert!(version.is_up_to_date);

    let second = server.write_main("2.0.0");
    server.write_annotated_tag("v1");
    let updated = repository
        .update_extension("repo", false)
        .await
        .expect("update moving annotated tag");
    assert!(!updated.is_up_to_date);
    let version = repository
        .get_extension_version("repo", false)
        .await
        .expect("read updated tag version");
    assert_eq!(version.current_commit_hash, second.to_string());
    assert!(version.is_up_to_date);

    drop(repository);
    drop(server);
    fs::remove_dir_all(root).await.expect("cleanup temp root");
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
        test_http_clients(),
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
    assert!(extension.managed, "migrated extension should be managed");
    assert_eq!(
        extension.remote_url.as_deref(),
        Some("https://github.com/N0VI028/JS-Slash-Runner")
    );

    fs::remove_dir_all(root).await.expect("cleanup temp root");
}

#[tokio::test]
async fn corrupt_embedded_git_ignores_stale_legacy_source_state() {
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
    fs::write(
        extension_dir.join(".tauritavern-source.json"),
        serde_json::to_vec_pretty(&legacy_source_metadata()).unwrap(),
    )
    .await
    .expect("write stale legacy source state");

    let repository = FileExtensionRepository::new(
        user_extensions_dir.clone(),
        global_extensions_dir,
        source_store_root.clone(),
        test_http_clients(),
    )
    .expect("create extension repository");

    assert!(
        !source_store_root
            .join("local")
            .join("git-ext.json")
            .exists()
    );
    assert!(extension_dir.join(".tauritavern-source.json").exists());

    let extensions = repository
        .discover_extensions()
        .await
        .expect("discover extensions");
    let extension = extensions
        .into_iter()
        .find(|extension| extension.name == "third-party/git-ext")
        .expect("git extension should be discoverable");
    assert!(!extension.managed, "corrupt git must remain unmanaged");
    assert_eq!(extension.remote_url, None);
    assert!(
        repository
            .get_extension_version("git-ext", false)
            .await
            .is_err()
    );
    assert!(repository.update_extension("git-ext", false).await.is_err());

    fs::remove_dir_all(root).await.expect("cleanup temp root");
}

#[tokio::test]
async fn unsupported_gitfile_layout_is_not_converted_to_source_json() {
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
        test_http_clients(),
    )
    .expect("create extension repository");

    assert!(
        !source_store_root
            .join("local")
            .join("gitfile-ext.json")
            .exists()
    );

    let extensions = repository
        .discover_extensions()
        .await
        .expect("discover extensions");
    let extension = extensions
        .into_iter()
        .find(|extension| extension.name == "third-party/gitfile-ext")
        .expect("gitfile extension should be discoverable");
    assert!(
        !extension.managed,
        "unsupported gitfile must remain unmanaged"
    );
    assert_eq!(extension.remote_url, None);

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
        test_http_clients(),
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
        test_http_clients(),
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
async fn delete_extension_rejects_nested_extension_identifier() {
    let (root, user_extensions_dir, global_extensions_dir, source_store_root) = setup_paths().await;
    let repository = FileExtensionRepository::new(
        user_extensions_dir,
        global_extensions_dir,
        source_store_root,
        test_http_clients(),
    )
    .expect("create extension repository");

    let result = repository
        .delete_extension("third-party/delete-ext/nested", false)
        .await;

    assert!(matches!(result, Err(DomainError::InvalidData(_))));

    fs::remove_dir_all(root).await.expect("cleanup temp root");
}

#[tokio::test]
async fn discover_extensions_keeps_extensions_without_source_state_as_unmanaged() {
    let (root, user_extensions_dir, global_extensions_dir, source_store_root) = setup_paths().await;
    let extension_dir = user_extensions_dir.join("orphan-ext");
    fs::create_dir_all(&extension_dir)
        .await
        .expect("create extension dir");
    fs::write(
        extension_dir.join("manifest.json"),
        serde_json::to_vec_pretty(&json!({
            "display_name": "Orphan Extension",
            "version": "0.0.1",
            "author": "Unknown"
        }))
        .expect("serialize orphan manifest"),
    )
    .await
    .expect("write orphan manifest");

    let repository = FileExtensionRepository::new(
        user_extensions_dir.clone(),
        global_extensions_dir,
        source_store_root,
        test_http_clients(),
    )
    .expect("create extension repository");

    let extensions = repository
        .discover_extensions()
        .await
        .expect("discover extensions");

    assert!(
        extension_dir.exists(),
        "unmanaged extension directory should not be deleted"
    );
    assert!(
        extensions
            .iter()
            .any(|extension| extension.name == "third-party/orphan-ext" && !extension.managed),
        "orphan extension should be returned and marked unmanaged"
    );

    fs::remove_dir_all(root).await.expect("cleanup temp root");
}

#[tokio::test]
async fn discover_extensions_accepts_single_item_asset_arrays_in_manifest() {
    let (root, user_extensions_dir, global_extensions_dir, source_store_root) = setup_paths().await;
    let extension_dir = user_extensions_dir.join("array-assets-ext");
    fs::create_dir_all(&extension_dir)
        .await
        .expect("create extension dir");
    fs::write(
        extension_dir.join("manifest.json"),
        serde_json::to_vec_pretty(&json!({
            "display_name": "Array Assets Extension",
            "version": "1.2.3",
            "author": "Faxrd9",
            "description": "Uses single-item asset arrays",
            "loading_order": 10,
            "js": ["index.js"],
            "css": ["style.css"],
            "entryPoint": "index.js"
        }))
        .expect("serialize manifest"),
    )
    .await
    .expect("write manifest");

    let repository = FileExtensionRepository::new(
        user_extensions_dir,
        global_extensions_dir,
        source_store_root,
        test_http_clients(),
    )
    .expect("create extension repository");

    let extensions = repository
        .discover_extensions()
        .await
        .expect("discover extensions");

    let extension = extensions
        .into_iter()
        .find(|extension| extension.name == "third-party/array-assets-ext")
        .expect("array-assets extension should be discoverable");

    assert!(!extension.managed, "extension should remain unmanaged");
    let manifest = extension.manifest.expect("manifest summary should exist");
    assert_eq!(manifest.display_name, "Array Assets Extension");
    assert_eq!(manifest.version, "1.2.3");
    assert_eq!(manifest.author, "Faxrd9");
    assert_eq!(manifest.description, "Uses single-item asset arrays");
    assert_eq!(manifest.loading_order, 10);

    fs::remove_dir_all(root).await.expect("cleanup temp root");
}
