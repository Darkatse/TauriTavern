//! Outbound port：skill 脚本沙箱执行引擎。
//!
//! 应用层经此 port 请求在隔离的 QuickJS 运行时中执行某个 skill 包
//! 的入口脚本源码；适配器在内存覆盖层上执行 JS，返回值、写入 delta
//! 与日志，不直接接触物理文件系统。

use std::collections::HashMap;

use async_trait::async_trait;
use thiserror::Error;

/// Skill 脚本引擎错误。port 层独立错误类型，不依赖 `DomainError`，
/// 由 Application 映射为对外错误。
#[derive(Error, Debug)]
pub enum SkillScriptEngineError {
    /// JS 异常、模块声明/求值失败、API 调用失败、超时等执行错误。
    /// 消息包含 JS 异常 message + stack（如可用）。
    #[error("Skill script execution failed: {message}")]
    ExecutionFailed { message: String },

    /// 返回值序列化后超过引擎限制。
    #[error("Skill script result is {actual_bytes} bytes, exceeding the {limit_bytes}-byte limit")]
    ResultTooLarge {
        actual_bytes: usize,
        limit_bytes: usize,
    },

    /// 引擎内部故障（Runtime 创建失败、spawn_blocking panic 等）。
    #[error("Skill script engine internal error: {0}")]
    Internal(String),
}

/// 一次脚本执行请求。适配器只接触逻辑模块名、源码字符串与纯 JSON 上下文，
/// 冻结宏仅携带内存替换值，不接收任何物理路径或宿主对象。
pub struct SkillScriptRequest {
    pub frozen_macros: std::sync::Arc<tt_domain::frozen_macros::FrozenMacros>,
    /// 入口模块的逻辑名（如 `scripts/main.js`），必须存在于 `modules` 中。
    pub entry_module: String,
    /// 内存模块快照：逻辑模块名 → 模块源码（含入口模块）。
    /// 相对导入（`./x.js`、`../x.js`）按 importer 的逻辑模块名规范化解析，
    /// 且只能命中这张 map；内嵌工具箱由引擎单独提供。
    pub modules: HashMap<String, String>,
    /// 调用方传入的参数对象。
    pub args: serde_json::Value,
    /// 工作区文件快照：逻辑路径 → 文件文本内容。
    /// 脚本通过 `workspace.readText` 读取这些文件；`workspace.writeText` 写入
    /// 的文件若已存在则覆盖此快照中的值，不存在则新增。
    pub workspace_files: HashMap<String, String>,
    /// 可见根前缀列表（逻辑路径前缀，如 `["output"]`）。
    /// `workspace.readText` / `workspace.listFiles` / `workspace.exists` 仅允许访问
    /// 这些前缀下的路径。
    pub visible_roots: Vec<String>,
    /// 可写根前缀列表（逻辑路径前缀，如 `["output"]`）。
    /// `workspace.writeText` 仅允许写入这些前缀下的路径。
    pub writable_roots: Vec<String>,
    /// Application 为本次执行投影的只读宿主上下文。
    /// 引擎只负责把这份普通 JSON 映射为 `context`，不解释平台字段。
    pub context: serde_json::Value,
}

/// 脚本写入操作（内存 delta 的一部分）。
#[derive(Debug, Clone)]
pub struct SkillScriptWrite {
    /// 逻辑工作区路径（如 `output/result.txt`）。
    pub path: String,
    /// 写入的文本内容。
    pub text: String,
}

/// 脚本日志条目。
#[derive(Debug, Clone)]
pub struct SkillScriptLog {
    pub level: SkillScriptLogLevel,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillScriptLogLevel {
    Info,
    Warn,
    Error,
    Debug,
}

/// 脚本执行结果：返回值 + 写入 delta + 日志。
#[derive(Debug)]
pub struct SkillScriptResult {
    /// 脚本 `default(args)` 或 `main(args)` 导出的 JSON 返回值。
    pub value: serde_json::Value,
    /// 脚本通过 `workspace.writeText` 产生的写入（按路径排序的最终 delta，
    /// 同一路径仅保留最终内容）。应用层负责通过 `write_text_guarded` 落盘。
    pub writes: Vec<SkillScriptWrite>,
    /// 脚本最后一次调用 `workspace.writeText` 的逻辑路径。
    /// 与按路径排序的最终 delta 分开保存，供 Application 保留 mutation 顺序语义。
    pub last_write_path: Option<String>,
    /// 脚本通过 `log` 产生的日志条目。
    pub logs: Vec<SkillScriptLog>,
}

#[async_trait]
pub trait SkillScriptEngine: Send + Sync {
    /// 执行入口脚本的 `default(args)` 或 `main(args)` 导出并返回
    /// `{value, writes, logs}`。JS 异常与超时以 `ExecutionFailed`、
    /// 返回值超限以 `ResultTooLarge` 传播。
    async fn execute(
        &self,
        request: SkillScriptRequest,
    ) -> Result<SkillScriptResult, SkillScriptEngineError>;
}
