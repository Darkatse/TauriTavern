use std::path::Path;
use std::sync::Arc;

use bashkit::{ExecutionBudget, ExecutionCapability, FileSystem};
use tokio::runtime::Handle;
use tt_domain::errors::DomainError;
use tt_domain::models::agent::WorkspacePath;

use crate::filesystem::MAX_DIRECTORY_ENTRIES;

#[derive(Clone)]
pub(super) struct Files {
    pub fs: Arc<dyn FileSystem>,
    pub runtime: Handle,
    pub budget: ExecutionCapability<ExecutionBudget>,
}

impl Files {
    pub fn check(&self) -> Result<(), String> {
        self.budget
            .try_with(|budget| budget.check())
            .map_err(|error| error.to_string())?
            .map_err(|error| error.to_string())
    }

    pub fn charge_input(&self, bytes: usize) -> Result<(), String> {
        self.budget
            .try_with(|budget| budget.consume_input(bytes))
            .map_err(|error| error.to_string())?
            .map_err(|error| error.to_string())
    }

    pub fn read(&self, path: &str) -> Result<String, String> {
        self.check()?;
        let bytes = self
            .runtime
            .block_on(self.fs.read_file(Path::new(path)))
            .map_err(|error| file_error("read", path, error))?;
        self.charge_input(bytes.len())?;
        String::from_utf8(bytes).map_err(|_| format!("`{path}` is not UTF-8 text."))
    }

    pub fn write(&self, path: &str, text: &str) -> Result<(), String> {
        self.check()?;
        self.runtime.block_on(async {
            let parent = Path::new(path)
                .parent()
                .expect("workspace paths have a root");
            match self.fs.stat(parent).await {
                Ok(metadata) if metadata.file_type.is_dir() => {}
                Ok(_) => return Err(format!("Parent of `{path}` is not a directory.")),
                Err(bashkit::Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                    self.fs
                        .mkdir(parent, true)
                        .await
                        .map_err(|error| file_error("create parent directory for", path, error))?;
                }
                Err(error) => return Err(file_error("inspect parent of", path, error)),
            }
            self.fs
                .write_file(Path::new(path), text.as_bytes())
                .await
                .map_err(|error| file_error("write", path, error))
        })
    }

    pub fn exists(&self, path: &str) -> Result<bool, String> {
        self.check()?;
        self.runtime
            .block_on(self.fs.exists(Path::new(path)))
            .map_err(|error| file_error("inspect", path, error))
    }

    pub fn list(&self, directory: Option<&str>) -> Result<Vec<String>, String> {
        let root = directory.unwrap_or("/");
        let mut directories = vec![root.to_owned()];
        let mut files = Vec::new();
        let mut visited = 0;
        while let Some(path) = directories.pop() {
            self.check()?;
            let entries = self
                .runtime
                .block_on(self.fs.read_dir(Path::new(&path)))
                .map_err(|error| file_error("list", &path, error))?;
            visited += entries.len();
            if visited > MAX_DIRECTORY_ENTRIES {
                return Err(format!(
                    "Directory listing exceeds {MAX_DIRECTORY_ENTRIES} entries; choose a narrower directory."
                ));
            }
            for entry in entries {
                self.charge_input(entry.name.len())?;
                if directory.is_none() {
                    files.push(entry.name);
                    continue;
                }
                let child = format!("{path}/{}", entry.name);
                if entry.metadata.file_type.is_dir() {
                    directories.push(child);
                } else {
                    files.push(child[root.len() + 1..].to_owned());
                }
            }
        }
        files.sort();
        Ok(files)
    }
}

pub(super) fn workspace_path(raw: &str) -> Result<String, String> {
    WorkspacePath::parse(raw)
        .map(|path| format!("/{}", path.as_str()))
        .map_err(|error| error.to_string())
}

/// Resolve POSIX workspace paths independently of the host's absolute-path rules.
pub(super) fn resolve_path(base: &str, raw: &str) -> Result<String, String> {
    if raw.contains(['\\', '\0']) || raw.as_bytes().get(1) == Some(&b':') {
        return Err(format!(
            "Invalid workspace path `{raw}`; use / separators and workspace paths."
        ));
    }
    let joined = if raw.starts_with('/') {
        raw.to_owned()
    } else {
        format!("{base}/{raw}")
    };
    let normalized = bashkit::normalize_path(Path::new(&joined));
    let path = normalized.to_str().ok_or("Workspace path must be UTF-8.")?;
    WorkspacePath::parse(path.trim_start_matches('/')).map_err(|error| error.to_string())?;
    Ok(path.to_owned())
}

fn file_error(operation: &str, path: &str, error: bashkit::Error) -> String {
    if let bashkit::Error::Io(error) = &error {
        if error.kind() == std::io::ErrorKind::NotFound {
            return format!(
                "Cannot {operation} `{path}`: path not found. Check the directory with workspace.listFiles."
            );
        }
        if let Some(source) = error
            .get_ref()
            .and_then(|source| source.downcast_ref::<DomainError>())
        {
            return source.to_string();
        }
    }
    format!("Cannot {operation} `{path}`: {error}")
}
