use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::{Mutex, Notify, watch};
use tt_domain::errors::DomainError;
use tt_domain::models::agent::WorkspacePath;
use tt_ports::workspace_fs::{
    WorkspaceAppendResult, WorkspaceDirectoryEntry, WorkspaceEntryKind, WorkspaceFile, WorkspaceFs,
    WorkspaceMetadata, WorkspaceWriteGuard,
};
use tt_ports::workspace_shell::{
    WorkspaceShell, WorkspaceShellContext, WorkspaceShellExit, WorkspaceShellRequest,
    WorkspaceShellTools,
};

use crate::WorkspaceShellEngine;

/// A controlled write future: the test decides when the pending side effect completes.
#[derive(Default)]
struct BlockedWorkspace {
    started: Notify,
    release: Notify,
    files: Mutex<HashMap<String, Vec<u8>>>,
}

#[async_trait]
impl WorkspaceFs for BlockedWorkspace {
    async fn metadata(
        &self,
        path: Option<&WorkspacePath>,
    ) -> Result<WorkspaceMetadata, DomainError> {
        let (kind, bytes) = match path {
            None => (WorkspaceEntryKind::Directory, 0),
            Some(path) => {
                let files = self.files.lock().await;
                let bytes = files
                    .get(path.as_str())
                    .ok_or_else(|| DomainError::NotFound(path.as_str().to_string()))?;
                (WorkspaceEntryKind::File, bytes.len() as u64)
            }
        };
        Ok(WorkspaceMetadata {
            kind,
            bytes,
            modified: None,
            created: None,
        })
    }

    async fn write_file(
        &self,
        path: &WorkspacePath,
        bytes: &[u8],
        _: WorkspaceWriteGuard,
    ) -> Result<(), DomainError> {
        if path.as_str() == "draft.md" {
            self.started.notify_one();
            self.release.notified().await;
        }
        self.files
            .lock()
            .await
            .insert(path.as_str().to_string(), bytes.to_vec());
        Ok(())
    }

    async fn read_file(&self, _: &WorkspacePath, _: usize) -> Result<Vec<u8>, DomainError> {
        unreachable!("the cancellation script only writes files")
    }
    async fn append_file(&self, _: &WorkspacePath, _: &[u8]) -> Result<(), DomainError> {
        unreachable!("the cancellation script only replaces files")
    }
    async fn append_text(
        &self,
        _: &WorkspacePath,
        _: &str,
    ) -> Result<WorkspaceAppendResult, DomainError> {
        unreachable!("the shell uses byte writes")
    }
    async fn read_dir(
        &self,
        _: Option<&WorkspacePath>,
        _: usize,
    ) -> Result<Vec<WorkspaceDirectoryEntry>, DomainError> {
        unreachable!("the cancellation script does not list directories")
    }
    async fn create_dir(&self, _: &WorkspacePath, _: bool) -> Result<(), DomainError> {
        unreachable!("the cancellation script does not create directories")
    }
    async fn remove(&self, _: &WorkspacePath, _: bool) -> Result<(), DomainError> {
        unreachable!("the cancellation script does not remove files")
    }
    async fn rename(&self, _: &WorkspacePath, _: &WorkspacePath) -> Result<(), DomainError> {
        unreachable!("the cancellation script does not move files")
    }
    async fn copy_file(&self, _: &WorkspacePath, _: &WorkspacePath) -> Result<(), DomainError> {
        unreachable!("the cancellation script does not copy files")
    }
}

#[tokio::test]
async fn cancellation_finishes_current_write_and_stops_further_writes() {
    for command in [
        r#"python3 -c 'from pathlib import Path; Path("/draft.md").write_text("draft"); Path("/later.md").write_text("later")'"#,
        r#"js -e 'import {workspace} from "@tauritavern/runtime"; workspace.writeText("draft.md", "draft")'; printf later > /later.md"#,
    ] {
        let files = Arc::new(BlockedWorkspace::default());
        let (cancel, receiver) = watch::channel(false);
        let request = WorkspaceShellRequest {
            command: command.to_owned(),
            workdir: "/".to_string(),
            files: files.clone(),
            context: Arc::default(),
            cancel: receiver,
        };
        let mut task = tokio::spawn(async move { WorkspaceShellEngine.execute(request).await });
        tokio::select! {
            _ = files.started.notified() => {}
            result = &mut task => panic!("shell returned before the controlled write: {result:?}"),
        }
        cancel.send(true).unwrap();
        files.release.notify_one();

        let result = task.await.unwrap().unwrap();
        assert_eq!(result.exit, WorkspaceShellExit::Cancelled, "{result:?}");
        let written = files.files.lock().await;
        assert_eq!(written.get("draft.md").unwrap(), b"draft");
        assert!(!written.contains_key("later.md"));
    }
}
/// A minimal in-memory workspace for tests that only need a directory tree.
#[derive(Default)]
struct MemWorkspace {
    dirs: std::sync::Mutex<std::collections::HashSet<String>>,
}

#[async_trait]
impl WorkspaceFs for MemWorkspace {
    async fn metadata(
        &self,
        path: Option<&WorkspacePath>,
    ) -> Result<WorkspaceMetadata, DomainError> {
        let is_dir = match path {
            None => true,
            Some(path) => self.dirs.lock().unwrap().contains(path.as_str()),
        };
        Ok(WorkspaceMetadata {
            kind: if is_dir {
                WorkspaceEntryKind::Directory
            } else {
                WorkspaceEntryKind::File
            },
            bytes: 0,
            modified: None,
            created: None,
        })
    }

    async fn write_file(
        &self,
        _: &WorkspacePath,
        _: &[u8],
        _: WorkspaceWriteGuard,
    ) -> Result<(), DomainError> {
        Ok(())
    }
    async fn read_file(&self, _: &WorkspacePath, _: usize) -> Result<Vec<u8>, DomainError> {
        Ok(Vec::new())
    }
    async fn append_file(&self, _: &WorkspacePath, _: &[u8]) -> Result<(), DomainError> {
        Ok(())
    }
    async fn append_text(
        &self,
        path: &WorkspacePath,
        _: &str,
    ) -> Result<WorkspaceAppendResult, DomainError> {
        Ok(WorkspaceAppendResult {
            file: WorkspaceFile::from_text(path.clone(), String::new()),
            previous_sha256: None,
        })
    }
    async fn read_dir(
        &self,
        _: Option<&WorkspacePath>,
        _: usize,
    ) -> Result<Vec<WorkspaceDirectoryEntry>, DomainError> {
        Ok(Vec::new())
    }
    async fn create_dir(&self, path: &WorkspacePath, _: bool) -> Result<(), DomainError> {
        self.dirs.lock().unwrap().insert(path.as_str().to_string());
        Ok(())
    }
    async fn remove(&self, _: &WorkspacePath, _: bool) -> Result<(), DomainError> {
        Ok(())
    }
    async fn rename(&self, _: &WorkspacePath, _: &WorkspacePath) -> Result<(), DomainError> {
        Ok(())
    }
    async fn copy_file(&self, _: &WorkspacePath, _: &WorkspacePath) -> Result<(), DomainError> {
        Ok(())
    }
}

/// Records what a shell command asked for and answers with a fixed result.
#[derive(Debug, Default)]
struct RecordingTools {
    calls: Mutex<Vec<(String, serde_json::Value)>>,
    visible: Vec<String>,
}

impl RecordingTools {
    fn exposing(names: &[&str]) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            visible: names.iter().map(|name| name.to_string()).collect(),
        }
    }
}

#[async_trait]
impl WorkspaceShellTools for RecordingTools {
    async fn call(&self, name: &str, args: serde_json::Value) -> Result<String, DomainError> {
        self.calls.lock().await.push((name.to_string(), args));
        Ok(format!("result for {name}\n"))
    }

    fn visible(&self) -> &[String] {
        &self.visible
    }
}

async fn run_with_tools(command: &str, tools: Arc<RecordingTools>) -> (String, i32) {
    let context = WorkspaceShellContext {
        frozen_macros: Arc::default(),
        host: Err("test".to_string()),
        tools: Some(tools),
        mcp: None,
    };
    let (_, receiver) = watch::channel(false);
    let result = WorkspaceShellEngine
        .execute(WorkspaceShellRequest {
            command: command.to_owned(),
            workdir: "/".to_string(),
            files: Arc::new(MemWorkspace::default()),
            context: Arc::new(context),
            cancel: receiver,
        })
        .await
        .expect("the shell runs");
    let exit = match result.exit {
        WorkspaceShellExit::Exited(code) => code,
        other => panic!("unexpected exit: {other:?}"),
    };
    (format!("{}{}", result.stdout, result.stderr), exit)
}

#[tokio::test]
async fn a_registered_tool_runs_as_a_command_and_receives_its_json_argument() {
    let tools = Arc::new(RecordingTools::exposing(&["chat.search"]));
    let (output, exit) = run_with_tools(
        r#"builtin.chat.search '{"query":"lantern","limit":10}'"#,
        tools.clone(),
    )
    .await;
    assert_eq!(exit, 0, "{output}");
    assert_eq!(output, "result for chat.search\n");
    let calls = tools.calls.lock().await;
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "chat.search");
    assert_eq!(
        calls[0].1,
        serde_json::json!({"query": "lantern", "limit": 10})
    );
}

#[tokio::test]
async fn a_tool_absent_from_the_visible_set_is_not_a_command() {
    let tools = Arc::new(RecordingTools::exposing(&["chat.search"]));
    let (output, exit) = run_with_tools("builtin.chat.read_messages '{}'", tools.clone()).await;
    assert_ne!(exit, 0, "{output}");
    assert!(output.contains("not found"), "{output}");
    assert!(tools.calls.lock().await.is_empty());
}
