//! Outbound port：skill 脚本沙箱执行引擎。
//!
//! 应用层经此 port 请求在隔离的 QuickJS 运行时中执行某个 skill 包
//! `scripts/` 目录下的入口脚本；具体沙箱规则由 adapter 实现。

use std::path::PathBuf;

use async_trait::async_trait;
use tt_domain::errors::DomainError;
use tt_domain::models::skill_script::{ActivatedWorldInfoEntry, SillyTavernVariableSnapshot};

/// 一次脚本执行请求。`script_path` 为入口脚本的规范化绝对物理路径，
/// 其 `parent()` 即当前 skill 的 `scripts/` 目录（相对模块加载白名单根）。
pub struct SkillScriptRequest {
    pub script_path: PathBuf,
    pub args: serde_json::Value,
    /// 当前 run 的工作区根目录（动态，随请求绑定）。
    pub work_dir: PathBuf,
    /// 相对 work_dir 的可见根（$fs 读门控）。
    pub visible_roots: Vec<String>,
    /// 相对 work_dir 的可写根（$fs 写门控）。
    pub writable_roots: Vec<String>,
    /// 预取的激活世界书条目快照。
    pub world_info_entries: Vec<ActivatedWorldInfoEntry>,
    /// 预取的 SillyTavern 变量快照（只读），经 `$variables` API 提供。
    pub variables: SillyTavernVariableSnapshot,
}

/// 脚本返回值（JSON）。
#[derive(Debug)]
pub struct SkillScriptResult {
    pub value: serde_json::Value,
}

#[async_trait]
pub trait SkillScriptEngine: Send + Sync {
    /// 执行入口脚本的 `default(args)` 或 `main(args)` 导出并返回其 JSON 结果。
    /// JS 异常、超时与返回值超限分别以 `DomainError::SkillScriptExecutionFailed`
    /// 与 `DomainError::SkillScriptResultTooLarge` 传播。
    async fn execute(&self, request: SkillScriptRequest) -> Result<SkillScriptResult, DomainError>;
}
