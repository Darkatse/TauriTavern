use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

use ttsync_contract::manifest::{ManifestEntryV2, ManifestV2};
use ttsync_contract::path::SyncPath;
use ttsync_core::dataset::ResolvedDatasetPolicy;

use tt_domain::errors::DomainError;

pub async fn scan_manifest_with_policy(
    sync_root: PathBuf,
    policy: ResolvedDatasetPolicy,
) -> Result<ManifestV2, DomainError> {
    tokio::task::spawn_blocking(move || scan_manifest_sync(&sync_root, &policy))
        .await
        .map_err(|error| DomainError::InternalError(error.to_string()))?
}

fn scan_manifest_sync(
    sync_root: &Path,
    policy: &ResolvedDatasetPolicy,
) -> Result<ManifestV2, DomainError> {
    let mut entries = Vec::new();

    for directory in policy.scan_roots() {
        let root = sync_root.join(directory);
        if !root.exists() {
            continue;
        }
        if !root.is_dir() {
            return Err(DomainError::InvalidData(format!(
                "Sync scope root is not a directory: {}",
                root.display()
            )));
        }

        scan_dir_recursive(sync_root, &root, policy, &mut entries)?;
    }

    for file in policy.files() {
        if !policy.contains_path(file) {
            continue;
        }

        let path = sync_root.join(file);
        if !path.exists() {
            continue;
        }
        if !path.is_file() {
            return Err(DomainError::InvalidData(format!(
                "Sync scope root is not a file: {}",
                path.display()
            )));
        }

        entries.push(make_entry(sync_root, &path)?);
    }

    entries.sort_by(|a, b| a.path.as_str().cmp(b.path.as_str()));
    Ok(ManifestV2 { entries })
}

fn scan_dir_recursive(
    sync_root: &Path,
    dir: &Path,
    policy: &ResolvedDatasetPolicy,
    entries: &mut Vec<ManifestEntryV2>,
) -> Result<(), DomainError> {
    for entry in std::fs::read_dir(dir).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to read directory {}: {}",
            dir.display(),
            error
        ))
    })? {
        let entry = entry.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read directory entry in {}: {}",
                dir.display(),
                error
            ))
        })?;

        let file_type = entry.file_type().map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read file type for {}: {}",
                entry.path().display(),
                error
            ))
        })?;

        let entry_path = entry.path();

        if file_type.is_symlink() {
            return Err(DomainError::InvalidData(format!(
                "Symlinks are not supported in sync scope: {}",
                entry_path.display()
            )));
        }

        let relative = entry_path
            .strip_prefix(sync_root)
            .map_err(|error| DomainError::InternalError(error.to_string()))?;
        let relative = normalize_relative_path(relative)?;

        if ttsync_core::dataset::is_excluded(&relative) {
            continue;
        }

        if file_type.is_dir() {
            if ttsync_core::dataset::is_agent_run_root_dir(&relative)
                && !agent_run_file_is_terminal(&entry_path.join("run.json"))?
            {
                continue;
            }

            if !policy.should_descend_dir(&relative) {
                continue;
            }

            scan_dir_recursive(sync_root, &entry_path, policy, entries)?;
            continue;
        }

        if file_type.is_file() && policy.contains_path(&relative) {
            if ttsync_core::dataset::is_agent_run_index_file(&relative)
                && !agent_run_file_is_terminal(&entry_path)?
            {
                continue;
            }

            entries.push(make_entry(sync_root, &entry_path)?);
        }
    }

    Ok(())
}

fn agent_run_file_is_terminal(path: &Path) -> Result<bool, DomainError> {
    if !path.exists() {
        return Ok(false);
    }

    let text = std::fs::read_to_string(path).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to read agent run {}: {}",
            path.display(),
            error
        ))
    })?;
    ttsync_core::dataset::agent_run_json_is_terminal(&text)
        .map_err(|error| DomainError::InvalidData(format!("{}: {}", path.display(), error)))
}

fn make_entry(sync_root: &Path, file_path: &Path) -> Result<ManifestEntryV2, DomainError> {
    let relative = file_path
        .strip_prefix(sync_root)
        .map_err(|error| DomainError::InternalError(error.to_string()))?;
    let relative = normalize_relative_path(relative)?;
    let path =
        SyncPath::new(relative).map_err(|error| DomainError::InvalidData(error.to_string()))?;

    let metadata = std::fs::metadata(file_path).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to read metadata for {}: {}",
            file_path.display(),
            error
        ))
    })?;

    let modified_ms = metadata
        .modified()
        .map_err(|error| DomainError::InternalError(error.to_string()))?
        .duration_since(UNIX_EPOCH)
        .map_err(|error| DomainError::InternalError(error.to_string()))?
        .as_millis() as u64;

    Ok(ManifestEntryV2 {
        path,
        size_bytes: metadata.len(),
        modified_ms,
        content_hash: None,
    })
}

fn normalize_relative_path(path: &Path) -> Result<String, DomainError> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => parts.push(value.to_str().ok_or_else(|| {
                DomainError::InvalidData("Path contains non-UTF-8 components".to_string())
            })?),
            Component::CurDir => continue,
            other => {
                return Err(DomainError::InvalidData(format!(
                    "Path contains unsupported component: {:?}",
                    other
                )));
            }
        }
    }

    Ok(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use rand::random;
    use ttsync_core::dataset::ResolvedDatasetPolicy;

    use super::scan_manifest_sync;

    fn unique_temp_root() -> PathBuf {
        std::env::temp_dir().join(format!("tauritavern-tt-sync-{}", random::<u64>()))
    }

    #[test]
    fn scan_manifest_excludes_all_sync_state_files() {
        let root = unique_temp_root();
        let _ = std::fs::remove_dir_all(&root);
        let lan_sync_dir = root.join("default-user").join("user").join("lan-sync");

        std::fs::create_dir_all(root.join("default-user").join("chats"))
            .expect("create chats directory");
        std::fs::create_dir_all(lan_sync_dir.join("v2")).expect("create lan sync state directory");
        std::fs::create_dir_all(lan_sync_dir.join("tt-sync-v2"))
            .expect("create tt sync state directory");

        std::fs::write(
            root.join("default-user").join("chats").join("chat.jsonl"),
            b"chat",
        )
        .expect("write included file");
        std::fs::write(lan_sync_dir.join("server-settings.json"), b"{}")
            .expect("write excluded server settings");
        std::fs::write(lan_sync_dir.join("sync-preferences.json"), b"{}")
            .expect("write excluded sync preferences");
        std::fs::write(lan_sync_dir.join("v2").join("identity.json"), b"{}")
            .expect("write excluded lan identity");
        std::fs::write(lan_sync_dir.join("v2").join("peers.json"), b"[]")
            .expect("write excluded lan peers");
        std::fs::write(lan_sync_dir.join("tt-sync-v2").join("identity.json"), b"{}")
            .expect("write excluded tt sync identity");
        std::fs::write(
            lan_sync_dir.join("tt-sync-v2").join("paired-servers.json"),
            b"[]",
        )
        .expect("write excluded paired servers");

        std::fs::create_dir_all(
            root.join("_tauritavern")
                .join("agent-profiles")
                .join("profiles"),
        )
        .expect("create agent profile directory");
        std::fs::create_dir_all(
            root.join("_tauritavern")
                .join("agent-workspaces")
                .join("chats")
                .join("workspace")
                .join("persistent-states")
                .join("run-1"),
        )
        .expect("create agent persistent state directory");
        std::fs::create_dir_all(root.join("_tauritavern").join("prompt-cache"))
            .expect("create prompt cache directory");
        std::fs::create_dir_all(
            root.join("_tauritavern")
                .join("agent-workspaces")
                .join("chats")
                .join("workspace")
                .join("runs")
                .join("run-done"),
        )
        .expect("create terminal run directory");
        std::fs::create_dir_all(
            root.join("_tauritavern")
                .join("agent-workspaces")
                .join("chats")
                .join("workspace")
                .join("runs")
                .join("run-done")
                .join("input"),
        )
        .expect("create terminal run input directory");
        std::fs::create_dir_all(
            root.join("_tauritavern")
                .join("agent-workspaces")
                .join("chats")
                .join("workspace")
                .join("runs")
                .join("run-done")
                .join("model-responses"),
        )
        .expect("create terminal run model responses directory");
        std::fs::create_dir_all(
            root.join("_tauritavern")
                .join("agent-workspaces")
                .join("chats")
                .join("workspace")
                .join("runs")
                .join("run-active"),
        )
        .expect("create active run directory");
        std::fs::create_dir_all(
            root.join("_tauritavern")
                .join("agent-workspaces")
                .join("index")
                .join("runs"),
        )
        .expect("create run index directory");

        std::fs::write(
            root.join("_tauritavern")
                .join("agent-profiles")
                .join("profiles")
                .join("writer.json"),
            b"{}",
        )
        .expect("write agent profile");
        std::fs::write(
            root.join("_tauritavern")
                .join("agent-workspaces")
                .join("chats")
                .join("workspace")
                .join("persistent-states")
                .join("run-1")
                .join("manifest.json"),
            b"{}",
        )
        .expect("write agent persistent state");
        std::fs::write(
            root.join("_tauritavern")
                .join("prompt-cache")
                .join("cache.json"),
            b"{}",
        )
        .expect("write prompt cache");
        std::fs::write(
            root.join("_tauritavern")
                .join("agent-workspaces")
                .join("chats")
                .join("workspace")
                .join("runs")
                .join("run-done")
                .join("run.json"),
            br#"{"status":"completed"}"#,
        )
        .expect("write terminal run");
        std::fs::write(
            root.join("_tauritavern")
                .join("agent-workspaces")
                .join("chats")
                .join("workspace")
                .join("runs")
                .join("run-done")
                .join("events.jsonl"),
            b"{}\n",
        )
        .expect("write terminal event");
        std::fs::write(
            root.join("_tauritavern")
                .join("agent-workspaces")
                .join("chats")
                .join("workspace")
                .join("runs")
                .join("run-done")
                .join("input")
                .join("prompt_snapshot.json"),
            b"{}",
        )
        .expect("write terminal run input");
        std::fs::write(
            root.join("_tauritavern")
                .join("agent-workspaces")
                .join("chats")
                .join("workspace")
                .join("runs")
                .join("run-done")
                .join("model-responses")
                .join("round-001.json"),
            b"{}",
        )
        .expect("write terminal run model response");
        std::fs::write(
            root.join("_tauritavern")
                .join("agent-workspaces")
                .join("chats")
                .join("workspace")
                .join("runs")
                .join("run-active")
                .join("run.json"),
            br#"{"status":"calling_model"}"#,
        )
        .expect("write active run");
        std::fs::write(
            root.join("_tauritavern")
                .join("agent-workspaces")
                .join("chats")
                .join("workspace")
                .join("runs")
                .join("run-active")
                .join("events.jsonl"),
            b"{}\n",
        )
        .expect("write active event");
        std::fs::write(
            root.join("_tauritavern")
                .join("agent-workspaces")
                .join("index")
                .join("runs")
                .join("run-done.json"),
            br#"{"status":"completed"}"#,
        )
        .expect("write terminal run index");
        std::fs::write(
            root.join("_tauritavern")
                .join("agent-workspaces")
                .join("index")
                .join("runs")
                .join("run-active.json"),
            br#"{"status":"calling_model"}"#,
        )
        .expect("write active run index");

        let policy = ResolvedDatasetPolicy::tauri_tavern_default();
        let manifest = scan_manifest_sync(&root, &policy).expect("scan manifest");
        let paths = manifest
            .entries
            .into_iter()
            .map(|entry| entry.path.to_string())
            .collect::<Vec<_>>();

        assert!(
            paths.contains(&"default-user/chats/chat.jsonl".to_string()),
            "included file must appear in manifest"
        );
        for state_path in [
            "default-user/user/lan-sync/server-settings.json",
            "default-user/user/lan-sync/sync-preferences.json",
            "default-user/user/lan-sync/v2/identity.json",
            "default-user/user/lan-sync/v2/peers.json",
            "default-user/user/lan-sync/tt-sync-v2/identity.json",
            "default-user/user/lan-sync/tt-sync-v2/paired-servers.json",
        ] {
            assert!(
                !paths.contains(&state_path.to_string()),
                "{state_path} must never be part of the manifest"
            );
        }
        assert!(
            paths.contains(&"_tauritavern/agent-profiles/profiles/writer.json".to_string()),
            "agent profiles are part of the TauriTavern default dataset"
        );
        assert!(
            paths.contains(
                &"_tauritavern/agent-workspaces/chats/workspace/persistent-states/run-1/manifest.json"
                    .to_string()
            ),
            "agent persistent state is part of the TauriTavern default dataset"
        );
        assert!(
            !paths.contains(&"_tauritavern/prompt-cache/cache.json".to_string()),
            "prompt cache is local runtime state"
        );
        assert!(paths.contains(
            &"_tauritavern/agent-workspaces/chats/workspace/runs/run-done/events.jsonl".to_string()
        ));
        assert!(
            !paths.contains(
                &"_tauritavern/agent-workspaces/chats/workspace/runs/run-done/input/prompt_snapshot.json"
                    .to_string()
            ),
            "default Agent continuity sync must not include run input context"
        );
        assert!(
            !paths.contains(
                &"_tauritavern/agent-workspaces/chats/workspace/runs/run-done/model-responses/round-001.json"
                    .to_string()
            ),
            "default Agent continuity sync must not include model responses"
        );
        assert!(
            paths.contains(&"_tauritavern/agent-workspaces/index/runs/run-done.json".to_string())
        );
        assert!(
            !paths.contains(
                &"_tauritavern/agent-workspaces/chats/workspace/runs/run-active/events.jsonl"
                    .to_string()
            ),
            "active agent runs must not be included"
        );
        assert!(
            !paths
                .contains(&"_tauritavern/agent-workspaces/index/runs/run-active.json".to_string()),
            "active agent run index entries must not be included"
        );

        std::fs::remove_dir_all(&root).expect("remove temp root");
    }
}
