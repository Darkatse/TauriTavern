use std::sync::Arc;

use super::*;
use tt_domain::models::agent::WorkspacePersistentChange as Change;
use tt_ports::workspace_fs::{WorkspaceEntryKind, WorkspaceFs};

fn path(value: &str) -> WorkspacePath {
    WorkspacePath::parse(value).unwrap()
}

async fn initialize(repository: &FileAgentRepository, run: &AgentRun) -> Arc<dyn WorkspaceFs> {
    let manifest = sample_manifest(run);
    repository.create_run(run).await.unwrap();
    repository
        .initialize_run(
            run,
            &manifest,
            &serde_json::json!({}),
            &sample_resolved_profile(&manifest),
        )
        .await
        .unwrap();
    repository.open_filesystem(&run.id).await.unwrap()
}

#[tokio::test]
async fn binary_workspace_content_survives_append_and_copy_without_text_conversion() {
    let root = temp_root();
    let repository = FileAgentRepository::new(root.clone());
    let files = initialize(&repository, &sample_run()).await;
    let source = path("output/data");
    files
        .write_file(&source, &[0xff, 0, 1], WorkspaceWriteGuard::Unchecked)
        .await
        .unwrap();
    assert!(matches!(
        files.read_text(&source).await,
        Err(DomainError::WorkspaceFileNotText { .. })
    ));
    assert!(matches!(
        files.append_text(&source, "text").await,
        Err(DomainError::WorkspaceFileNotText { .. })
    ));
    files.append_file(&source, &[2]).await.unwrap();
    let target = path("persist/data");
    files.copy_file(&source, &target).await.unwrap();
    assert_eq!(files.read_file(&target, 4).await.unwrap(), [0xff, 0, 1, 2]);
    assert!(
        files.read_file(&target, 3).await.is_err(),
        "a bounded read must reject excess bytes rather than truncate"
    );
    fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn persistent_tree_preserves_deletion_moves_empty_directories_and_type_changes() {
    let root = temp_root();
    let repository = FileAgentRepository::new(root.clone());
    let base_run = sample_run_with_id("tree_base");
    let base = initialize(&repository, &base_run).await;
    for (name, text) in [
        ("delete", "gone"),
        ("move", "moved bytes"),
        ("to-dir", "old file"),
        ("to-file/old", "child"),
    ] {
        base.write_text(
            &path(&format!("persist/{name}")),
            text,
            WorkspaceWriteGuard::Unchecked,
        )
        .await
        .unwrap();
    }
    base.create_dir(&path("persist/old-empty"), false)
        .await
        .unwrap();
    let first = repository
        .commit_persistent_changes(&base_run.id, None)
        .await
        .unwrap();
    let mut run = sample_run_with_id("tree_edit");
    run.chat_target_mut().unwrap().persist_base_state_id = Some(first.state_id.clone());
    let files = initialize(&repository, &run).await;
    files.remove(&path("persist/delete"), false).await.unwrap();
    files
        .rename(&path("persist/move"), &path("persist/moved"))
        .await
        .unwrap();
    files.remove(&path("persist/to-dir"), false).await.unwrap();
    files
        .create_dir(&path("persist/to-dir"), false)
        .await
        .unwrap();
    files.remove(&path("persist/to-file"), true).await.unwrap();
    files
        .write_text(
            &path("persist/to-file"),
            "new file",
            WorkspaceWriteGuard::Unchecked,
        )
        .await
        .unwrap();
    files
        .remove(&path("persist/old-empty"), false)
        .await
        .unwrap();
    files
        .create_dir(&path("persist/new-empty"), false)
        .await
        .unwrap();
    let second = repository
        .commit_persistent_changes(&run.id, None)
        .await
        .unwrap();
    assert!(second.changes.contains(&Change::Deleted {
        path: "persist/delete".into()
    }));
    assert!(second.changes.contains(&Change::DirectoryDeleted {
        path: "persist/old-empty".into()
    }));
    assert!(second.changes.contains(&Change::DirectoryAdded {
        path: "persist/new-empty".into()
    }));
    let type_change: Vec<_> = second
        .changes
        .iter()
        .filter(|change| change.path() == "persist/to-dir")
        .collect();
    assert!(matches!(
        type_change.as_slice(),
        [Change::Deleted { .. }, Change::DirectoryAdded { .. }]
    ));
    let type_change: Vec<_> = second
        .changes
        .iter()
        .filter(|change| change.path() == "persist/to-file")
        .collect();
    assert!(matches!(
        type_change.as_slice(),
        [Change::DirectoryDeleted { .. }, Change::Added { .. }]
    ));

    let mut next_run = sample_run_with_id("tree_next");
    next_run.chat_target_mut().unwrap().persist_base_state_id = Some(second.state_id.clone());
    let next = initialize(&repository, &next_run).await;
    for name in ["delete", "move", "old-empty", "to-file/old"] {
        assert!(
            next.metadata(Some(&path(&format!("persist/{name}"))))
                .await
                .is_err()
        );
    }
    for name in ["new-empty", "to-dir"] {
        assert_eq!(
            next.metadata(Some(&path(&format!("persist/{name}"))))
                .await
                .unwrap()
                .kind,
            WorkspaceEntryKind::Directory
        );
    }
    assert_eq!(
        next.read_text(&path("persist/moved")).await.unwrap().text,
        "moved bytes"
    );
    assert_eq!(
        next.read_text(&path("persist/to-file")).await.unwrap().text,
        "new file"
    );
    let original_dir = repository
        .persistent_state_dir(&run.workspace_id, &first.state_id)
        .unwrap();
    assert_eq!(
        fs::read(original_dir.join("persist/delete")).await.unwrap(),
        b"gone"
    );
    assert!(original_dir.join("persist/old-empty").is_dir());

    files
        .create_dir(&path("persist/revision-empty"), false)
        .await
        .unwrap();
    let revised = repository
        .commit_persistent_changes(&run.id, Some(&second.state_id))
        .await
        .unwrap();
    assert_ne!(revised.state_id, second.state_id);
    files
        .remove(&path("persist/revision-empty"), false)
        .await
        .unwrap();
    assert_eq!(
        repository
            .commit_persistent_changes(&run.id, Some(&second.state_id))
            .await
            .unwrap(),
        second
    );
    fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn legacy_baseline_uses_immutable_directories_and_reuses_legacy_summary() {
    let root = temp_root();
    let repository = FileAgentRepository::new(root.clone());
    let base_run = sample_run_with_id("legacy_base");
    let files = initialize(&repository, &base_run).await;
    files
        .create_dir(&path("persist/empty"), false)
        .await
        .unwrap();
    files
        .write_text(&path("persist/file"), "v1", WorkspaceWriteGuard::Unchecked)
        .await
        .unwrap();
    let published = repository
        .commit_persistent_changes(&base_run.id, None)
        .await
        .unwrap();
    // v1 manifests contain file changes only and have no directory inventory.
    let manifest_path = repository
        .persistent_state_dir(&base_run.workspace_id, &published.state_id)
        .unwrap()
        .join("manifest.json");
    let mut manifest: Value =
        serde_json::from_slice(&fs::read(&manifest_path).await.unwrap()).unwrap();
    manifest["version"] = serde_json::json!(1);
    manifest.as_object_mut().unwrap().remove("directories");
    manifest["changes"]
        .as_array_mut()
        .unwrap()
        .retain(|change| change["kind"] == "added");
    fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap())
        .await
        .unwrap();
    let reused = repository
        .commit_persistent_changes(&base_run.id, Some(&published.state_id))
        .await
        .unwrap();
    assert_eq!(reused.state_id, published.state_id);
    assert_eq!(
        serde_json::to_value(&reused.changes).unwrap(),
        manifest["changes"]
    );

    let mut run = sample_run_with_id("legacy_edit");
    run.chat_target_mut().unwrap().persist_base_state_id = Some(published.state_id);
    let files = initialize(&repository, &run).await;
    let snapshot_path = repository
        .run_dir(&run)
        .unwrap()
        .join("input/persist_snapshot.json");
    let mut snapshot: Value =
        serde_json::from_slice(&fs::read(&snapshot_path).await.unwrap()).unwrap();
    snapshot.as_object_mut().unwrap().remove("directories");
    fs::write(snapshot_path, serde_json::to_vec(&snapshot).unwrap())
        .await
        .unwrap();
    files.remove(&path("persist/empty"), false).await.unwrap();
    let changed = repository
        .commit_persistent_changes(&run.id, None)
        .await
        .unwrap();
    assert_eq!(
        changed.changes,
        vec![Change::DirectoryDeleted {
            path: "persist/empty".into()
        }]
    );
    fs::remove_dir_all(root).await.unwrap();
}
