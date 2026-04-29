use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::domain::errors::DomainError;
use crate::domain::models::agent::{
    AgentRun, AgentRunEvent, AgentRunEventLevel, Checkpoint, CheckpointFile, WorkspaceManifest,
    WorkspacePath,
};
use crate::domain::repositories::agent_run_repository::{
    AgentRunEventReadQuery, AgentRunRepository,
};
use crate::domain::repositories::checkpoint_repository::CheckpointRepository;
use crate::domain::repositories::workspace_repository::{
    WorkspaceEntry, WorkspaceEntryKind, WorkspaceFile, WorkspaceFileList, WorkspaceRepository,
};
use crate::infrastructure::persistence::file_system::{
    read_json_file, replace_file_with_fallback, unique_temp_path, write_json_file,
};

pub struct FileAgentRepository {
    root: PathBuf,
    event_lock: Arc<Mutex<()>>,
    checkpoint_lock: Arc<Mutex<()>>,
}

impl FileAgentRepository {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            event_lock: Arc::new(Mutex::new(())),
            checkpoint_lock: Arc::new(Mutex::new(())),
        }
    }

    fn index_run_path(&self, run_id: &str) -> Result<PathBuf, DomainError> {
        validate_segment(run_id, "run_id")?;
        Ok(self
            .root
            .join("index")
            .join("runs")
            .join(format!("{run_id}.json")))
    }

    fn run_dir(&self, run: &AgentRun) -> Result<PathBuf, DomainError> {
        validate_segment(&run.workspace_id, "workspace_id")?;
        validate_segment(&run.id, "run_id")?;
        Ok(self
            .root
            .join("chats")
            .join(&run.workspace_id)
            .join("runs")
            .join(&run.id))
    }

    async fn load_run_dir(&self, run_id: &str) -> Result<PathBuf, DomainError> {
        let run = self.load_run(run_id).await?;
        self.run_dir(&run)
    }

    async fn write_json_atomic<T: Serialize + ?Sized>(
        path: &Path,
        value: &T,
    ) -> Result<(), DomainError> {
        write_json_file(path, value).await
    }

    async fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, DomainError> {
        read_json_file(path).await
    }

    async fn safe_workspace_path(
        &self,
        run_id: &str,
        workspace_path: &WorkspacePath,
        create_parent: bool,
    ) -> Result<PathBuf, DomainError> {
        let run_dir = self.load_run_dir(run_id).await?;
        let target = run_dir.join(workspace_path.as_str());

        let canonical_run_dir = fs::canonicalize(&run_dir).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to resolve agent workspace root {}: {}",
                run_dir.display(),
                error
            ))
        })?;

        if let Some(parent) = target.parent() {
            if create_parent {
                fs::create_dir_all(parent).await.map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to create workspace parent {}: {}",
                        parent.display(),
                        error
                    ))
                })?;
            }

            let canonical_parent = fs::canonicalize(parent).await.map_err(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    DomainError::NotFound(format!(
                        "Workspace path parent not found: {}",
                        workspace_path.as_str()
                    ))
                } else {
                    DomainError::InternalError(format!(
                        "Failed to resolve workspace parent {}: {}",
                        parent.display(),
                        error
                    ))
                }
            })?;
            if !canonical_parent.starts_with(&canonical_run_dir) {
                return Err(DomainError::InvalidData(format!(
                    "Workspace path escapes run directory: {}",
                    workspace_path.as_str()
                )));
            }
        }

        match fs::symlink_metadata(&target).await {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(DomainError::InvalidData(format!(
                    "Workspace path targets a symlink: {}",
                    workspace_path.as_str()
                )));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(DomainError::InternalError(format!(
                    "Failed to inspect workspace path {}: {}",
                    target.display(),
                    error
                )));
            }
        }

        Ok(target)
    }

    async fn read_all_events(&self, run_id: &str) -> Result<Vec<AgentRunEvent>, DomainError> {
        let events_path = self.load_run_dir(run_id).await?.join("events.jsonl");
        let contents = match fs::read_to_string(&events_path).await {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Vec::new());
            }
            Err(error) => {
                return Err(DomainError::InternalError(format!(
                    "Failed to read agent event journal {}: {}",
                    events_path.display(),
                    error
                )));
            }
        };

        contents
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                serde_json::from_str::<AgentRunEvent>(line).map_err(|error| {
                    DomainError::InvalidData(format!(
                        "Invalid agent event in {}: {}",
                        events_path.display(),
                        error
                    ))
                })
            })
            .collect()
    }
}

#[async_trait]
impl AgentRunRepository for FileAgentRepository {
    async fn create_run(&self, run: &AgentRun) -> Result<(), DomainError> {
        let run_dir = self.run_dir(run)?;
        fs::create_dir_all(&run_dir).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to create agent run directory {}: {}",
                run_dir.display(),
                error
            ))
        })?;

        Self::write_json_atomic(&run_dir.join("run.json"), run).await?;
        Self::write_json_atomic(&self.index_run_path(&run.id)?, run).await
    }

    async fn load_run(&self, run_id: &str) -> Result<AgentRun, DomainError> {
        Self::read_json(&self.index_run_path(run_id)?).await
    }

    async fn save_run(&self, run: &AgentRun) -> Result<(), DomainError> {
        let run_dir = self.run_dir(run)?;
        Self::write_json_atomic(&run_dir.join("run.json"), run).await?;
        Self::write_json_atomic(&self.index_run_path(&run.id)?, run).await
    }

    async fn append_event(
        &self,
        run_id: &str,
        level: AgentRunEventLevel,
        event_type: &str,
        payload: Value,
    ) -> Result<AgentRunEvent, DomainError> {
        let _guard = self.event_lock.lock().await;
        let run_dir = self.load_run_dir(run_id).await?;
        let events_path = run_dir.join("events.jsonl");
        if let Some(parent) = events_path.parent() {
            fs::create_dir_all(parent).await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to create agent event journal parent {}: {}",
                    parent.display(),
                    error
                ))
            })?;
        }

        let seq = self
            .read_all_events(run_id)
            .await?
            .last()
            .map(|event| event.seq + 1)
            .unwrap_or(1);

        let event = AgentRunEvent {
            seq,
            id: format!("evt_{}", Uuid::new_v4().simple()),
            run_id: run_id.to_string(),
            timestamp: Utc::now(),
            level,
            event_type: event_type.to_string(),
            payload,
        };

        let line = serde_json::to_string(&event).map_err(|error| {
            DomainError::InvalidData(format!("Failed to serialize agent event: {error}"))
        })?;

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&events_path)
            .await
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to open agent event journal {}: {}",
                    events_path.display(),
                    error
                ))
            })?;
        file.write_all(line.as_bytes()).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to append agent event journal {}: {}",
                events_path.display(),
                error
            ))
        })?;
        file.write_all(b"\n").await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to append agent event journal newline {}: {}",
                events_path.display(),
                error
            ))
        })?;
        file.flush().await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to flush agent event journal {}: {}",
                events_path.display(),
                error
            ))
        })?;

        Ok(event)
    }

    async fn read_events(
        &self,
        run_id: &str,
        query: AgentRunEventReadQuery,
    ) -> Result<Vec<AgentRunEvent>, DomainError> {
        let limit = query.limit.clamp(1, 500);
        let mut events = self.read_all_events(run_id).await?;

        if let Some(before_seq) = query.before_seq {
            events.retain(|event| event.seq < before_seq);
            let start = events.len().saturating_sub(limit);
            return Ok(events.into_iter().skip(start).collect());
        }

        if let Some(after_seq) = query.after_seq {
            events.retain(|event| event.seq > after_seq);
        }

        events.truncate(limit);
        Ok(events)
    }
}

#[async_trait]
impl WorkspaceRepository for FileAgentRepository {
    async fn initialize_run(
        &self,
        run: &AgentRun,
        manifest: &WorkspaceManifest,
        prompt_snapshot: &Value,
    ) -> Result<(), DomainError> {
        let run_dir = self.run_dir(run)?;
        fs::create_dir_all(run_dir.join("input"))
            .await
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to create agent run input directory {}: {}",
                    run_dir.join("input").display(),
                    error
                ))
            })?;
        fs::create_dir_all(run_dir.join("output"))
            .await
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to create agent run output directory {}: {}",
                    run_dir.join("output").display(),
                    error
                ))
            })?;

        Self::write_json_atomic(&run_dir.join("manifest.json"), manifest).await?;
        Self::write_json_atomic(
            &run_dir.join("input").join("prompt_snapshot.json"),
            prompt_snapshot,
        )
        .await
    }

    async fn read_manifest(&self, run_id: &str) -> Result<WorkspaceManifest, DomainError> {
        Self::read_json(&self.load_run_dir(run_id).await?.join("manifest.json")).await
    }

    async fn write_text(
        &self,
        run_id: &str,
        path: &WorkspacePath,
        text: &str,
    ) -> Result<WorkspaceFile, DomainError> {
        let target = self.safe_workspace_path(run_id, path, true).await?;
        let temp_path = unique_temp_path(&target, "workspace.txt");
        fs::write(&temp_path, text.as_bytes())
            .await
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to write workspace temp file {}: {}",
                    temp_path.display(),
                    error
                ))
            })?;
        replace_file_with_fallback(&temp_path, &target).await?;

        workspace_file_from_text(path.clone(), text.to_string())
    }

    async fn read_text(
        &self,
        run_id: &str,
        path: &WorkspacePath,
    ) -> Result<WorkspaceFile, DomainError> {
        let target = self.safe_workspace_path(run_id, path, false).await?;
        let text = fs::read_to_string(&target).await.map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                DomainError::NotFound(format!("Workspace file not found: {}", path.as_str()))
            } else {
                DomainError::InternalError(format!(
                    "Failed to read workspace file {}: {}",
                    target.display(),
                    error
                ))
            }
        })?;

        workspace_file_from_text(path.clone(), text)
    }

    async fn list_files(
        &self,
        run_id: &str,
        path: Option<&WorkspacePath>,
        depth: usize,
        max_entries: usize,
    ) -> Result<WorkspaceFileList, DomainError> {
        let run_dir = self.load_run_dir(run_id).await?;
        let root = match path {
            Some(path) => self.safe_workspace_path(run_id, path, false).await?,
            None => run_dir.clone(),
        };

        let canonical_run_dir = fs::canonicalize(&run_dir).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to resolve agent workspace root {}: {}",
                run_dir.display(),
                error
            ))
        })?;
        let canonical_root = fs::canonicalize(&root).await.map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                DomainError::NotFound(format!(
                    "Workspace path not found: {}",
                    path.map(WorkspacePath::as_str).unwrap_or(".")
                ))
            } else {
                DomainError::InternalError(format!(
                    "Failed to resolve workspace list root {}: {}",
                    root.display(),
                    error
                ))
            }
        })?;
        if !canonical_root.starts_with(&canonical_run_dir) {
            return Err(DomainError::InvalidData(format!(
                "Workspace path escapes run directory: {}",
                path.map(WorkspacePath::as_str).unwrap_or(".")
            )));
        }

        let root_metadata = fs::symlink_metadata(&root).await.map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                DomainError::NotFound(format!(
                    "Workspace path not found: {}",
                    path.map(WorkspacePath::as_str).unwrap_or(".")
                ))
            } else {
                DomainError::InternalError(format!(
                    "Failed to inspect workspace path {}: {}",
                    root.display(),
                    error
                ))
            }
        })?;
        if root_metadata.file_type().is_symlink() {
            return Err(DomainError::InvalidData(format!(
                "Workspace path targets a symlink: {}",
                path.map(WorkspacePath::as_str).unwrap_or(".")
            )));
        }

        let mut entries = Vec::new();
        let mut stack = vec![(root, 0_usize)];
        let mut truncated = false;

        while let Some((dir, level)) = stack.pop() {
            let metadata = fs::metadata(&dir).await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to inspect workspace path {}: {}",
                    dir.display(),
                    error
                ))
            })?;
            if metadata.is_file() {
                entries.push(WorkspaceEntry {
                    path: workspace_path_from_run_dir(&run_dir, &dir)?,
                    kind: WorkspaceEntryKind::File,
                    bytes: Some(metadata.len()),
                });
                continue;
            }
            if !metadata.is_dir() {
                continue;
            }

            let mut children = fs::read_dir(&dir).await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to read workspace directory {}: {}",
                    dir.display(),
                    error
                ))
            })?;
            let mut child_paths = Vec::new();
            while let Some(entry) = children.next_entry().await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to read workspace directory entry {}: {}",
                    dir.display(),
                    error
                ))
            })? {
                child_paths.push(entry.path());
            }
            child_paths.sort();

            for child in child_paths.into_iter().rev() {
                if entries.len() >= max_entries {
                    truncated = true;
                    break;
                }

                let metadata = fs::symlink_metadata(&child).await.map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to inspect workspace path {}: {}",
                        child.display(),
                        error
                    ))
                })?;
                if metadata.file_type().is_symlink() {
                    return Err(DomainError::InvalidData(format!(
                        "Workspace path targets a symlink: {}",
                        child.display()
                    )));
                }

                let path = workspace_path_from_run_dir(&run_dir, &child)?;
                if metadata.is_dir() {
                    entries.push(WorkspaceEntry {
                        path,
                        kind: WorkspaceEntryKind::Directory,
                        bytes: None,
                    });
                    if level < depth {
                        stack.push((child, level + 1));
                    }
                } else if metadata.is_file() {
                    entries.push(WorkspaceEntry {
                        path,
                        kind: WorkspaceEntryKind::File,
                        bytes: Some(metadata.len()),
                    });
                }
            }

            if truncated {
                break;
            }
        }

        entries.sort_by(|a, b| {
            let kind_order = match (&a.kind, &b.kind) {
                (WorkspaceEntryKind::Directory, WorkspaceEntryKind::File) => {
                    std::cmp::Ordering::Less
                }
                (WorkspaceEntryKind::File, WorkspaceEntryKind::Directory) => {
                    std::cmp::Ordering::Greater
                }
                _ => std::cmp::Ordering::Equal,
            };
            kind_order.then_with(|| a.path.as_str().cmp(b.path.as_str()))
        });

        Ok(WorkspaceFileList { entries, truncated })
    }
}

#[async_trait]
impl CheckpointRepository for FileAgentRepository {
    async fn create_checkpoint(
        &self,
        run_id: &str,
        reason: &str,
        event_seq: u64,
        paths: &[WorkspacePath],
    ) -> Result<Checkpoint, DomainError> {
        let _guard = self.checkpoint_lock.lock().await;
        let run_dir = self.load_run_dir(run_id).await?;
        let checkpoints_dir = run_dir.join("checkpoints");
        fs::create_dir_all(&checkpoints_dir)
            .await
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to create checkpoint directory {}: {}",
                    checkpoints_dir.display(),
                    error
                ))
            })?;

        let mut next_seq = 1_u64;
        let mut entries = fs::read_dir(&checkpoints_dir).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read checkpoint directory {}: {}",
                checkpoints_dir.display(),
                error
            ))
        })?;
        while let Some(entry) = entries.next_entry().await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read checkpoint directory entry {}: {}",
                checkpoints_dir.display(),
                error
            ))
        })? {
            if entry
                .file_type()
                .await
                .map(|file_type| file_type.is_dir())
                .unwrap_or(false)
            {
                next_seq += 1;
            }
        }

        let checkpoint_id = format!("cp_{next_seq:06}");
        let checkpoint_dir = checkpoints_dir.join(&checkpoint_id);
        fs::create_dir_all(&checkpoint_dir).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to create checkpoint {}: {}",
                checkpoint_dir.display(),
                error
            ))
        })?;

        let mut files = Vec::new();
        for path in paths {
            let source = self.safe_workspace_path(run_id, path, false).await?;
            let bytes = fs::read(&source).await.map_err(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    DomainError::NotFound(format!(
                        "Required checkpoint file not found: {}",
                        path.as_str()
                    ))
                } else {
                    DomainError::InternalError(format!(
                        "Failed to read checkpoint source {}: {}",
                        source.display(),
                        error
                    ))
                }
            })?;

            let target = checkpoint_dir.join(path.as_str());
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).await.map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to create checkpoint file parent {}: {}",
                        parent.display(),
                        error
                    ))
                })?;
            }
            fs::write(&target, &bytes).await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to write checkpoint file {}: {}",
                    target.display(),
                    error
                ))
            })?;

            files.push(CheckpointFile {
                path: path.as_str().to_string(),
                sha256: sha256_hex(&bytes),
                bytes: bytes.len() as u64,
            });
        }

        let checkpoint = Checkpoint {
            id: checkpoint_id,
            seq: next_seq,
            run_id: run_id.to_string(),
            created_at: Utc::now(),
            reason: reason.to_string(),
            event_seq,
            files,
        };
        Self::write_json_atomic(&checkpoint_dir.join("checkpoint.json"), &checkpoint).await?;

        Ok(checkpoint)
    }
}

fn validate_segment(value: &str, label: &str) -> Result<(), DomainError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        return Err(DomainError::InvalidData(format!(
            "Invalid agent storage segment {label}: {value}"
        )));
    }
    Ok(())
}

fn workspace_file_from_text(
    path: WorkspacePath,
    text: String,
) -> Result<WorkspaceFile, DomainError> {
    let bytes = text.as_bytes().to_vec();
    Ok(WorkspaceFile {
        path,
        text,
        bytes: bytes.len() as u64,
        sha256: sha256_hex(&bytes),
    })
}

fn workspace_path_from_run_dir(
    run_dir: &Path,
    target: &Path,
) -> Result<WorkspacePath, DomainError> {
    let relative = target.strip_prefix(run_dir).map_err(|error| {
        DomainError::InvalidData(format!(
            "Workspace path is outside run directory {}: {}",
            run_dir.display(),
            error
        ))
    })?;
    let value = relative
        .iter()
        .map(|part| part.to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    WorkspacePath::parse(value)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::agent::{
        AgentChatRef, AgentRunStatus, ArtifactSpec, ArtifactTarget, CommitPolicy,
        WorkspaceInputManifest,
    };

    fn temp_root() -> PathBuf {
        std::env::temp_dir().join(format!("tauritavern-agent-repo-{}", Uuid::new_v4()))
    }

    fn sample_run() -> AgentRun {
        AgentRun {
            id: "run_test".to_string(),
            workspace_id: "chat_test".to_string(),
            stable_chat_id: "stable_chat_test".to_string(),
            chat_ref: AgentChatRef::Character {
                character_id: "Seraphina".to_string(),
                file_name: "Seraphina.png".to_string(),
            },
            generation_type: "normal".to_string(),
            profile_id: None,
            status: AgentRunStatus::Created,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn sample_manifest(run: &AgentRun) -> WorkspaceManifest {
        WorkspaceManifest {
            workspace_version: 1,
            run_id: run.id.clone(),
            stable_chat_id: run.stable_chat_id.clone(),
            chat_ref: run.chat_ref.clone(),
            created_at: Utc::now(),
            input: WorkspaceInputManifest {
                mode: "prompt_snapshot".to_string(),
                prompt_snapshot_path: "input/prompt_snapshot.json".to_string(),
            },
            artifacts: vec![ArtifactSpec {
                id: "main".to_string(),
                path: "output/main.md".to_string(),
                kind: "markdown".to_string(),
                target: ArtifactTarget::MessageBody,
                required: true,
                assembly_order: 0,
            }],
            commit_policy: CommitPolicy {
                default_target: ArtifactTarget::MessageBody,
                combine_template: None,
                store_artifacts_in_extra: true,
            },
        }
    }

    #[tokio::test]
    async fn repository_round_trips_run_workspace_event_and_checkpoint() {
        let root = temp_root();
        let repository = FileAgentRepository::new(root.clone());
        let run = sample_run();
        let manifest = sample_manifest(&run);

        repository.create_run(&run).await.expect("create run");
        repository
            .initialize_run(&run, &manifest, &serde_json::json!({"messages": []}))
            .await
            .expect("initialize workspace");

        let path = WorkspacePath::parse("output/main.md").expect("workspace path");
        let written = repository
            .write_text(&run.id, &path, "hello")
            .await
            .expect("write text");
        assert_eq!(written.sha256.len(), 64);

        let event = repository
            .append_event(
                &run.id,
                AgentRunEventLevel::Info,
                "artifact_written",
                Value::Null,
            )
            .await
            .expect("append event");
        assert_eq!(event.seq, 1);

        let events = repository
            .read_events(
                &run.id,
                AgentRunEventReadQuery {
                    after_seq: Some(0),
                    before_seq: None,
                    limit: 10,
                },
            )
            .await
            .expect("read events");
        assert_eq!(events.len(), 1);

        let checkpoint = repository
            .create_checkpoint(&run.id, "test", event.seq, &[path])
            .await
            .expect("checkpoint");
        assert_eq!(checkpoint.files[0].bytes, 5);

        fs::remove_dir_all(root).await.expect("cleanup");
    }
}
