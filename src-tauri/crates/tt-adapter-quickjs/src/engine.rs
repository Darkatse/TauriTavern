//! QuickJs engine core implementation
//! 
//! Provides the main script execution engine with sandboxing and API injection.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use rquickjs::{Context, Runtime, Ctx, Module, Async};
use anyhow::{Context as AnyhowContext, Result, bail};
use serde_json::Value;
use crate::sandbox::SandboxConfig;
use crate::api::{FsApi, WorldInfoApi, LogApi, ActivatedWorldInfoEntry};

/// Main QuickJs execution engine
pub struct QuickJsEngine {
    runtime: Runtime,
    context: Arc<Context>,
    sandbox: SandboxConfig,
}

impl QuickJsEngine {
    /// Create a new QuickJs engine with the given sandbox configuration
    pub fn new(sandbox: SandboxConfig) -> Result<Self> {
        let runtime = Runtime::new()?;
        
        // Configure memory limits for safety
        runtime.set_memory_limit(1024 * 1024 * 32)?; // 32MB limit
        runtime.set_max_stack_size(1024 * 256)?; // 256KB stack
        
        let context = Arc::new(Context::full(&runtime)?);
        
        Ok(Self {
            runtime,
            context,
            sandbox,
        })
    }

    /// Get the sandbox configuration
    pub fn sandbox(&self) -> &SandboxConfig {
        &self.sandbox
    }

    /// Execute a script file with the given arguments
    pub async fn execute_script(
        &self,
        script_path: &Path,
        args: &Value,
        world_info_entries: Vec<ActivatedWorldInfoEntry>,
    ) -> Result<Value> {
        // Verify script path is allowed
        if !self.sandbox.is_module_load_allowed(script_path) {
            bail!("Script path not allowed: {:?}", script_path);
        }

        let ctx = self.context.clone();
        let script_path = script_path.to_path_buf();
        let args = args.clone();
        
        // Run in async context
        let result = ctx.with(|ctx| {
            // Register APIs
            let fs_api = FsApi::new(self.sandbox.clone());
            fs_api.register(&ctx)?;
            
            let wi_api = WorldInfoApi::new(world_info_entries);
            wi_api.register(&ctx)?;
            
            let log_api = LogApi::new();
            log_api.register(&ctx)?;
            
            // Load and execute the script
            let script_path_str = script_path
                .to_string_lossy()
                .to_string();
            
            // Use Module::import to load the script
            let module = unsafe {
                Module::import(&ctx, script_path_str.clone())
                    .await
                    .map_err(|e| rquickjs::Error::Exception)?
            };
            
            // Call the default export function if it exists, or look for a main function
            let result_value = if let Ok(default) = module.get::<_, rquickjs::Function>("default") {
                // Convert args to JS values
                let js_args = json_to_js(&ctx, &args)?;
                default.call((js_args,))?
            } else if let Ok(main) = module.get::<_, rquickjs::Function>("main") {
                let js_args = json_to_js(&ctx, &args)?;
                main.call((js_args,))?
            } else {
                // If no default or main, just return the module exports
                rquickjs::Value::undefined(ctx.clone())
            };
            
            // Convert result back to JSON
            js_to_json(&result_value)
        })
        .await?;
        
        Ok(result)
    }

    /// Execute a script string (for inline scripts)
    pub async fn execute_string(
        &self,
        script_code: &str,
        args: &Value,
        world_info_entries: Vec<ActivatedWorldInfoEntry>,
    ) -> Result<Value> {
        let ctx = self.context.clone();
        let script_code = script_code.to_string();
        let args = args.clone();
        
        let result = ctx.with(|ctx| {
            // Register APIs
            let fs_api = FsApi::new(self.sandbox.clone());
            fs_api.register(&ctx)?;
            
            let wi_api = WorldInfoApi::new(world_info_entries);
            wi_api.register(&ctx)?;
            
            let log_api = LogApi::new();
            log_api.register(&ctx)?;
            
            // Evaluate the script
            let module = Module::evaluate(ctx.clone(), "inline", script_code)?;
            
            // Call default or main function
            let result_value = if let Ok(default) = module.get::<_, rquickjs::Function>("default") {
                let js_args = json_to_js(&ctx, &args)?;
                default.call((js_args,))?
            } else if let Ok(main) = module.get::<_, rquickjs::Function>("main") {
                let js_args = json_to_js(&ctx, &args)?;
                main.call((js_args,))?
            } else {
                rquickjs::Value::undefined(ctx.clone())
            };
            
            js_to_json(&result_value)
        })
        .await?;
        
        Ok(result)
    }
}

/// Convert JSON Value to QuickJs Value
fn json_to_js<'js>(ctx: &Ctx<'js>, value: &Value) -> Result<rquickjs::Value<'js>> {
    match value {
        Value::Null => Ok(rquickjs::Value::undefined(ctx.clone())),
        Value::Bool(b) => Ok(rquickjs::Value::from_bool(ctx.clone(), *b)),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(rquickjs::Value::from_i64(ctx.clone(), i))
            } else if let Some(f) = n.as_f64() {
                Ok(rquickjs::Value::from_f64(ctx.clone(), f))
            } else {
                Ok(rquickjs::Value::from_f64(ctx.clone(), 0.0))
            }
        }
        Value::String(s) => Ok(rquickjs::Value::from_string(ctx.clone(), s.as_str())?),
        Value::Array(arr) => {
            let js_arr = rquickjs::Array::new(ctx.clone())?;
            for (i, item) in arr.iter().enumerate() {
                js_arr.set(i, json_to_js(ctx, item)?)?;
            }
            Ok(rquickjs::Value::from_array(js_arr))
        }
        Value::Object(obj) => {
            let js_obj = rquickjs::Object::new(ctx.clone())?;
            for (k, v) in obj.iter() {
                js_obj.set(k, json_to_js(ctx, v)?)?;
            }
            Ok(rquickjs::Value::from_object(js_obj))
        }
    }
}

/// Convert QuickJs Value to JSON Value
fn js_to_json(value: &rquickjs::Value) -> Result<Value> {
    if value.is_undefined() || value.is_null() {
        Ok(Value::Null)
    } else if let Some(b) = value.as_bool() {
        Ok(Value::Bool(b))
    } else if let Some(i) = value.as_int() {
        Ok(Value::Number(serde_json::Number::from(i)))
    } else if let Some(f) = value.as_float() {
        Ok(serde_json::Number::from_f64(f)
            .map(Value::Number)
            .unwrap_or(Value::Null))
    } else if let Some(s) = value.as_string() {
        Ok(Value::String(s.to_string()?))
    } else if let Some(arr) = value.as_array() {
        let mut vec = Vec::new();
        for item in arr.iter::<rquickjs::Value>() {
            vec.push(js_to_json(&item?)?);
        }
        Ok(Value::Array(vec))
    } else if let Some(obj) = value.as_object() {
        let mut map = serde_json::Map::new();
        for prop in obj.props::<rquickjs::String, rquickjs::Value>() {
            let (key, val) = prop?;
            map.insert(key.to_string()?, js_to_json(&val)?);
        }
        Ok(Value::Object(map))
    } else {
        Ok(Value::Null)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use std::fs;

    #[tokio::test]
    async fn test_basic_execution() {
        let temp = TempDir::new().unwrap();
        let work_dir = temp.path().join("work");
        fs::create_dir_all(&work_dir).unwrap();
        
        let sandbox = SandboxConfig::new(work_dir, vec![], vec![]);
        let engine = QuickJsEngine::new(sandbox).unwrap();
        
        let result = engine.execute_string(
            "export default function(args) { return args.value + 1; }",
            &json!({"value": 41}),
            vec![],
        ).await.unwrap();
        
        assert_eq!(result, json!(42));
    }
}
