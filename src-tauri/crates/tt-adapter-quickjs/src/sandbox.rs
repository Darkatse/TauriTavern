//! Sandbox configuration for restricting script access
//! 
//! This module provides path validation to ensure scripts can only:
//! - Read/write files within the agent's work directory
//! - Load modules from public library directories and skill script directories

use std::path::{Path, PathBuf};
use anyhow::{Context, Result};

/// Configuration for the script sandbox environment
#[derive(Debug, Clone)]
pub struct SandboxConfig {
    /// Agent's working directory - scripts can read/write anywhere within this directory
    pub work_dir: PathBuf,
    /// Public library directories where scripts can load modules from
    pub public_lib_dirs: Vec<PathBuf>,
    /// Skill script directories where scripts can load modules from
    pub skill_script_dirs: Vec<PathBuf>,
}

impl SandboxConfig {
    pub fn new(
        work_dir: PathBuf,
        public_lib_dirs: Vec<PathBuf>,
        skill_script_dirs: Vec<PathBuf>,
    ) -> Self {
        Self {
            work_dir,
            public_lib_dirs,
            skill_script_dirs,
        }
    }

    /// Check if a path is allowed for file read/write operations
    /// Only paths within the work directory are allowed
    pub fn is_path_allowed_for_io(&self, path: &Path) -> bool {
        let resolved = self.resolve_safe(path);
        resolved.starts_with(&self.work_dir)
    }

    /// Check if a module path is allowed for loading
    /// Only modules from public lib dirs and skill script dirs are allowed
    pub fn is_module_load_allowed(&self, module_path: &Path) -> bool {
        let resolved = self.resolve_safe(module_path);
        
        // Check public lib directories
        for lib_dir in &self.public_lib_dirs {
            if resolved.starts_with(lib_dir) {
                return true;
            }
        }
        
        // Check skill script directories
        for script_dir in &self.skill_script_dirs {
            if resolved.starts_with(script_dir) {
                return true;
            }
        }
        
        false
    }

    /// Resolve a path safely, preventing directory traversal attacks
    /// Returns the canonical absolute path
    pub fn resolve_safe(&self, path: &Path) -> PathBuf {
        // First clean the path to remove .. and . components
        let cleaned = path_clean::clean(path);
        
        // If it's relative, make it absolute relative to work_dir
        if cleaned.is_relative() {
            self.work_dir.join(cleaned)
        } else {
            cleaned
        }
    }

    /// Get the absolute path for a relative path within work_dir
    pub fn resolve_work_path(&self, path: &str) -> Result<PathBuf> {
        let parsed = Path::new(path);
        let resolved = self.resolve_safe(parsed);
        
        // Verify the resolved path is still within work_dir
        if !resolved.starts_with(&self.work_dir) {
            anyhow::bail!("Path escapes work directory: {}", path);
        }
        
        Ok(resolved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_work_dir_access() {
        let temp = TempDir::new().unwrap();
        let work_dir = temp.path().to_path_buf();
        let config = SandboxConfig::new(work_dir.clone(), vec![], vec![]);

        // Should allow access within work_dir
        assert!(config.is_path_allowed_for_io(Path::new("file.txt")));
        assert!(config.is_path_allowed_for_io(Path::new("subdir/file.txt")));
        
        // Should deny access outside work_dir
        assert!(!config.is_path_allowed_for_io(Path::new("/etc/passwd")));
        assert!(!config.is_path_allowed_for_io(Path::new("../other/file.txt")));
    }

    #[test]
    fn test_module_load_allowed() {
        let temp = TempDir::new().unwrap();
        let work_dir = temp.path().join("work");
        let public_lib = temp.path().join("public_lib");
        let skill_scripts = temp.path().join("skills/skill1/scripts");
        
        std::fs::create_dir_all(&work_dir).unwrap();
        std::fs::create_dir_all(&public_lib).unwrap();
        std::fs::create_dir_all(&skill_scripts).unwrap();
        
        let config = SandboxConfig::new(
            work_dir,
            vec![public_lib.clone()],
            vec![skill_scripts.clone()],
        );

        // Should allow loading from public lib
        assert!(config.is_module_load_allowed(&public_lib.join("utils.js")));
        
        // Should allow loading from skill scripts
        assert!(config.is_module_load_allowed(&skill_scripts.join("helper.js")));
        
        // Should deny loading from other locations
        assert!(!config.is_module_load_allowed(&temp.path().join("malicious.js")));
    }
}
