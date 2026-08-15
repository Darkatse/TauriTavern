//! QuickJS 沙箱脚本引擎（`SkillScriptEngine` 的 adapter 实现）。

pub mod api;
pub mod convert;
pub mod engine;
pub mod sandbox;

pub use engine::{DEFAULT_EXECUTION_TIMEOUT, DEFAULT_MAX_RESULT_BYTES, QuickJsScriptEngine};
pub use sandbox::SandboxIoPolicy;
