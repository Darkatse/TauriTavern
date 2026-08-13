//! World Info API for scripts
//! 
//! Provides access to activated world info entries.

use rquickjs::{Ctx, Result, Object, Function};
use serde::{Deserialize, Serialize};

/// World info entry data exposed to scripts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptWorldInfoEntry {
    pub uid: String,
    #[serde(rename = "ref")]
    pub ref_key: String,
    pub content: String,
    pub constant: bool,
    pub position: Option<String>,
    #[serde(rename = "displayName", skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    pub world: String,
}

/// World info read result
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldInfoReadResult {
    pub entries: Vec<ScriptWorldInfoEntry>,
}

/// Raw activated entry from the snapshot
#[derive(Debug, Clone)]
pub struct ActivatedWorldInfoEntry {
    pub world: String,
    pub uid: String,
    pub display_name: Option<String>,
    pub constant: bool,
    pub position: Option<String>,
    pub content: String,
    pub ref_key: String,
}

impl ActivatedWorldInfoEntry {
    /// Parse an activated entry from a JSON value (matching read_activated.rs format)
    pub fn from_value(index: usize, value: &serde_json::Value) -> Option<Self> {
        let obj = value.as_object()?;
        
        let world = obj.get("world")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
            
        let uid = obj.get("uid")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
            
        let ref_key = if world.is_empty() || uid.is_empty() {
            format!("worldinfo:activated#{index}")
        } else {
            format!("worldinfo:{world}#{uid}")
        };
        
        let content = obj.get("content")
            .and_then(|v| v.as_str())?
            .to_string();
        
        Some(Self {
            display_name: obj.get("displayName").and_then(|v| v.as_str()).map(String::from),
            constant: obj.get("constant").and_then(|v| v.as_bool()).unwrap_or(false),
            position: obj.get("position").and_then(|v| v.as_str()).map(String::from),
            world,
            uid,
            content,
            ref_key,
        })
    }
    
    pub fn to_script_entry(&self) -> ScriptWorldInfoEntry {
        ScriptWorldInfoEntry {
            uid: self.uid.clone(),
            ref_key: self.ref_key.clone(),
            content: self.content.clone(),
            constant: self.constant,
            position: self.position.clone(),
            display_name: self.display_name.clone(),
            world: self.world.clone(),
        }
    }
}

/// World Info API exposed to scripts as $worldInfo
pub struct WorldInfoApi {
    /// Pre-fetched activated world info entries
    activated_entries: Vec<ActivatedWorldInfoEntry>,
}

impl WorldInfoApi {
    pub fn new(activated_entries: Vec<ActivatedWorldInfoEntry>) -> Self {
        Self { activated_entries }
    }

    /// Read all activated world info entries
    pub fn read_activated(&self) -> WorldInfoReadResult {
        let entries = self.activated_entries
            .iter()
            .map(|e| e.to_script_entry())
            .collect();
        
        WorldInfoReadResult { entries }
    }

    /// Read specific entries by their ref keys
    pub fn read_entries(&self, refs: Vec<String>) -> WorldInfoReadResult {
        let entries = self.activated_entries
            .iter()
            .filter(|e| refs.contains(&e.ref_key))
            .map(|e| e.to_script_entry())
            .collect();
        
        WorldInfoReadResult { entries }
    }

    /// Register the $worldInfo API object in the QuickJs context
    pub fn register<'js>(&self, ctx: &Ctx<'js>) -> Result<()> {
        let globals = ctx.globals();
        
        let wi_obj = Object::new(ctx.clone())?;
        
        // Create closures that capture self
        let activated_entries = self.activated_entries.clone();
        let read_activated = Function::new(ctx.clone(), move || {
            let api = WorldInfoApi::new(activated_entries.clone());
            Ok::<_, rquickjs::Error>(api.read_activated())
        })?;
        
        let all_entries = self.activated_entries.clone();
        let read_entries = Function::new(ctx.clone(), move |refs: Vec<String>| {
            let api = WorldInfoApi::new(all_entries.clone());
            Ok::<_, rquickjs::Error>(api.read_entries(refs))
        })?;
        
        wi_obj.set("readActivated", read_activated)?;
        wi_obj.set("readEntries", read_entries)?;
        
        globals.set("$worldInfo", wi_obj)?;
        
        Ok(())
    }
}
