//! QuickJs script execution engine for Tauri Tavern skills
//! 
//! This crate provides a sandboxed JavaScript execution environment using QuickJs,
//! with restricted file system access and module loading capabilities.

pub mod api;
pub mod engine;
pub mod sandbox;
pub mod tool;

pub use engine::QuickJsEngine;
pub use sandbox::SandboxConfig;
pub use tool::{ScriptToolDescriptor, ScriptToolExecutor};
