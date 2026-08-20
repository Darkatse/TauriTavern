//! QuickJS 沙箱脚本引擎（`SkillScriptEngine` 的 adapter 实现）。

pub mod api;
pub mod convert;
pub mod engine;
pub mod runtime_module;

pub use engine::{
    QuickJsScriptEngine, DEFAULT_EXECUTION_TIMEOUT, DEFAULT_MAX_RESULT_BYTES,
    DEFAULT_MAX_TOTAL_INPUT_BYTES, DEFAULT_MAX_TOTAL_OUTPUT_BYTES,
};
