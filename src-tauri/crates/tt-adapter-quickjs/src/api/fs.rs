//! File system API for scripts
//! 
//! Provides restricted file system operations that only work within the agent's work directory.

use std::path::PathBuf;
use rquickjs::{Ctx, Result, Object, Function, Async};
use serde::{Deserialize, Serialize};
use tokio::fs;
use crate::sandbox::SandboxConfig;

/// File entry information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: String,
    pub kind: String, // "file" or "dir"
}

/// File system API exposed to scripts as $fs
pub struct FsApi {
    sandbox: SandboxConfig,
}

impl FsApi {
    pub fn new(sandbox: SandboxConfig) -> Self {
        Self { sandbox }
    }

    /// Read text content from a file (relative to work_dir)
    pub async fn read_text(&self, path: String) -> anyhow::Result<String> {
        let resolved = self.sandbox.resolve_work_path(&path)?;
        
        if !self.sandbox.is_path_allowed_for_io(&resolved) {
            anyhow::bail!("Access denied: path outside work directory");
        }
        
        fs::read_to_string(&resolved)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to read file {}: {}", path, e))
    }

    /// Write text content to a file (relative to work_dir)
    pub async fn write_text(&self, path: String, content: String) -> anyhow::Result<()> {
        let resolved = self.sandbox.resolve_work_path(&path)?;
        
        if !self.sandbox.is_path_allowed_for_io(&resolved) {
            anyhow::bail!("Access denied: path outside work directory");
        }
        
        // Ensure parent directory exists
        if let Some(parent) = resolved.parent() {
            fs::create_dir_all(parent).await?;
        }
        
        fs::write(&resolved, content)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to write file {}: {}", path, e))
    }

    /// List files in a directory (relative to work_dir)
    pub async fn list_files(&self, path: Option<String>) -> anyhow::Result<Vec<FileEntry>> {
        let base_path = match path {
            Some(p) => self.sandbox.resolve_work_path(&p)?,
            None => self.sandbox.work_dir.clone(),
        };
        
        if !self.sandbox.is_path_allowed_for_io(&base_path) {
            anyhow::bail!("Access denied: path outside work directory");
        }
        
        let mut entries = Vec::new();
        let mut dir = fs::read_dir(&base_path).await?;
        
        while let Some(entry) = dir.next_entry().await? {
            let file_type = entry.file_type().await?;
            let entry_path = entry.path();
            
            // Get relative path from work_dir
            let relative_path = entry_path
                .strip_prefix(&self.sandbox.work_dir)
                .unwrap_or(&entry_path)
                .to_string_lossy()
                .to_string();
            
            let kind = if file_type.is_dir() {
                "dir".to_string()
            } else {
                "file".to_string()
            };
            
            entries.push(FileEntry {
                path: relative_path,
                kind,
            });
        }
        
        Ok(entries)
    }

    /// Check if a path exists (relative to work_dir)
    pub async fn exists(&self, path: String) -> anyhow::Result<bool> {
        let resolved = self.sandbox.resolve_work_path(&path)?;
        
        if !self.sandbox.is_path_allowed_for_io(&resolved) {
            return Ok(false);
        }
        
        Ok(fs::try_exists(&resolved).await.unwrap_or(false))
    }

    /// Register the $fs API object in the QuickJs context
    pub fn register<'js>(&self, ctx: &Ctx<'js>) -> Result<()> {
        let globals = ctx.globals();
        
        let fs_obj = Object::new(ctx.clone())?;
        
        // Register async functions
        let read_text = Function::new(ctx.clone(), move |path: String| {
            let api = FsApi::new(self.sandbox.clone());
            async move { api.read_text(path).await }
        })?;
        
        let write_text = Function::new(ctx.clone(), move |path: String, content: String| {
            let api = FsApi::new(self.sandbox.clone());
            async move { api.write_text(path, content).await }
        })?;
        
        let list_files = Function::new(ctx.clone(), move |path: Option<String>| {
            let api = FsApi::new(self.sandbox.clone());
            async move { api.list_files(path).await }
        })?;
        
        let exists = Function::new(ctx.clone(), move |path: String| {
            let api = FsApi::new(self.sandbox.clone());
            async move { api.exists(path).await }
        })?;
        
        fs_obj.set("readText", read_text)?;
        fs_obj.set("writeText", write_text)?;
        fs_obj.set("listFiles", list_files)?;
        fs_obj.set("exists", exists)?;
        
        globals.set("$fs", fs_obj)?;
        
        Ok(())
    }
}
