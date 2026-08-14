# skill.script 工具设计

- 日期：2026-08-14
- 状态：已批准（设计阶段）
- 范围：为 Agent 工具体系新增 `skill.script` builtin 工具，使各 skill 的 `scripts/` 目录下的 JS 脚本可在 QuickJS 沙箱内被模型执行

## 1. 背景与目标

TauriTavern 的 skill 包支持携带 `scripts/` 目录（安装索引以 `has_scripts` 标记），但当前只存储不执行（见 `file_skill_repository/package.rs` 安装警告）。已有 `tt-adapter-quickjs` crate 提供了 QuickJS 引擎、沙箱配置与执行器雏形，但：

- 未被任何 crate 依赖，未注册进 `BuiltinAgentToolRegistry`，无 dispatch 分支；
- 生成的 `skill-script:` provider ToolId 与 dispatcher 只认 `builtin:` 的体系不匹配；
- executor 直接构造领域类型且字段形状过时（`success` vs 实际的 `is_error` 等）；
- 缺少超时、标识符校验、执行隔离等安全机制。

本设计将其以方案 A（`SkillRepository` 暴露脚本路径 + 独立 `SkillScriptEngine` port）接入现有 Agent 工具体系。

## 2. 已确认的约束与决策

| # | 决策 | 内容 |
|---|------|------|
| D1 | 可执行范围 | 只能执行 skills 包内 `scripts/` 目录下的 `.js` 脚本 |
| D2 | 模块加载白名单 | 仅当前被执行脚本所在 skill 的 `scripts/` 目录 + 公共 libs 目录 |
| D3 | 文件读写 | `$fs` 只能读写当前 agent run 的 workdir（动态创建），按 profile 的 workspace visible/writable roots 门控 |
| D4 | 脚本发现 | 零发现机制：SKILL.md 正文即说明书，模型直接发起调用；脚本不存在则 fail-fast |
| D5 | 启用方式 | profile 显式 `tools.allow` 包含 `skill.script` 才可用；且只能执行可见（visible 且未 deny）skill 的脚本 |
| D6 | 公共 libs 目录 | `{data_root}/skill-libs/`（第一期只定目录约定，不建管理功能） |
| D7 | 执行超时 | 默认 30 秒，interrupt handler 强制中断 |
| D8 | 前端 | 第一期零前端改动，结果走现有 agent run 事件流 |

## 3. 工具契约

单一 builtin 工具，native name `skill.script`，注册进 `BuiltinAgentToolRegistry`（第 20 个），model alias 自动派生为 `skill_script`。

参数 schema：

```json
{
  "type": "object",
  "properties": {
    "skill":  { "type": "string", "description": "Exact skill name from skill_list." },
    "script": { "type": "string", "description": "Script file name under the skill's scripts/ directory, without .js extension." },
    "args":   { "type": "object", "description": "Arguments object passed to the script.", "additionalProperties": true }
  },
  "required": ["skill", "script"]
}
```

`args` 缺省为 `{}`。结果遵循现有呈现约定：`content` 为文本呈现，`structured` 为脚本返回的 JSON 值。

## 4. 执行流程

```
模型调用 skill.script(skill, script, args)
  → AgentToolDispatcher 匹配 SKILL_SCRIPT
  → tt-application agent_tools/skill/script.rs 处理器
      1. 校验 script 标识符格式：^[a-z0-9][a-z0-9-]*$（fail-fast，防路径穿越）
      2. 从 session.effective_skills() 定位 skill（取 scope）；未命中 → 报错
      3. skill_is_visible(policy, name) 门控（复用现有函数）
      4. SkillRepository::skill_file_path(scope, name, "scripts/{script}.js") → 物理路径
      5. 解析当前 run workdir 物理路径 + profile.workspace 的 visible/writable roots
      6. （可选）从 run prompt snapshot 预取激活世界书条目
      7. 调用 SkillScriptEngine::execute
  → tt-adapter-quickjs：沙箱内执行，返回 JSON
  → 处理器组装 AgentToolResult（call_id / tool_id 来自 ToolInvocation）
```

## 5. Port 定义（tt-ports）

`SkillRepository` 新增方法（实现留在 `file_skill_repository`，含路径规范化与存在性校验）：

```rust
async fn skill_file_path(
    &self,
    scope: SkillScope,
    name: &str,
    relative_path: &str,
) -> Result<PathBuf, DomainError>;
```

新增 `SkillScriptEngine` port（独立文件 `tt-ports/src/skill_script.rs`）：

```rust
pub struct SkillScriptRequest {
    pub script_path: PathBuf,        // 入口脚本物理路径；其 parent() 即当前 skill 的 scripts/ 目录（模块白名单）
    pub args: serde_json::Value,
    pub work_dir: PathBuf,           // 当前 run 的 workdir（动态）
    pub visible_roots: Vec<String>,  // 相对 workdir
    pub writable_roots: Vec<String>, // 相对 workdir
    pub world_info_entries: Vec<ActivatedWorldInfoEntry>, // 预取快照
}

pub struct SkillScriptResult {
    pub value: serde_json::Value,    // 脚本返回值
}

#[async_trait]
pub trait SkillScriptEngine: Send + Sync {
    async fn execute(&self, request: SkillScriptRequest) -> Result<SkillScriptResult, DomainError>;
}
```

- port 位于 `tt-ports/src/skill_script.rs`（顶层模块，与 `mcp.rs`、`external_import.rs` 等既有 outbound port 布局一致）；
- `ActivatedWorldInfoEntry` 从 quickjs crate 移至 `tt-domain`（模型层），quickjs 与 tt-application 均从 domain 引用，避免反向依赖。

## 6. 沙箱规则（tt-adapter-quickjs 重写）

- **入口**：仅执行 `skill_file_path` 解析出的 `scripts/{script}.js`。
- **模块加载**：自定义 module loader；相对导入（`./helper.js`、`../scripts/x.js`）规范化后必须仍位于当前 skill 的 `scripts/` 目录内；裸模块名解析到 `{data_root}/skill-libs/`（即公共 libs 目录，由 composition 注入 engine）；其余一律拒绝。
- **$fs**：路径规范化（`path_clean`）后相对 `work_dir` 解析；读限于 visible_roots、写限于 writable_roots；拒绝逃逸。
- **资源限制**：内存 32MB、栈 256KB（沿用）；执行超时 30s（`Runtime::set_interrupt_handler` + wall-clock）；返回值序列化大小上限（建议 256KB，超出报错）。
- **隔离与并发**：每次执行新建 Runtime + Context，在 `spawn_blocking` 中运行，不阻塞 tokio worker；引擎实例本身无共享可变状态。
- **API 注入**：`$fs`（readText/writeText/listFiles/exists）、`$worldInfo`（readActivated/readEntries，预取快照）、`$log`（info/warn/error/debug → tracing）。不提供网络、定时器、进程等能力。
- **错误细节**：JS 异常消息与栈完整传播，不再被吞。

删除：现有 `tool/descriptor.rs`、`tool/executor.rs`（`ScriptToolDescriptor`/`ScriptToolExecutor`）、`eprintln!` 调试输出。

## 7. 组合层装配（host）

- composition 创建 `QuickJsScriptEngine`，注入公共 libs 目录（`{data_root}/skill-libs/`）与超时配置；
- `AgentToolDispatcher::new` 增加 `SkillScriptEngine` 参数；
- `BuiltinAgentToolRegistry` 增加 `skill_script_descriptor()`；
- `tauritavern` host crate 的 Cargo.toml 新增 `tt-adapter-quickjs` 依赖。

## 8. 错误处理（fail-fast，is_error=true + error_code）

| 场景 | error_code |
|---|---|
| script 标识符非法 | `skill.script_invalid_name` |
| skill 不可见或不存在 | `skill.script_skill_not_visible` |
| 脚本文件不存在 | `skill.script_not_found` |
| JS 执行异常 | `skill.script_execution_failed`（含异常消息与栈） |
| 超时 | `skill.script_timeout` |
| 返回值超限 | `skill.script_result_too_large` |

## 9. 测试策略

- **tt-adapter-quickjs**：沙箱单测（`../` 穿越、白名单外模块、$fs 越权读写）；执行测试（default/main 导出调用、异常传播、超时中断、返回值 JSON 转换、结果大小上限）。
- **tt-application**：`skill/script.rs` 处理器测试（fake engine：不可见 skill 拒绝、非法 script 名、脚本不存在、成功路径的 AgentToolResult 组装）。
- **既有测试更新**：`registry_declares_canonical_builtin_descriptors` 的 `len() == 19` → `20`。
- **门槛**：`cargo test`（受影响 crate）+ `scripts/check-rust-crate-boundaries.mjs` + `pnpm run check`。

## 10. 第一期明确排除

- 前端 UI / DTO / 事件（零改动）；
- 脚本枚举与元数据机制（清单、JSDoc 解析等）；
- 公共 libs 目录的安装/管理界面（只定目录约定）；
- skill 安装时对脚本内容的额外校验；
- 脚本并发调用配额（依赖现有 per-tool max_calls 机制即可）。

## 11. 涉及文件（预估）

| 层 | 文件 | 动作 |
|----|------|------|
| tt-ports | `repositories/skill_repository.rs` | 新增 `skill_file_path` |
| tt-ports | `skill_script.rs`（新） | port trait + request/result |
| tt-adapter-storage-userdata | `file_skill_repository/` | 实现 `skill_file_path` |
| tt-adapter-quickjs | `engine.rs` / `sandbox.rs` / `api/*` | 重写：port 实现、动态绑定、超时、隔离 |
| tt-adapter-quickjs | `tool/` | 删除 |
| tt-application | `agent_tools/skill/script.rs`（新）+ `descriptors.rs` + `mod.rs` + `registry.rs` + `dispatcher.rs` | 注册与分发 |
| tauritavern host | composition + Cargo.toml | 装配引擎 |
