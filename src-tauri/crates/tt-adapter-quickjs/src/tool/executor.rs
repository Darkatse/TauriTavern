//! Script tool executor
//! 
//! Executes skill scripts as tools.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use serde_json::{json, Value};
use anyhow::{Context, Result};
use crate::engine::QuickJsEngine;
use crate::api::ActivatedWorldInfoEntry;
use tt_domain::models::agent::AgentToolResult;

/// Executor for script-based tools
pub struct ScriptToolExecutor {
    engine: Arc<QuickJsEngine>,
    work_dir: PathBuf,
}

impl ScriptToolExecutor {
    pub fn new(engine: Arc<QuickJsEngine>, work_dir: PathBuf) -> Self {
        Self { engine, work_dir }
    }

    /// Execute a script tool call
    pub async fn execute(
        &self,
        skill_id: &str,
        script_name: &str,
        args: &Value,
        world_info_entries: Vec<ActivatedWorldInfoEntry>,
    ) -> AgentToolResult {
        // Construct the script path
        let script_path = self.work_dir
            .join("skills")
            .join(skill_id)
            .join("scripts")
            .join(format!("{script_name}.js"));
        
        // Verify the script exists
        if !tokio::fs::try_exists(&script_path).await.unwrap_or(false) {
            return AgentToolResult {
                call_id: String::new(),
                tool_id: tt_domain::models::tool::ToolId::parse("skill-script:unknown").unwrap_or_else(|_| tt_domain::models::tool::ToolId::new(&tt_domain::models::tool::ToolProviderId::parse("skill-script").unwrap(), "unknown").unwrap()),
                content: format!("Script not found: {}.{}", skill_id, script_name),
                structured: None,
                success: false,
            };
        }
        
        // Execute the script
        match self.engine.execute_script(&script_path, args, world_info_entries).await {
            Ok(result) => AgentToolResult {
                call_id: String::new(),
                tool_id: tt_domain::models::tool::ToolId::parse(&format!("skill-script:{}.{}", skill_id, script_name)).unwrap_or_else(|_| tt_domain::models::tool::ToolId::new(&tt_domain::models::tool::ToolProviderId::parse("skill-script").unwrap(), &format!("{skill_id}.{script_name}")).unwrap()),
                content: result.to_string(),
                structured: Some(result),
                success: true,
            },
            Err(e) => AgentToolResult {
                call_id: String::new(),
                tool_id: tt_domain::models::tool::ToolId::parse(&format!("skill-script:{}.{}", skill_id, script_name)).unwrap_or_else(|_| tt_domain::models::tool::ToolId::new(&tt_domain::models::tool::ToolProviderId::parse("skill-script").unwrap(), &format!("{skill_id}.{script_name}")).unwrap()),
                content: e.to_string(),
                structured: None,
                success: false,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use std::fs;

    #[tokio::test]
    async fn test_executor_basic() {
        let temp = TempDir::new().unwrap();
        let work_dir = temp.path().join("work");
        let scripts_dir = work_dir.join("skills/test-skill/scripts");
        fs::create_dir_all(&scripts_dir).unwrap();
        
        // Write a test script
        let script_content = r#"
export default function(args) {
    return { result: args.message + " processed" };
}
"#;
        fs::write(scripts_dir.join("test.js"), script_content).unwrap();
        
        let sandbox = crate::sandbox::SandboxConfig::new(
            work_dir.clone(),
            vec![work_dir.join("public_lib")],
            vec![scripts_dir.clone()],
        );
        let engine = Arc::new(QuickJsEngine::new(sandbox).unwrap());
        let executor = ScriptToolExecutor::new(engine, work_dir);
        
        let result = executor.execute(
            "test-skill",
            "test",
            &json!({"message": "hello"}),
            vec![],
        ).await;
        
        assert!(result.success);
        assert!(result.structured.is_some());
    }
}
