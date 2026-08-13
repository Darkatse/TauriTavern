//! Script tool descriptor generation
//! 
//! Creates ToolDescriptor instances for skill scripts.

use tt_domain::models::tool::{ToolDescriptor, ToolId, ToolProviderId};
use serde_json::{json, Value};

/// Helper for creating script tool descriptors
pub struct ScriptToolDescriptor;

impl ScriptToolDescriptor {
    /// Create a tool descriptor for a skill script
    pub fn create(
        skill_id: &str,
        script_name: &str,
        title: Option<String>,
        description: Option<String>,
        input_schema: Option<Value>,
    ) -> anyhow::Result<ToolDescriptor> {
        let provider_id = ToolProviderId::parse("skill-script")?;
        let tool_id = ToolId::new(&provider_id, &format!("{skill_id}.{script_name}"))?;
        
        Ok(ToolDescriptor {
            id: tool_id,
            title: title.or_else(|| Some(script_name.to_string())),
            description,
            input_schema: input_schema.unwrap_or_else(|| json!({
                "type": "object",
                "properties": {},
                "additionalProperties": true
            })),
            output_schema: None,
            annotations: Some(json!({
                "readOnly": false,
                "idempotent": false,
                "destructiveHint": false,
                "openWorldHint": true
            })),
        })
    }

    /// Create a tool descriptor with a simple string argument schema
    pub fn create_with_args(
        skill_id: &str,
        script_name: &str,
        title: Option<String>,
        description: Option<String>,
        args: Vec<(&str, &str, Option<bool>)>, // (name, description, required)
    ) -> anyhow::Result<ToolDescriptor> {
        let mut properties = serde_json::Map::new();
        let mut required = Vec::new();
        
        for (name, desc, req) in args {
            properties.insert(
                name.to_string(),
                json!({
                    "type": "string",
                    "description": desc
                }),
            );
            
            if req.unwrap_or(false) {
                required.push(name.to_string());
            }
        }
        
        let input_schema = json!({
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": true
        });
        
        Self::create(
            skill_id,
            script_name,
            title,
            description,
            Some(input_schema),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_descriptor() {
        let descriptor = ScriptToolDescriptor::create(
            "my-skill",
            "helper",
            Some("Helper Script".to_string()),
            Some("A helper script for doing things".to_string()),
            None,
        ).unwrap();
        
        assert_eq!(descriptor.id.as_str(), "skill-script:my-skill.helper");
        assert_eq!(descriptor.title, Some("Helper Script".to_string()));
    }

    #[test]
    fn test_create_with_args() {
        let descriptor = ScriptToolDescriptor::create_with_args(
            "my-skill",
            "process",
            None,
            Some("Process data").to_string().into(),
            vec![
                ("input", "Input data", Some(true)),
                ("format", "Output format", Some(false)),
            ],
        ).unwrap();
        
        let schema = descriptor.input_schema.as_object().unwrap();
        assert!(schema.get("required").unwrap().as_array().unwrap().contains(&"input".into()));
    }
}
