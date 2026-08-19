//! Outbound port：skill 脚本沙箱执行引擎。
//!
//! 应用层经此 port 请求在隔离的 QuickJS 运行时中执行某个 skill 包
//! 的入口脚本源码；适配器在内存覆盖层上执行 JS，返回值、写入 delta
//! 与日志，不直接接触物理文件系统。

use std::collections::HashMap;

use async_trait::async_trait;
use tt_domain::errors::DomainError;

/// 一次脚本执行请求。适配器只接触源码字符串与纯 JSON 上下文，
/// 不接收任何物理路径或领域模型类型。
pub struct SkillScriptRequest {
    /// 入口脚本的完整源码（由应用层从 skill 包读取）。
    pub script_source: String,
    /// 入口模块名（仅用于错误消息与模块声明标识，非物理路径）。
    pub script_name: String,
    /// 调用方传入的参数对象。
    pub args: serde_json::Value,
    /// 工作区文件快照：逻辑路径 → 文件文本内容。
    /// 脚本通过 `$fs.readText` 读取这些文件；`$fs.writeText` 写入
    /// 的文件若已存在则覆盖此快照中的值，不存在则新增。
    pub workspace_files: HashMap<String, String>,
    /// 可见根前缀列表（逻辑路径前缀，如 `["output"]`）。
    /// `$fs.readText` / `$fs.listFiles` / `$fs.exists` 仅允许访问
    /// 这些前缀下的路径。
    pub visible_roots: Vec<String>,
    /// 可写根前缀列表（逻辑路径前缀，如 `["output"]`）。
    /// `$fs.writeText` 仅允许写入这些前缀下的路径。
    pub writable_roots: Vec<String>,
    /// 预取的世界书快照（纯 JSON，由应用层从 `ActivatedWorldInfoEntry`
    /// 投影而成），经 `$worldInfo` API 提供给脚本。
    pub world_info: serde_json::Value,
    /// 预取的 SillyTavern 变量快照（纯 JSON），格式为
    /// `{ "local": { ... }, "global": { ... } }`，
    /// 经 `$variables` API 提供给脚本（只读）。
    pub variables: serde_json::Value,
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
    /// 脚本通过 `$fs.writeText` 产生的写入（按调用顺序排列）。
    /// 应用层负责通过 `write_text_guarded` 落盘。
    pub writes: Vec<SkillScriptWrite>,
    /// 脚本通过 `$log` 产生的日志条目。
    pub logs: Vec<SkillScriptLog>,
}

#[async_trait]
pub trait SkillScriptEngine: Send + Sync {
    /// 执行入口脚本的 `default(args)` 或 `main(args)` 导出并返回
    /// `{value, writes, logs}`。JS 异常、超时与返回值超限分别以
    /// `DomainError::SkillScriptExecutionFailed` 与
    /// `DomainError::SkillScriptResultTooLarge` 传播。
    async fn execute(&self, request: SkillScriptRequest) -> Result<SkillScriptResult, DomainError>;
}
