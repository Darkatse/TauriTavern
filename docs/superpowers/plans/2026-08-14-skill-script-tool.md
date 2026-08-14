# skill.script 内置工具实现计划

> **面向 AI 代理的工作者：** 必需子技能：使用 superpowers:subagent-driven-development（推荐）或 superpowers:executing-plans 逐任务实现此计划。步骤使用复选框（`- [ ]`）语法来跟踪进度。

**目标：** 新增第 20 个 builtin Agent 工具 `skill.script`，在 QuickJS 沙箱内执行可见 skill 包 `scripts/` 目录下的 JS 脚本，读写当前 run 的动态 workdir。

**架构：** 方案 A —— `SkillRepository` 新增 `skill_file_path` 解析物理脚本路径；新增 `SkillScriptEngine` outbound port（`tt-ports`）由重写后的 `tt-adapter-quickjs` 实现；`tt-application` 的 `skill/script.rs` 处理器完成校验（script 标识符、skill 可见性）、workdir/roots 解析、世界书预取与错误映射；host composition 装配引擎并注入公共 libs 目录。

**技术栈：** Rust（workspace crates：tt-domain / tt-ports / tt-adapter-storage-userdata / tt-adapter-quickjs / tt-application / tauritavern）、rquickjs 0.9、tokio spawn_blocking、path-clean。

**设计规格：** `docs/superpowers/specs/2026-08-14-skill-script-design.md`（已批准）。分支：`feature/quickjs-script-engine`。

---

## 文件结构

| 层 | 文件 | 动作 | 职责 |
|----|------|------|------|
| tt-domain | `src/errors.rs` | 修改 | 新增 `SkillScriptExecutionFailed`、`SkillScriptResultTooLarge` 变体 |
| tt-domain | `src/models/skill_script.rs` | 创建 | `ActivatedWorldInfoEntry`（世界书快照模型 + 解析） |
| tt-domain | `src/models.rs`（或 `models/mod.rs`） | 修改 | 注册 `skill_script` 模块 |
| tt-ports | `src/repositories/skill_repository.rs` | 修改 | trait 新增 `skill_file_path` |
| tt-ports | `src/repositories/workspace_repository.rs` | 修改 | trait 新增 `run_workspace_root` |
| tt-ports | `src/skill_script.rs` | 创建 | `SkillScriptEngine` port + request/result |
| tt-ports | `src/lib.rs` | 修改 | 注册 `skill_script` 模块 |
| tt-adapter-storage-userdata | `src/repositories/file_skill_repository/mod.rs` | 修改 | 实现 `skill_file_path` |
| tt-adapter-storage-userdata | `src/repositories/file_skill_repository/tests.rs` | 修改 | `skill_file_path` 测试 |
| tt-adapter-storage-userdata | `src/repositories/file_agent_repository/workspace_store.rs` | 修改 | 实现 `run_workspace_root` |
| tt-adapter-storage-userdata | `src/repositories/file_agent_repository/tests.rs` | 修改 | `run_workspace_root` 测试 |
| tt-adapter-quickjs | `src/sandbox.rs` | 重写 | `SandboxIoPolicy`（$fs 门控 + 模块白名单） |
| tt-adapter-quickjs | `src/convert.rs` | 创建 | JSON ↔ JS 值转换 |
| tt-adapter-quickjs | `src/engine.rs` | 重写 | `QuickJsScriptEngine` 实现 port（隔离执行、超时、上限） |
| tt-adapter-quickjs | `src/api/fs.rs` | 重写 | 同步 `$fs`（std::fs + 门控） |
| tt-adapter-quickjs | `src/api/world_info.rs` | 重写 | `$worldInfo`（改用 domain 模型） |
| tt-adapter-quickjs | `src/api/log.rs` | 重写 | 同步 `$log` |
| tt-adapter-quickjs | `src/api/mod.rs`、`src/lib.rs` | 修改 | 模块声明与导出更新 |
| tt-adapter-quickjs | `src/tool/`（目录） | 删除 | 旧 descriptor/executor 废弃 |
| tt-adapter-quickjs | `Cargo.toml` | 修改 | 移除 anyhow/thiserror，保留其余 |
| tt-application | `src/errors.rs` | 修改 | `From<DomainError>` 新增两个映射臂 |
| tt-application | `src/services/skill_service.rs` | 修改 | 暴露 `skill_file_path`；测试 fake 补新方法 |
| tt-application | `src/services/agent_tools/skill/descriptors.rs` | 修改 | `skill_script_descriptor()` |
| tt-application | `src/services/agent_tools/skill/script.rs` | 创建 | 处理器 + 测试 |
| tt-application | `src/services/agent_tools/skill/mod.rs` | 修改 | `SKILL_SCRIPT` 常量与导出 |
| tt-application | `src/services/agent_tools/registry.rs` | 修改 | 注册第 20 个工具；测试 19→20 |
| tt-application | `src/services/agent_tools/dispatcher.rs` | 修改 | 引擎字段、构造参数、分发分支 |
| tt-application | `src/services/agent_runtime_service.rs` | 修改 | 构造参数透传 |
| tt-application | `src/services/agent_tools/workspace/tests.rs` | 修改 | `TestWorkspaceRepository` 补新方法 |
| tauritavern | `Cargo.toml` | 修改 | 新增 `tt-adapter-quickjs` 依赖 |
| tauritavern | `src/app/composition/services/mod.rs` | 修改 | 构造引擎（libs 目录注入） |
| tauritavern | `src/app/composition/services/agent.rs` | 修改 | 透传引擎给 `AgentRuntimeService::new` |

任务间依赖：1 → 2 → 3 → 4 → 5 → 6（严格顺序；每步结束时 workspace 必须 `cargo check` 通过）。

**实现者注意（rquickjs API）：** 本计划中的 rquickjs 0.9 调用形态（`Module::declare`、`set_module_loader` 闭包签名、`Exception::throw_message` 等）以编译器报错为准微调调用签名，但函数职责、参数语义与调用顺序不得改变。

---

### 任务 1：tt-domain 错误变体 + 世界书脚本模型 + From 映射

**文件：**
- 修改：`src-tauri/crates/tt-domain/src/errors.rs`
- 创建：`src-tauri/crates/tt-domain/src/models/skill_script.rs`
- 修改：`src-tauri/crates/tt-domain/src/models.rs`（若是 `models/mod.rs` 目录则改该文件）
- 修改：`src-tauri/crates/tt-application/src/errors.rs`

- [ ] **步骤 1.1：编写失败的测试（错误变体）**

在 `tt-domain/src/errors.rs` 的 `mod tests` 中追加：

```rust
    #[test]
    fn skill_script_execution_failed_constructor_keeps_message() {
        let error = DomainError::SkillScriptExecutionFailed {
            message: "boom at line 3".to_string(),
        };

        assert!(matches!(
            &error,
            DomainError::SkillScriptExecutionFailed { message } if message == "boom at line 3"
        ));
        assert_eq!(error.to_string(), "Skill script execution failed: boom at line 3");
    }

    #[test]
    fn skill_script_result_too_large_constructor_keeps_sizes() {
        let error = DomainError::SkillScriptResultTooLarge {
            actual_bytes: 300_000,
            limit_bytes: 262_144,
        };

        assert!(matches!(
            &error,
            DomainError::SkillScriptResultTooLarge { actual_bytes: 300_000, limit_bytes: 262_144 }
        ));
    }
```

- [ ] **步骤 1.2：运行测试验证失败**

运行：`cargo test -p tt-domain --lib errors`
预期：FAIL，报 `no variant named SkillScriptExecutionFailed`

- [ ] **步骤 1.3：实现错误变体**

在 `tt-domain/src/errors.rs` 的 `DomainError` 枚举中、`WorkspaceWriteConflict` 变体之后追加（并在 `impl DomainError` 中补构造器）：

```rust
    #[error("Skill script execution failed: {message}")]
    SkillScriptExecutionFailed { message: String },

    #[error("Skill script result is {actual_bytes} bytes, exceeding the {limit_bytes}-byte limit")]
    SkillScriptResultTooLarge {
        actual_bytes: usize,
        limit_bytes: usize,
    },
```

```rust
    pub fn skill_script_execution_failed(message: impl Into<String>) -> Self {
        Self::SkillScriptExecutionFailed {
            message: message.into(),
        }
    }
```

- [ ] **步骤 1.4：运行测试验证通过**

运行：`cargo test -p tt-domain --lib errors`
预期：PASS

- [ ] **步骤 1.5：编写失败的测试（ActivatedWorldInfoEntry）**

创建 `tt-domain/src/models/skill_script.rs`，先只写测试模块（文件顶部暂留空实现占位会导致编译失败，直接写测试 + 空结构体定义）：

```rust
//! 模型：skill 脚本执行所需的运行时快照数据。

use serde_json::Value;

/// 当前 agent run 激活的世界书条目（预取自 run prompt snapshot），
/// 经 `$worldInfo` API 提供给 skill 脚本。
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

#[cfg(test)]
mod tests {
    use super::ActivatedWorldInfoEntry;
    use serde_json::json;

    #[test]
    fn parses_entry_and_builds_ref_key() {
        let entry = ActivatedWorldInfoEntry::from_value(
            0,
            &json!({
                "world": "lore",
                "uid": "12",
                "displayName": "Capital",
                "constant": true,
                "position": "before",
                "content": "The capital is Alden."
            }),
        )
        .expect("valid entry");

        assert_eq!(entry.ref_key, "worldinfo:lore#12");
        assert_eq!(entry.content, "The capital is Alden.");
        assert!(entry.constant);
        assert_eq!(entry.position.as_deref(), Some("before"));
    }

    #[test]
    fn numeric_uid_is_stringified() {
        let entry = ActivatedWorldInfoEntry::from_value(
            0,
            &json!({ "world": "lore", "uid": 7, "content": "text" }),
        )
        .expect("valid entry");

        assert_eq!(entry.ref_key, "worldinfo:lore#7");
    }

    #[test]
    fn missing_content_or_object_returns_none() {
        assert!(ActivatedWorldInfoEntry::from_value(0, &json!("plain")).is_none());
        assert!(ActivatedWorldInfoEntry::from_value(0, &json!({ "world": "lore" })).is_none());
    }

    #[test]
    fn missing_world_falls_back_to_indexed_ref() {
        let entry = ActivatedWorldInfoEntry::from_value(3, &json!({ "content": "x" }))
            .expect("valid entry");

        assert_eq!(entry.ref_key, "worldinfo:activated#3");
    }
}
```

在 `tt-domain/src/models.rs`（或 `models/mod.rs`）中按字母序追加一行模块声明：

```rust
pub mod skill_script;
```

- [ ] **步骤 1.6：运行测试验证失败**

运行：`cargo test -p tt-domain --lib models::skill_script`
预期：FAIL，报 `from_value` 未定义

- [ ] **步骤 1.7：实现 from_value**

在 `skill_script.rs` 的 `impl ActivatedWorldInfoEntry` 中实现（对齐 `agent_tools/world_info/read_activated.rs` 的 normalize 语义）：

```rust
impl ActivatedWorldInfoEntry {
    /// 从 `worldInfoActivation.entries[i]` JSON 项解析；
    /// 非对象或缺 string content 时返回 `None`（调用方跳过）。
    pub fn from_value(index: usize, value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        let world = object
            .get("world")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let uid = match object.get("uid") {
            Some(Value::String(value)) => value.clone(),
            Some(Value::Number(value)) => value.to_string(),
            _ => String::new(),
        };
        let ref_key = if world.is_empty() || uid.is_empty() {
            format!("worldinfo:activated#{index}")
        } else {
            format!("worldinfo:{world}#{uid}")
        };
        let content = object.get("content")?.as_str()?.to_string();
        Some(Self {
            display_name: object
                .get("displayName")
                .and_then(Value::as_str)
                .map(String::from),
            constant: object
                .get("constant")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            position: object
                .get("position")
                .and_then(Value::as_str)
                .map(String::from),
            world,
            uid,
            content,
            ref_key,
        })
    }
}
```

- [ ] **步骤 1.8：运行测试验证通过**

运行：`cargo test -p tt-domain`
预期：PASS（全部）

- [ ] **步骤 1.9：修复 tt-application 的穷尽匹配并补测试**

`cargo check -p tt-application` 会因 `From<DomainError>` 非穷尽而失败。在 `tt-application/src/errors.rs` 的 `impl From<DomainError> for ApplicationError` 中追加两个映射臂：

```rust
            DomainError::SkillScriptExecutionFailed { message } => {
                ApplicationError::InternalError(message)
            }
            DomainError::SkillScriptResultTooLarge {
                actual_bytes,
                limit_bytes,
            } => ApplicationError::ValidationError(format!(
                "Skill script result is {actual_bytes} bytes, exceeding the {limit_bytes}-byte limit"
            )),
```

在其 `mod tests` 中追加：

```rust
    #[test]
    fn skill_script_errors_map_to_application_errors() {
        let execution = ApplicationError::from(DomainError::SkillScriptExecutionFailed {
            message: "boom".to_string(),
        });
        assert!(matches!(execution, ApplicationError::InternalError(message) if message == "boom"));

        let too_large = ApplicationError::from(DomainError::SkillScriptResultTooLarge {
            actual_bytes: 300_000,
            limit_bytes: 262_144,
        });
        assert!(matches!(
            too_large,
            ApplicationError::ValidationError(message) if message.contains("300000 bytes")
        ));
    }
```

运行：`cargo test -p tt-application --lib errors`
预期：PASS

- [ ] **步骤 1.10：边界检查 + Commit**

运行：`node scripts/check-rust-crate-boundaries.mjs`
预期：通过（domain 未引入违禁依赖；serde_json 为 domain 既有依赖）

```bash
git add src-tauri/crates/tt-domain/src/errors.rs src-tauri/crates/tt-domain/src/models/skill_script.rs src-tauri/crates/tt-domain/src/models.rs src-tauri/crates/tt-application/src/errors.rs
git commit -m "feat(skill-script): add domain error variants and world info script model"
```

（若 models 为目录模块，提交路径相应调整为 `models/mod.rs`。）

---

### 任务 2：仓储层 — skill_file_path 与 run_workspace_root

**文件：**
- 修改：`src-tauri/crates/tt-ports/src/repositories/skill_repository.rs`
- 修改：`src-tauri/crates/tt-ports/src/repositories/workspace_repository.rs`
- 修改：`src-tauri/crates/tt-adapter-storage-userdata/src/repositories/file_skill_repository/mod.rs`
- 修改：`src-tauri/crates/tt-adapter-storage-userdata/src/repositories/file_skill_repository/tests.rs`
- 修改：`src-tauri/crates/tt-adapter-storage-userdata/src/repositories/file_agent_repository/workspace_store.rs`
- 修改：`src-tauri/crates/tt-adapter-storage-userdata/src/repositories/file_agent_repository/tests.rs`
- 修改：`src-tauri/crates/tt-application/src/services/skill_service.rs`（fake 补方法）
- 修改：`src-tauri/crates/tt-application/src/services/agent_tools/workspace/tests.rs`（fake 补方法）

- [ ] **步骤 2.1：编写失败的测试（skill_file_path）**

在 `file_skill_repository/tests.rs` 末尾追加（复用现有 `temp_root` / `global_scope` / `inline_skill` / `SkillInstallRequest` 基建，参照 `installs_inline_skill_and_reads_file` 测试）：

```rust
async fn install_skill_with_scripts(repository: &FileSkillRepository) {
    use tt_domain::models::skill::SkillInstallAction;

    let result = repository
        .install_import(SkillInstallRequest {
            target_scope: global_scope(),
            input: inline_skill(
                "scripted-skill",
                vec![("scripts/helper.js", "export const answer = 42;")],
            ),
            conflict_strategy: None,
        })
        .await
        .expect("install skill");
    assert_eq!(result.action, SkillInstallAction::Installed);
}

#[tokio::test]
async fn skill_file_path_resolves_canonical_script_file() {
    let root = temp_root("skill-file-path");
    let repository = FileSkillRepository::new(root.clone());
    install_skill_with_scripts(&repository).await;

    let path = repository
        .skill_file_path(global_scope(), "scripted-skill", "scripts/helper.js")
        .await
        .expect("resolve script path");

    assert!(path.is_absolute());
    assert!(path.ends_with("scripted-skill/scripts/helper.js"));
    assert!(tokio_fs::try_exists(&path).await.expect("stat script"));

    tokio_fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn skill_file_path_reports_missing_script() {
    let root = temp_root("skill-file-path-missing");
    let repository = FileSkillRepository::new(root.clone());
    install_skill_with_scripts(&repository).await;

    let error = repository
        .skill_file_path(global_scope(), "scripted-skill", "scripts/nope.js")
        .await
        .expect_err("missing script");

    assert!(matches!(error, DomainError::NotFound(message) if message.contains("scripts/nope.js")));

    tokio_fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn skill_file_path_rejects_paths_outside_scripts_dir() {
    let root = temp_root("skill-file-path-escape");
    let repository = FileSkillRepository::new(root.clone());
    install_skill_with_scripts(&repository).await;

    for bad_path in ["SKILL.md", "../outside.js", "scripts/../../escape.js"] {
        let error = repository
            .skill_file_path(global_scope(), "scripted-skill", bad_path)
            .await
            .expect_err("path outside scripts/ must be rejected");
        assert!(
            matches!(error, DomainError::InvalidData(_) | DomainError::NotFound(_)),
            "unexpected error for {bad_path}: {error:?}"
        );
    }

    tokio_fs::remove_dir_all(root).await.expect("cleanup");
}
```

注意：`skill_file_path` 走 trait 调用时需要 `use tt_ports::repositories::skill_repository::SkillRepository;`（tests.rs 顶部已有）。

- [ ] **步骤 2.2：运行测试验证失败**

运行：`cargo test -p tt-adapter-storage-userdata skill_file_path`
预期：FAIL，报 trait 无 `skill_file_path` 方法

- [ ] **步骤 2.3：port trait + 实现 + fake 修复**

在 `tt-ports/src/repositories/skill_repository.rs` 的 `SkillRepository` trait 中、`read_skill_file` 之前追加：

```rust
    /// 解析已安装 skill 包内文件的**规范化绝对物理路径**。
    /// 实现必须校验：skill 已安装、相对路径规范化后未逃逸 skill 目录、
    /// 目标存在且为普通文件（非符号链接）。
    async fn skill_file_path(
        &self,
        scope: SkillScope,
        name: &str,
        relative_path: &str,
    ) -> Result<std::path::PathBuf, DomainError>;
```

在 `file_skill_repository/mod.rs` 的 `impl SkillRepository for FileSkillRepository` 中实现（复用 `read_text_file_at` 的防逃逸模式；`read.rs` 中 `normalize_skill_path` 已拒绝 `..` 逃逸，此处再强制 `scripts/` 前缀，见步骤 1.10 提交前先阅读 `paths.rs::normalize_skill_path` 确认其语义）：

```rust
    async fn skill_file_path(
        &self,
        scope: SkillScope,
        name: &str,
        relative_path: &str,
    ) -> Result<PathBuf, DomainError> {
        let name = paths::validate_skill_name(name)?;
        let path = paths::normalize_skill_path(relative_path)?;
        if !path.starts_with("scripts/") {
            return Err(DomainError::InvalidData(format!(
                "Skill script path must stay under scripts/: skills/{name}/{path}"
            )));
        }
        let skill_root = self.installed_skill_root(&scope, &name).await?;
        let full_path = skill_root.join(&path);
        let metadata = fs::symlink_metadata(&full_path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                DomainError::NotFound(format!("Skill file not found: skills/{name}/{path}"))
            } else {
                DomainError::InternalError(format!(
                    "Failed to read Skill file metadata '{}': {}",
                    full_path.display(),
                    error
                ))
            }
        })?;
        if metadata.file_type().is_symlink() {
            return Err(DomainError::InvalidData(format!(
                "Skill file cannot be a symlink: skills/{name}/{path}"
            )));
        }
        if !metadata.is_file() {
            return Err(DomainError::InvalidData(format!(
                "Skill path is not a file: skills/{name}/{path}"
            )));
        }
        let canonical_root = fs::canonicalize(&skill_root).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to resolve Skill directory '{}': {}",
                skill_root.display(),
                error
            ))
        })?;
        let canonical_file = fs::canonicalize(&full_path).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to resolve Skill file '{}': {}",
                full_path.display(),
                error
            ))
        })?;
        if !canonical_file.starts_with(&canonical_root) {
            return Err(DomainError::InvalidData(format!(
                "Skill file escapes installed directory: skills/{name}/{path}"
            )));
        }
        Ok(canonical_file)
    }
```

（`PathBuf` 已在 mod.rs 顶部 `use std::path::PathBuf;` 引入。）

在 `tt-application/src/services/skill_service.rs` 测试模块的 `FakeSkillRepository` 中补：

```rust
        async fn skill_file_path(
            &self,
            _scope: SkillScope,
            _name: &str,
            _relative_path: &str,
        ) -> Result<std::path::PathBuf, DomainError> {
            unreachable!("not needed for resolver tests")
        }
```

- [ ] **步骤 2.4：运行测试验证通过**

运行：`cargo test -p tt-adapter-storage-userdata skill_file_path`
预期：PASS（3 个测试）

- [ ] **步骤 2.5：编写失败的测试（run_workspace_root）**

在 `file_agent_repository/tests.rs` 中追加（`initialize_run` 的 profile 参数复用本文件 round-trip 测试 `repository_round_trips_run_workspace_event_and_checkpoint` 所用的 profile helper；若 helper 名不同以现有代码为准）：

```rust
#[tokio::test]
async fn run_workspace_root_returns_existing_canonical_run_dir() {
    let root = temp_root();
    let repository = FileAgentRepository::new(root.clone());
    let run = sample_run();
    let manifest = sample_manifest(&run);
    repository
        .initialize_run(&run, &manifest, &serde_json::json!({}), &sample_resolved_profile())
        .await
        .expect("initialize run");

    let workspace_root = WorkspaceRepository::run_workspace_root(&repository, &run.id)
        .await
        .expect("resolve workspace root");

    assert!(workspace_root.is_absolute());
    assert!(workspace_root.starts_with(&root));
    assert!(workspace_root.ends_with("runs/run_test"));
    assert!(fs::try_exists(&workspace_root).await.expect("stat run dir"));

    let error = WorkspaceRepository::run_workspace_root(&repository, "run_missing")
        .await
        .expect_err("missing run");
    assert!(matches!(error, DomainError::NotFound(_)));

    let _ = tokio::fs::remove_dir_all(root).await;
}
```

（若 `sample_resolved_profile` 不存在，使用 round-trip 测试中构造 `ResolvedAgentProfile` 的现有 helper/内联字面量。）

- [ ] **步骤 2.6：运行测试验证失败**

运行：`cargo test -p tt-adapter-storage-userdata run_workspace_root`
预期：FAIL，报 trait 无 `run_workspace_root` 方法

- [ ] **步骤 2.7：port trait + 实现 + fake 修复**

在 `tt-ports/src/repositories/workspace_repository.rs` 的 `WorkspaceRepository` trait 中追加：

```rust
    /// 解析当前 run 工作区根目录的**规范化绝对物理路径**（供沙箱脚本引擎做
    /// 路径前缀门控；run 不存在时返回 `NotFound`）。
    async fn run_workspace_root(&self, run_id: &str)
        -> Result<std::path::PathBuf, DomainError>;
```

在 `file_agent_repository/workspace_store.rs` 的 `impl WorkspaceRepository for FileAgentRepository` 中实现：

```rust
    async fn run_workspace_root(&self, run_id: &str) -> Result<PathBuf, DomainError> {
        let run_dir = self.load_run_dir(run_id).await?;
        fs::canonicalize(&run_dir).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to resolve agent workspace root {}: {}",
                run_dir.display(),
                error
            ))
        })
    }
```

（`PathBuf` 若未导入则在文件头补 `use std::path::PathBuf;`。）

同步修复两处 fake：
- `tt-application/src/services/agent_tools/workspace/tests.rs` 的 `TestWorkspaceRepository` 追加：

```rust
    async fn run_workspace_root(&self, _run_id: &str) -> Result<PathBuf, DomainError> {
        unreachable!("not needed for workspace tool tests")
    }
```

（并补 `use std::path::PathBuf;` 若缺失。）

- 若 `agent_runtime_service/delegation/workspace_policy.rs` 的 `InvocationWorkspaceRepository`（第二处实现）因此编译失败，为其补一个基于其内部存储的等价实现或 `unreachable!`（阅读该文件后选择：该实现包装 model workspace repository，可直接委托 `self.inner.run_workspace_root(run_id)`，以其真实结构为准）。

- [ ] **步骤 2.8：运行测试验证通过**

```bash
cargo test -p tt-adapter-storage-userdata run_workspace_root
cargo test -p tt-application --lib services::agent_tools::workspace
```
预期：全部 PASS

- [ ] **步骤 2.9：SkillService 暴露 skill_file_path**

在 `tt-application/src/services/skill_service.rs` 中（紧邻 `read_skill_file` 方法）追加：

```rust
    /// 解析已安装 skill 包内文件的规范化绝对物理路径（供 skill.script 引擎定位入口脚本）。
    pub async fn skill_file_path(
        &self,
        scope: SkillScope,
        name: &str,
        relative_path: &str,
    ) -> Result<PathBuf, ApplicationError> {
        self.repository
            .skill_file_path(scope, name, relative_path)
            .await
            .map_err(ApplicationError::from)
    }
```

（文件顶部已有 `SkillScope` 导入则复用；`PathBuf` 若缺失补 `use std::path::PathBuf;`。）

运行：`cargo check -p tt-application`
预期：通过

- [ ] **步骤 2.10：Commit**

```bash
git add src-tauri/crates/tt-ports/src/repositories/skill_repository.rs src-tauri/crates/tt-ports/src/repositories/workspace_repository.rs src-tauri/crates/tt-adapter-storage-userdata/src/repositories/file_skill_repository/ src-tauri/crates/tt-adapter-storage-userdata/src/repositories/file_agent_repository/ src-tauri/crates/tt-application/src/services/skill_service.rs src-tauri/crates/tt-application/src/services/agent_tools/workspace/tests.rs src-tauri/crates/tt-application/src/services/agent_runtime_service/delegation/workspace_policy.rs
git commit -m "feat(skill-script): expose skill file and run workspace physical paths via repositories"
```

---

### 任务 3：SkillScriptEngine port + SandboxIoPolicy（安全策略核心）

**文件：**
- 创建：`src-tauri/crates/tt-ports/src/skill_script.rs`
- 修改：`src-tauri/crates/tt-ports/src/lib.rs`
- 重写：`src-tauri/crates/tt-adapter-quickjs/src/sandbox.rs`

- [ ] **步骤 3.1：定义 port（纯契约，编译即验证）**

创建 `tt-ports/src/skill_script.rs`：

```rust
//! Outbound port：skill 脚本沙箱执行引擎。
//!
//! 应用层经此 port 请求在隔离的 QuickJS 运行时中执行某个 skill 包
//! `scripts/` 目录下的入口脚本；具体沙箱规则由 adapter 实现。

use std::path::PathBuf;

use async_trait::async_trait;
use tt_domain::errors::DomainError;
use tt_domain::models::skill_script::ActivatedWorldInfoEntry;

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
}

/// 脚本返回值（JSON）。
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
```

在 `tt-ports/src/lib.rs` 按字母序追加：

```rust
pub mod skill_script;
```

检查 `tt-ports/Cargo.toml`：若尚无 `serde_json` 依赖则添加（与 `mcp.rs` 等 port 一致）：`serde_json = { workspace = true }`。

运行：`cargo check -p tt-ports`
预期：通过

- [ ] **步骤 3.2：编写失败的测试（SandboxIoPolicy）**

重写 `tt-adapter-quickjs/src/sandbox.rs`，先写测试与结构骨架（实现体先不写）：

```rust
//! 单次脚本执行的沙箱 IO 策略：
//! - `$fs` 路径解析相对 `work_dir`，读/写分别受 visible/writable roots 门控，拒绝逃逸；
//! - 模块加载白名单 = 入口脚本所在 skill 的 `scripts/` 目录 + 公共 libs 目录。

use std::path::{Path, PathBuf};

/// 一次执行的不可变 IO 策略。
#[derive(Debug, Clone)]
pub struct SandboxIoPolicy {
    pub work_dir: PathBuf,
    pub visible_roots: Vec<String>,
    pub writable_roots: Vec<String>,
    pub scripts_dir: PathBuf,
    pub libs_dir: PathBuf,
}

impl SandboxIoPolicy {
    pub fn new(
        work_dir: PathBuf,
        visible_roots: Vec<String>,
        writable_roots: Vec<String>,
        scripts_dir: PathBuf,
        libs_dir: PathBuf,
    ) -> Self {
        Self {
            work_dir,
            visible_roots,
            writable_roots,
            scripts_dir,
            libs_dir,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SandboxIoPolicy;
    use std::path::PathBuf;

    fn policy_with_dirs(scripts_dir: PathBuf, libs_dir: PathBuf) -> SandboxIoPolicy {
        SandboxIoPolicy::new(
            PathBuf::from("/tmp/work"),
            vec!["output".to_string()],
            vec!["output".to_string()],
            scripts_dir,
            libs_dir,
        )
    }

    #[test]
    fn read_is_gated_by_visible_roots() {
        let policy = policy_with_dirs(PathBuf::from("/tmp/scripts"), PathBuf::from("/tmp/libs"));

        assert!(policy.check_read("output/a.md").is_ok());
        assert!(policy.check_read("input/secret.json").is_err());
    }

    #[test]
    fn write_is_gated_by_writable_roots() {
        let policy = SandboxIoPolicy::new(
            PathBuf::from("/tmp/work"),
            vec!["output".to_string()],
            vec![], // nothing writable
            PathBuf::from("/tmp/scripts"),
            PathBuf::from("/tmp/libs"),
        );

        assert!(policy.check_write("output/a.md").is_err());
        assert!(policy.check_read("output/a.md").is_ok());
    }

    #[test]
    fn escapes_and_absolute_paths_are_rejected() {
        let policy = policy_with_dirs(PathBuf::from("/tmp/scripts"), PathBuf::from("/tmp/libs"));

        assert!(policy.check_read("../outside.md").is_err());
        assert!(policy.check_read("output/../../outside.md").is_err());
        assert!(policy.check_read("/etc/passwd").is_err());
    }

    #[test]
    fn relative_modules_must_stay_in_scripts_dir() {
        let scripts_dir = PathBuf::from("/tmp/skills/demo/scripts");
        let policy = policy_with_dirs(scripts_dir.clone(), PathBuf::from("/tmp/libs"));

        let base = scripts_dir.join("main.js").to_string_lossy().to_string();
        assert!(policy.resolve_module(&base, "./helper.js").is_ok());
        // ../lib.js 清洗后落在 scripts 目录外 -> 拒绝（即使文件不存在）
        assert!(policy.resolve_module(&base, "../lib.js").is_err());
        assert!(policy.resolve_module(&base, "/abs/path.js").is_err());
    }

    #[test]
    fn bare_modules_resolve_to_public_libs() {
        let temp = tempfile::tempdir().expect("tempdir");
        let libs_dir = temp.path().join("skill-libs");
        std::fs::create_dir_all(libs_dir.join("markdown")).expect("mkdir");
        std::fs::write(libs_dir.join("markdown").join("index.js"), "export const x = 1;")
            .expect("write index.js");
        std::fs::write(libs_dir.join("utils.js"), "export const y = 2;").expect("write utils.js");
        let policy = policy_with_dirs(PathBuf::from("/tmp/scripts"), libs_dir.clone());

        let base = PathBuf::from("/tmp/scripts/main.js").to_string_lossy().to_string();
        let direct = policy.resolve_module(&base, "utils").expect("utils resolves");
        assert!(direct.ends_with("utils.js"));
        let index = policy
            .resolve_module(&base, "markdown")
            .expect("markdown resolves");
        assert!(index.ends_with("markdown").and_then(|_| Some(true)).unwrap_or(false)
            || index.to_string_lossy().contains("markdown"));
        assert!(policy.resolve_module(&base, "missing-lib").is_err());
    }
}
```

- [ ] **步骤 3.3：运行测试验证失败**

运行：`cargo test -p tt-adapter-quickjs sandbox`
预期：FAIL，报 `check_read`/`resolve_module` 等方法未定义

- [ ] **步骤 3.4：实现 SandboxIoPolicy**

在 `sandbox.rs` 的 `impl SandboxIoPolicy` 中追加：

```rust
    /// 将脚本提供的相对路径解析到 work_dir 内；拒绝绝对路径与 `..` 逃逸。
    pub fn resolve_work_path(&self, raw: &str) -> Result<PathBuf, String> {
        if raw.contains('\0') {
            return Err(format!("path must not contain NUL: {raw:?}"));
        }
        let relative = Path::new(raw);
        if relative.is_absolute() {
            return Err(format!("absolute paths are not allowed: {raw}"));
        }
        let cleaned = path_clean::clean(relative);
        if cleaned.to_string_lossy().starts_with("..") {
            return Err(format!("path escapes the workspace: {raw}"));
        }
        Ok(self.work_dir.join(cleaned))
    }

    fn under_roots(cleaned: &Path, roots: &[String]) -> bool {
        roots.iter().any(|root| {
            let root = root.trim();
            !root.is_empty() && cleaned.starts_with(Path::new(root))
        })
    }

    /// `$fs` 读门控：路径（清洗后）必须落在某个 visible root 内。
    pub fn check_read(&self, raw: &str) -> Result<PathBuf, String> {
        let cleaned = path_clean::clean(Path::new(raw));
        if !Self::under_roots(&cleaned, &self.visible_roots) {
            return Err(format!("path is outside the visible workspace roots: {raw}"));
        }
        self.resolve_work_path(raw)
    }

    /// `$fs` 写门控：路径（清洗后）必须落在某个 writable root 内。
    pub fn check_write(&self, raw: &str) -> Result<PathBuf, String> {
        let cleaned = path_clean::clean(Path::new(raw));
        if !Self::under_roots(&cleaned, &self.writable_roots) {
            return Err(format!("path is outside the writable workspace roots: {raw}"));
        }
        self.resolve_work_path(raw)
    }

    /// 模块解析门控（D2）：
    /// - `./`、`../` 相对导入：清洗后必须仍位于当前 skill 的 `scripts/` 目录内；
    /// - 裸模块名：解析到公共 libs 目录（`{name}.js` 或 `{name}/index.js`）；
    /// - 绝对路径与其他形式一律拒绝。
    pub fn resolve_module(&self, base: &str, specifier: &str) -> Result<PathBuf, String> {
        if specifier.contains('\0') {
            return Err("module specifier must not contain NUL".to_string());
        }
        if specifier.starts_with("./") || specifier.starts_with("../") {
            let base_dir = Path::new(base)
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| self.scripts_dir.clone());
            let direct = path_clean::clean(base_dir.join(specifier));
            for candidate in [
                direct.clone(),
                path_clean::clean(format!("{}.js", direct.display())),
                path_clean::clean(direct.join("index.js")),
            ] {
                if candidate.starts_with(&self.scripts_dir) && candidate.is_file() {
                    return Ok(candidate);
                }
            }
            Err(format!(
                "module `{specifier}` was not found inside the skill scripts directory"
            ))
        } else {
            if Path::new(specifier).is_absolute() {
                return Err(format!(
                    "module `{specifier}` must not be an absolute path"
                ));
            }
            for candidate in [
                self.libs_dir.join(format!("{specifier}.js")),
                self.libs_dir.join(specifier).join("index.js"),
            ] {
                if candidate.is_file() {
                    return Ok(candidate);
                }
            }
            Err(format!(
                "module `{specifier}` was not found in the public skill libraries"
            ))
        }
    }
```

文件顶部确保有 `use path_clean::clean as _;` 不需要——直接以 `path_clean::clean(...)` 全路径调用（Cargo.toml 已有 `path-clean` 依赖）。

修正步骤 3.2 测试中 `bare_modules_resolve_to_public_libs` 里那行冗余断言为简单形式：

```rust
        assert!(index.to_string_lossy().contains("markdown"));
```

- [ ] **步骤 3.5：运行测试验证通过**

运行：`cargo test -p tt-adapter-quickjs sandbox`
预期：PASS（5 个测试）

- [ ] **步骤 3.6：Commit**

```bash
git add src-tauri/crates/tt-ports/src/skill_script.rs src-tauri/crates/tt-ports/src/lib.rs src-tauri/crates/tt-ports/Cargo.toml src-tauri/crates/tt-adapter-quickjs/src/sandbox.rs
git commit -m "feat(skill-script): add SkillScriptEngine port and sandbox IO policy"
```

---

### 任务 4：QuickJS 引擎重写（隔离执行 + 模块加载 + 超时 + 上限 + API 注入）

**文件：**
- 创建：`src-tauri/crates/tt-adapter-quickjs/src/convert.rs`
- 重写：`src-tauri/crates/tt-adapter-quickjs/src/engine.rs`
- 重写：`src-tauri/crates/tt-adapter-quickjs/src/api/fs.rs`
- 重写：`src-tauri/crates/tt-adapter-quickjs/src/api/world_info.rs`
- 重写：`src-tauri/crates/tt-adapter-quickjs/src/api/log.rs`
- 修改：`src-tauri/crates/tt-adapter-quickjs/src/api/mod.rs`
- 修改：`src-tauri/crates/tt-adapter-quickjs/src/lib.rs`
- 删除：`src-tauri/crates/tt-adapter-quickjs/src/tool/`（descriptor.rs、executor.rs、mod.rs）
- 修改：`src-tauri/crates/tt-adapter-quickjs/Cargo.toml`

- [ ] **步骤 4.1：Cargo.toml 与模块布局调整**

`tt-adapter-quickjs/Cargo.toml` 依赖区改为（去掉 `anyhow`、`thiserror`；其余保留）：

```toml
[dependencies]
rquickjs = { version = "0.9", features = ["macro", "parallel"] }
async-trait = "0.1"
serde = { workspace = true }
serde_json = { workspace = true }
tokio = { workspace = true }
log = { workspace = true }
tt-domain = { path = "../tt-domain" }
tt-ports = { path = "../tt-ports" }
path-clean = "1.0"

[dev-dependencies]
tempfile = "3"
```

删除 `src/tool/` 目录。`src/lib.rs` 重写为：

```rust
//! QuickJS 沙箱脚本引擎（`SkillScriptEngine` 的 adapter 实现）。

pub mod api;
pub mod convert;
pub mod engine;
pub mod sandbox;

pub use engine::{
    DEFAULT_EXECUTION_TIMEOUT, DEFAULT_MAX_RESULT_BYTES, QuickJsScriptEngine,
};
pub use sandbox::SandboxIoPolicy;
```

`src/api/mod.rs` 重写为：

```rust
//! 注入脚本全局对象的 API（`$fs` / `$worldInfo` / `$log`）。

pub mod fs;
pub mod log;
pub mod world_info;

pub(crate) use fs::register_fs_api;
pub(crate) use log::register_log_api;
pub(crate) use world_info::register_world_info_api;
```

- [ ] **步骤 4.2：convert.rs（JSON ↔ JS 值转换）**

创建 `src/convert.rs`：

```rust
//! serde_json::Value 与 rquickjs 值互转（仅覆盖 JSON 可表达类型）。

use rquickjs::{Ctx, Value as JsValue};
use serde_json::Value as JsonValue;

pub(crate) fn json_to_js<'js>(
    ctx: &Ctx<'js>,
    value: &JsonValue,
) -> rquickjs::Result<JsValue<'js>> {
    match value {
        JsonValue::Null => Ok(JsValue::new_null(ctx.clone())),
        JsonValue::Bool(value) => Ok(JsValue::new_bool(ctx.clone(), *value)),
        JsonValue::Number(value) => {
            if let Some(int) = value.as_i64() {
                Ok(JsValue::new_int64(ctx.clone(), int))
            } else if let Some(float) = value.as_f64() {
                Ok(JsValue::new_float(ctx.clone(), float))
            } else {
                Ok(JsValue::new_number(ctx.clone(), value.to_string().parse().unwrap_or(0.0)))
            }
        }
        JsonValue::String(value) => Ok(JsValue::new_string(ctx.clone(), value.as_str())),
        JsonValue::Array(items) => {
            let array = rquickjs::Array::new(ctx.clone())?;
            for (index, item) in items.iter().enumerate() {
                array.set(index, json_to_js(ctx, item)?)?;
            }
            Ok(array.into_value())
        }
        JsonValue::Object(fields) => {
            let object = rquickjs::Object::new(ctx.clone())?;
            for (key, field) in fields.iter() {
                object.set(key.as_str(), json_to_js(ctx, field)?)?;
            }
            Ok(object.into_value())
        }
    }
}

pub(crate) fn js_to_json<'js>(ctx: &Ctx<'js>, value: &JsValue<'js>) -> rquickjs::Result<JsonValue> {
    if value.is_undefined() || value.is_null() {
        return Ok(JsonValue::Null);
    }
    if let Some(value) = value.as_bool() {
        return Ok(JsonValue::Bool(value));
    }
    if let Some(value) = value.as_float() {
        if let Some(int) = as_exact_int(value) {
            return Ok(JsonValue::Number(int.into()));
        }
        return serde_json::Number::from_f64(value)
            .map(JsonValue::Number)
            .ok_or_else(|| rquickjs::Error::Unknown);
    }
    if let Some(value) = value.as_string() {
        let text = value.as_str()?.to_string();
        return Ok(JsonValue::String(text));
    }
    if let Some(array) = value.as_array() {
        let mut items = Vec::with_capacity(array.len());
        for item in array.iter::<JsValue>() {
            items.push(js_to_json(ctx, &item?)?);
        }
        return Ok(JsonValue::Array(items));
    }
    if let Some(object) = value.as_object() {
        let mut fields = serde_json::Map::new();
        for property in object.props::<rquickjs::String, JsValue>() {
            let (key, field) = property?;
            fields.insert(key.as_str()?.to_string(), js_to_json(ctx, &field)?);
        }
        return Ok(JsonValue::Object(fields));
    }
    Ok(JsonValue::Null)
}

fn as_exact_int(value: f64) -> Option<i64> {
    if value.fract() == 0.0 && value.is_finite() && value.abs() <= 9.007_199_254_740_992e15 {
        Some(value as i64)
    } else {
        None
    }
}
```

- [ ] **步骤 4.3：重写 api/fs.rs（同步 std::fs + 门控）**

```rust
//! `$fs`：受 SandboxIoPolicy 门控的同步文件 API（执行线程已在 spawn_blocking 中）。

use rquickjs::{Ctx, Function, Object};

use crate::sandbox::SandboxIoPolicy;

fn js_error<'js>(ctx: &Ctx<'js>, message: String) -> rquickjs::Error {
    rquickjs::Exception::throw_message(ctx, &message)
}

pub(crate) fn register_fs_api<'js>(
    ctx: &Ctx<'js>,
    policy: SandboxIoPolicy,
) -> rquickjs::Result<()> {
    let globals = ctx.globals();
    let fs_object = Object::new(ctx.clone())?;

    let read_policy = policy.clone();
    let read_text = Function::new(
        ctx.clone(),
        move |ctx: Ctx<'_>, path: String| -> Result<String, rquickjs::Error> {
            let target = read_policy
                .check_read(&path)
                .map_err(|message| js_error(&ctx, message))?;
            std::fs::read_to_string(&target)
                .map_err(|error| js_error(&ctx, format!("failed to read `{path}`: {error}")))
        },
    )?;

    let write_policy = policy.clone();
    let write_text = Function::new(
        ctx.clone(),
        move |ctx: Ctx<'_>, path: String, content: String| -> Result<(), rquickjs::Error> {
            let target = write_policy
                .check_write(&path)
                .map_err(|message| js_error(&ctx, message))?;
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|error| js_error(&ctx, format!("failed to create directory for `{path}`: {error}")))?;
            }
            std::fs::write(&target, content)
                .map_err(|error| js_error(&ctx, format!("failed to write `{path}`: {error}")))
        },
    )?;

    let list_policy = policy.clone();
    let list_files = Function::new(
        ctx.clone(),
        move |ctx: Ctx<'_>, path: Option<String>| -> Result<Vec<String>, rquickjs::Error> {
            // 无参：列出 work_dir 顶层条目名（仅名字，无内容）；
            // 有参：读取该目录下条目的 work_dir 相对路径，读权限同 check_read。
            let mut entries = Vec::new();
            let base = match path.as_deref() {
                None => list_policy.work_dir.clone(),
                Some(path) => list_policy
                    .check_read(path)
                    .map_err(|message| js_error(&ctx, message))?,
            };
            let directory = std::fs::read_dir(&base)
                .map_err(|error| js_error(&ctx, format!("failed to list `{}`: {error}", base.display())))?;
            for entry in directory {
                let entry = entry
                    .map_err(|error| js_error(&ctx, format!("failed to read entry: {error}")))?;
                let name = entry.file_name().to_string_lossy().to_string();
                entries.push(match path.as_deref() {
                    None => name,
                    Some(prefix) => {
                        let prefix = prefix.trim_end_matches(['/', '\\']);
                        format!("{prefix}/{name}")
                    }
                });
            }
            entries.sort();
            Ok(entries)
        },
    )?;

    let exists_policy = policy.clone();
    let exists = Function::new(
        ctx.clone(),
        move |ctx: Ctx<'_>, path: String| -> Result<bool, rquickjs::Error> {
            match exists_policy.check_read(&path) {
                Ok(target) => Ok(target.is_file() || target.is_dir()),
                Err(_) => Ok(false),
            }
        },
    )?;

    fs_object.set("readText", read_text)?;
    fs_object.set("writeText", write_text)?;
    fs_object.set("listFiles", list_files)?;
    fs_object.set("exists", exists)?;
    globals.set("$fs", fs_object)?;
    Ok(())
}
```

- [ ] **步骤 4.4：重写 api/world_info.rs 与 api/log.rs**

`world_info.rs`（改用 domain 模型，产出 JSON 后经 convert 注入）：

```rust
//! `$worldInfo`：读取预取的激活世界书快照（只读）。

use rquickjs::{Ctx, Function, Object};
use serde::Serialize;
use serde_json::json;

use tt_domain::models::skill_script::ActivatedWorldInfoEntry;

use crate::convert::json_to_js;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ScriptWorldInfoEntry<'a> {
    uid: &'a str,
    #[serde(rename = "ref")]
    ref_key: &'a str,
    content: &'a str,
    constant: bool,
    position: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    display_name: Option<&'a str>,
    world: &'a str,
}

fn entries_json(entries: &[ActivatedWorldInfoEntry]) -> serde_json::Value {
    json!({
        "entries": entries
            .iter()
            .map(|entry| serde_json::to_value(ScriptWorldInfoEntry {
                uid: entry.uid.as_str(),
                ref_key: entry.ref_key.as_str(),
                content: entry.content.as_str(),
                constant: entry.constant,
                position: entry.position.as_deref(),
                display_name: entry.display_name.as_deref(),
                world: entry.world.as_str(),
            })
            .unwrap_or(serde_json::Value::Null))
            .collect::<Vec<_>>(),
    })
}

pub(crate) fn register_world_info_api<'js>(
    ctx: &Ctx<'js>,
    entries: Vec<ActivatedWorldInfoEntry>,
) -> rquickjs::Result<()> {
    let globals = ctx.globals();
    let object = Object::new(ctx.clone())?;

    let activated = entries.clone();
    let read_activated = Function::new(ctx.clone(), move || entries_json(&activated))?;

    let filtered = entries;
    let read_entries = Function::new(
        ctx.clone(),
        move |refs: Vec<String>| -> serde_json::Value {
            let filtered: Vec<_> = filtered
                .iter()
                .filter(|entry| refs.contains(&entry.ref_key))
                .cloned()
                .collect();
            entries_json(&filtered)
        },
    )?;

    object.set("readActivated", read_activated)?;
    object.set("readEntries", read_entries)?;
    globals.set("$worldInfo", object)?;
    Ok(())
}
```

注意：`Function::new` 返回 `serde_json::Value` 时 rquickjs 无法直接转换——两个闭包统一改为返回 `rquickjs::Result<rquickjs::Value>` 并在闭包内调用 `json_to_js(&ctx, &entries_json(...))`（闭包签名带 `ctx: Ctx<'_>` 首参，同 fs.rs 模式）。实现时按此形态写：

```rust
    let activated = entries.clone();
    let read_activated = Function::new(
        ctx.clone(),
        move |ctx: Ctx<'_>| -> rquickjs::Result<rquickjs::Value> {
            json_to_js(&ctx, &entries_json(&activated))
        },
    )?;

    let filtered = entries;
    let read_entries = Function::new(
        ctx.clone(),
        move |ctx: Ctx<'_>, refs: Vec<String>| -> rquickjs::Result<rquickjs::Value> {
            let selected: Vec<_> = filtered
                .iter()
                .filter(|entry| refs.contains(&entry.ref_key))
                .cloned()
                .collect();
            json_to_js(&ctx, &entries_json(&selected))
        },
    )?;
```

`log.rs`：

```rust
//! `$log`：脚本日志输出到宿主 log（无返回值）。

use rquickjs::{Ctx, Function, Object};

pub(crate) fn register_log_api<'js>(ctx: &Ctx<'js>) -> rquickjs::Result<()> {
    let globals = ctx.globals();
    let object = Object::new(ctx.clone())?;
    for (name, level) in [
        ("info", log::Level::Info),
        ("warn", log::Level::Warn),
        ("error", log::Level::Error),
        ("debug", log::Level::Debug),
    ] {
        let function = Function::new(ctx.clone(), move |message: String| {
            log::log!(level, "[skill-script] {message}");
        })?;
        object.set(name, function)?;
    }
    globals.set("$log", object)?;
    Ok(())
}
```

- [ ] **步骤 4.5：编写失败的引擎测试**

重写 `engine.rs`——先写测试模块（实现骨架为空函数签名，测试无法编译/失败即满足）：

```rust
//! `SkillScriptEngine` 实现：每次执行独立 Runtime+Context，spawn_blocking 中运行，
//! 30s 超时中断、32MB 内存/256KB 栈限制、256KB 返回值上限、模块白名单加载。

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use rquickjs::{Context, Ctx, Function, Module, Runtime, Value as JsValue};
use tokio::task::spawn_blocking;

use tt_domain::errors::DomainError;
use tt_ports::skill_script::{SkillScriptEngine, SkillScriptRequest, SkillScriptResult};

use crate::api::{register_fs_api, register_log_api, register_world_info_api};
use crate::convert::{json_to_js, js_to_json};
use crate::sandbox::SandboxIoPolicy;

pub const DEFAULT_EXECUTION_TIMEOUT: Duration = Duration::from_secs(30);
pub const DEFAULT_MAX_RESULT_BYTES: usize = 256 * 1024;
const MEMORY_LIMIT_BYTES: u64 = 32 * 1024 * 1024;
const MAX_STACK_BYTES: u64 = 256 * 1024;

pub struct QuickJsScriptEngine {
    libs_dir: PathBuf,
    timeout: Duration,
    max_result_bytes: usize,
}

impl QuickJsScriptEngine {
    pub fn new(libs_dir: PathBuf) -> Self {
        Self {
            libs_dir,
            timeout: DEFAULT_EXECUTION_TIMEOUT,
            max_result_bytes: DEFAULT_MAX_RESULT_BYTES,
        }
    }

    /// 测试与装配侧收紧限制的构造器。
    pub fn with_limits(mut self, timeout: Duration, max_result_bytes: usize) -> Self {
        self.timeout = timeout;
        self.max_result_bytes = max_result_bytes;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::QuickJsScriptEngine;
    use crate::sandbox::SandboxIoPolicy;
    use serde_json::json;
    use std::path::PathBuf;
    use std::time::Duration;
    use tempfile::TempDir;
    use tt_domain::errors::DomainError;
    use tt_domain::models::skill_script::ActivatedWorldInfoEntry;
    use tt_ports::skill_script::SkillScriptRequest;

    struct Fixture {
        _temp: TempDir,
        scripts_dir: PathBuf,
        work_dir: PathBuf,
        libs_dir: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let temp = TempDir::new().expect("tempdir");
            let scripts_dir = temp.path().join("skills/demo/scripts");
            let work_dir = temp.path().join("work");
            let libs_dir = temp.path().join("skill-libs");
            std::fs::create_dir_all(&scripts_dir).expect("scripts dir");
            std::fs::create_dir_all(&work_dir).expect("work dir");
            std::fs::create_dir_all(&libs_dir).expect("libs dir");
            Self {
                _temp: temp,
                scripts_dir,
                work_dir,
                libs_dir,
            }
        }

        fn write_script(&self, name: &str, source: &str) -> PathBuf {
            let path = self.scripts_dir.join(name);
            std::fs::write(&path, source).expect("write script");
            path
        }

        fn request(&self, script_path: PathBuf, args: serde_json::Value) -> SkillScriptRequest {
            SkillScriptRequest {
                script_path,
                args,
                work_dir: self.work_dir.clone(),
                visible_roots: vec!["output".to_string()],
                writable_roots: vec!["output".to_string()],
                world_info_entries: Vec::new(),
            }
        }

        fn engine(&self) -> QuickJsScriptEngine {
            QuickJsScriptEngine::new(self.libs_dir.clone())
        }
    }

    #[tokio::test]
    async fn executes_default_export_with_args() {
        let fixture = Fixture::new();
        let script = fixture.write_script(
            "add.js",
            "export default function (args) { return { sum: args.a + args.b }; }",
        );

        let result = fixture
            .engine()
            .execute(fixture.request(script, json!({ "a": 20, "b": 22 })))
            .await
            .expect("execute");

        assert_eq!(result.value, json!({ "sum": 42 }));
    }

    #[tokio::test]
    async fn falls_back_to_main_export() {
        let fixture = Fixture::new();
        let script = fixture.write_script(
            "main.js",
            "export function main(args) { return args.value; }",
        );

        let result = fixture
            .engine()
            .execute(fixture.request(script, json!({ "value": "ok" })))
            .await
            .expect("execute");

        assert_eq!(result.value, json!("ok"));
    }

    #[tokio::test]
    async fn propagates_exception_message_and_stack() {
        let fixture = Fixture::new();
        let script = fixture.write_script(
            "boom.js",
            "export default function () { throw new Error('kaboom'); }",
        );

        let error = fixture
            .engine()
            .execute(fixture.request(script, json!({})))
            .await
            .expect_err("must fail");

        match error {
            DomainError::SkillScriptExecutionFailed { message } => {
                assert!(message.contains("kaboom"), "message was: {message}");
                assert!(message.contains("boom.js"), "message was: {message}");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[tokio::test]
    async fn timeout_interrupts_infinite_loop() {
        let fixture = Fixture::new();
        let script = fixture.write_script(
            "loop.js",
            "export default function () { while (true) {} }",
        );
        let engine = QuickJsScriptEngine::new(fixture.libs_dir.clone())
            .with_limits(Duration::from_millis(200), 256 * 1024);

        let error = engine
            .execute(fixture.request(script, json!({})))
            .await
            .expect_err("must time out");

        match error {
            DomainError::SkillScriptExecutionFailed { message } => {
                assert!(message.contains("timed out"), "message was: {message}");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[tokio::test]
    async fn result_size_limit_is_enforced() {
        let fixture = Fixture::new();
        let script = fixture.write_script(
            "big.js",
            "export default function () { return 'x'.repeat(1024); }",
        );
        let engine = QuickJsScriptEngine::new(fixture.libs_dir.clone())
            .with_limits(Duration::from_secs(5), 512);

        let error = engine
            .execute(fixture.request(script, json!({})))
            .await
            .expect_err("must exceed");

        assert!(matches!(
            error,
            DomainError::SkillScriptResultTooLarge { .. }
        ));
    }

    #[tokio::test]
    async fn relative_imports_stay_inside_scripts_dir() {
        let fixture = Fixture::new();
        std::fs::write(
            fixture.scripts_dir.join("helper.js"),
            "export const value = 7;",
        )
        .expect("write helper");
        let script = fixture.write_script(
            "main.js",
            "import { value } from './helper.js';\nexport default function () { return value; }",
        );

        let result = fixture
            .engine()
            .execute(fixture.request(script, json!({})))
            .await
            .expect("execute");

        assert_eq!(result.value, json!(7));
    }

    #[tokio::test]
    async fn bare_imports_resolve_from_public_libs() {
        let fixture = Fixture::new();
        std::fs::write(
            fixture.libs_dir.join("mathlib.js"),
            "export const triple = (x) => 3 * x;",
        )
        .expect("write lib");
        let script = fixture.write_script(
            "main.js",
            "import { triple } from 'mathlib';\nexport default function () { return triple(5); }",
        );

        let result = fixture
            .engine()
            .execute(fixture.request(script, json!({})))
            .await
            .expect("execute");

        assert_eq!(result.value, json!(15));
    }

    #[tokio::test]
    async fn imports_escaping_scripts_dir_are_rejected() {
        let fixture = Fixture::new();
        std::fs::write(
            fixture.scripts_dir.parent().unwrap().join("SKILL.md"),
            "# skill",
        )
        .expect("write sibling file");
        let script = fixture.write_script(
            "escape.js",
            "import data from '../SKILL.md';\nexport default function () { return 1; }",
        );

        let error = fixture
            .engine()
            .execute(fixture.request(script, json!({})))
            .await
            .expect_err("must reject");

        assert!(matches!(error, DomainError::SkillScriptExecutionFailed { .. }));
    }

    #[tokio::test]
    async fn fs_api_reads_and_writes_within_gated_roots() {
        let fixture = Fixture::new();
        let script = fixture.write_script(
            "io.js",
            "export default function () {\n\
             \x20 $fs.writeText('output/note.txt', 'hello');\n\
             \x20 return $fs.readText('output/note.txt');\n\
             }",
        );

        let result = fixture
            .engine()
            .execute(fixture.request(script, json!({})))
            .await
            .expect("execute");

        assert_eq!(result.value, json!("hello"));
        assert!(fixture.work_dir.join("output/note.txt").is_file());

        let denied = fixture.write_script(
            "deny.js",
            "export default function () { return $fs.readText('input/prompt_snapshot.json'); }",
        );
        let error = fixture
            .engine()
            .execute(fixture.request(denied, json!({})))
            .await
            .expect_err("read outside visible roots must fail");
        assert!(matches!(error, DomainError::SkillScriptExecutionFailed { .. }));
    }

    #[tokio::test]
    async fn world_info_snapshot_is_readable() {
        let fixture = Fixture::new();
        let script = fixture.write_script(
            "wi.js",
            "export default function () { return $worldInfo.readActivated(); }",
        );
        let mut request = fixture.request(script, json!({}));
        request.world_info_entries = vec![ActivatedWorldInfoEntry {
            world: "lore".to_string(),
            uid: "1".to_string(),
            display_name: None,
            constant: true,
            position: None,
            content: "text".to_string(),
            ref_key: "worldinfo:lore#1".to_string(),
        }];

        let result = fixture.engine().execute(request).await.expect("execute");

        assert_eq!(
            result.value,
            json!({ "entries": [{ "uid": "1", "ref": "worldinfo:lore#1", "content": "text", "constant": true, "world": "lore" }] })
        );
    }
}
```

- [ ] **步骤 4.6：运行测试验证失败**

运行：`cargo test -p tt-adapter-quickjs`
预期：FAIL（`SkillScriptEngine` 未实现）

- [ ] **步骤 4.7：实现引擎**

在 `engine.rs` 中补全（与上方结构体定义合并）：

```rust
#[async_trait]
impl SkillScriptEngine for QuickJsScriptEngine {
    async fn execute(&self, request: SkillScriptRequest) -> Result<SkillScriptResult, DomainError> {
        let scripts_dir = request
            .script_path
            .parent()
            .map(PathBuf::from)
            .ok_or_else(|| {
                DomainError::InvalidData(
                    "skill script path must have a parent directory".to_string(),
                )
            })?;
        let policy = SandboxIoPolicy::new(
            request.work_dir.clone(),
            request.visible_roots.clone(),
            request.writable_roots.clone(),
            scripts_dir,
            self.libs_dir.clone(),
        );
        let timeout = self.timeout;
        let max_result_bytes = self.max_result_bytes;
        spawn_blocking(move || execute_sync(request, policy, timeout, max_result_bytes))
            .await
            .map_err(|error| {
                DomainError::InternalError(format!("Skill script engine task failed: {error}"))
            })?
    }
}

fn internal_error(error: rquickjs::Error) -> DomainError {
    DomainError::InternalError(format!("QuickJS runtime failure: {error}"))
}

fn execute_sync(
    request: SkillScriptRequest,
    policy: SandboxIoPolicy,
    timeout: Duration,
    max_result_bytes: usize,
) -> Result<SkillScriptResult, DomainError> {
    // 每次执行全新的 Runtime + Context：无跨执行共享状态（项目既定教训）。
    let entry_source = std::fs::read_to_string(&request.script_path).map_err(|error| {
        DomainError::InternalError(format!(
            "Failed to read skill script {}: {error}",
            request.script_path.display()
        ))
    })?;

    let runtime = Runtime::new().map_err(internal_error)?;
    runtime
        .set_memory_limit(MEMORY_LIMIT_BYTES)
        .map_err(internal_error)?;
    runtime
        .set_max_stack_size(MAX_STACK_BYTES)
        .map_err(internal_error)?;

    let deadline = Instant::now() + timeout;
    let timed_out = Arc::new(AtomicBool::new(false));
    let interrupt_flag = timed_out.clone();
    runtime.set_interrupt_handler(Some(Box::new(move || {
        if Instant::now() >= deadline {
            interrupt_flag.store(true, Ordering::SeqCst);
            return true;
        }
        false
    })));

    install_module_loader(&runtime, policy.clone());

    let context = Context::full(&runtime).map_err(internal_error)?;
    let module_name = request.script_path.to_string_lossy().to_string();
    let outcome = context.with(|ctx| {
        register_fs_api(&ctx, policy.clone())?;
        register_world_info_api(&ctx, request.world_info_entries.clone())?;
        register_log_api(&ctx)?;

        let module = Module::declare(&ctx, module_name.clone(), entry_source)?;
        module.evaluate::<JsValue>()?;

        let args = json_to_js(&ctx, &request.args)?;
        let entry_result = if let Ok(function) = module.get::<_, Function>("default") {
            function.call::<_, JsValue>((args,))?
        } else if let Ok(function) = module.get::<_, Function>("main") {
            function.call::<_, JsValue>((args,))?
        } else {
            JsValue::new_undefined(ctx.clone())
        };

        js_to_json(&ctx, &entry_result)
    });

    match outcome {
        Ok(value) => {
            let encoded = serde_json::to_string(&value).map_err(|error| {
                DomainError::skill_script_execution_failed(format!(
                    "Failed to serialize skill script result: {error}"
                ))
            })?;
            if encoded.len() > max_result_bytes {
                return Err(DomainError::SkillScriptResultTooLarge {
                    actual_bytes: encoded.len(),
                    limit_bytes: max_result_bytes,
                });
            }
            Ok(SkillScriptResult { value })
        }
        Err(error) => {
            if timed_out.load(Ordering::SeqCst) {
                return Err(DomainError::skill_script_execution_failed(format!(
                    "Skill script {} timed out after {:.1}s and was interrupted.",
                    request.script_path.display(),
                    timeout.as_secs_f64()
                )));
            }
            let detail = context.with(|ctx| format_exception(&ctx, &error));
            Err(DomainError::skill_script_execution_failed(format!(
                "Skill script {} failed: {detail}",
                request.script_path.display()
            )))
        }
    }
}

/// 模块加载白名单：normalizer 用 SandboxIoPolicy 解析并校验；loader 再做
/// 目录前缀防御并同步读文件。
fn install_module_loader(runtime: &Runtime, policy: SandboxIoPolicy) {
    let loader_policy = policy.clone();
    let resolver_policy = policy;
    runtime.set_module_loader(
        move |_ctx: &Ctx<'_>, name: &str| -> Result<Vec<u8>, rquickjs::Error> {
            let path = PathBuf::from(name);
            if !(path.starts_with(&loader_policy.scripts_dir)
                || path.starts_with(&loader_policy.libs_dir))
            {
                return Err(rquickjs::Error::new_loading_failure(format!(
                    "module `{name}` is outside the allowed script directories"
                )));
            }
            std::fs::read(&path).map_err(|error| {
                rquickjs::Error::new_loading_failure(format!(
                    "failed to load module `{name}`: {error}"
                ))
            })
        },
        move |base: &str, specifier: &str, _raw: *mut std::ffi::c_char| {
            resolver_policy
                .resolve_module(base, specifier)
                .map(|path| path.to_string_lossy().to_string())
                .map_err(rquickjs::Error::new_loading_failure)
        },
    );
}

/// 提取 JS 异常的 message 与 stack（如可用），否则回退到错误字符串。
fn format_exception(ctx: &Ctx<'_>, error: &rquickjs::Error) -> String {
    if !matches!(error, rquickjs::Error::Exception) {
        return error.to_string();
    }
    let Some(exception) = ctx.catch() else {
        return "unknown JavaScript exception".to_string();
    };
    let Some(object) = exception.as_object() else {
        return format!("JavaScript exception: {exception:?}");
    };
    let message = object
        .get::<_, JsValue>("message")
        .ok()
        .and_then(|value| value.as_string().and_then(|s| s.as_str().ok().map(str::to_string)));
    let stack = object
        .get::<_, JsValue>("stack")
        .ok()
        .and_then(|value| value.as_string().and_then(|s| s.as_str().ok().map(str::to_string)));
    match (message, stack) {
        (Some(message), Some(stack)) => format!("{message}\n{stack}"),
        (Some(message), None) => message,
        (None, Some(stack)) => stack,
        (None, None) => "JavaScript exception without message".to_string(),
    }
}
```

**rquickjs 适配提示**（允许且仅允许调整调用形态，不改语义）：
- `set_module_loader` 的闭包式 loader/normalizer 签名以 0.9 文档/编译器为准（normalizer 第三参 `*mut c_char` 可能为 `*const c_char` 或 `Context`）。
- `rquickjs::Error::new_loading_failure` 若不存在，改用 0.9 提供的等价加载失败构造（如 `Error::LoadingFailed` 变体或 `Exception::throw_message`）。
- `Module::evaluate` 的泛型参数按 0.9 实际签名调整。
- Windows 路径含 `\` 时模块名比较使用 `PathBuf::starts_with`（已按此写）。

- [ ] **步骤 4.8：运行测试验证通过**

运行：`cargo test -p tt-adapter-quickjs`
预期：PASS（sandbox 5 + engine 10）

- [ ] **步骤 4.9：边界检查 + Commit**

运行：`node scripts/check-rust-crate-boundaries.mjs`
预期：通过（adapter 不引用 tt_application/tauritavern/tauri）

```bash
git add src-tauri/crates/tt-adapter-quickjs/
git commit -m "feat(skill-script): rewrite quickjs engine as isolated SkillScriptEngine adapter"
```

---

### 任务 5：应用层 — skill.script 处理器 + 注册 + 分发

**文件：**
- 创建：`src-tauri/crates/tt-application/src/services/agent_tools/skill/script.rs`
- 修改：`src-tauri/crates/tt-application/src/services/agent_tools/skill/descriptors.rs`
- 修改：`src-tauri/crates/tt-application/src/services/agent_tools/skill/mod.rs`
- 修改：`src-tauri/crates/tt-application/src/services/agent_tools/registry.rs`
- 修改：`src-tauri/crates/tt-application/src/services/agent_tools/dispatcher.rs`
- 修改：`src-tauri/crates/tt-application/src/services/agent_runtime_service.rs`

- [ ] **步骤 5.1：编写失败的处理器测试**

创建 `skill/script.rs`，先写处理器签名骨架与完整测试模块：

```rust
use serde_json::Value;

use super::super::common::{
    ensure_only_args, object_args, required_trimmed_string_arg, tool_error,
};
use super::super::dispatcher::AgentToolEffect;
use super::super::session::AgentToolSession;
use super::list::skill_is_visible;
use crate::errors::ApplicationError;
use crate::services::skill_service::SkillService;
use tt_domain::errors::DomainError;
use tt_domain::models::agent::profile::ResolvedAgentProfile;
use tt_domain::models::agent::AgentToolResult;
use tt_domain::models::skill::SkillScope;
use tt_domain::models::skill_script::ActivatedWorldInfoEntry;
use tt_domain::models::tool::ToolInvocation;
use tt_ports::repositories::workspace_repository::WorkspaceRepository;
use tt_ports::skill_script::{SkillScriptEngine, SkillScriptRequest, SkillScriptResult};

pub(in crate::services::agent_tools) async fn script(
    _skill_service: &SkillService,
    _engine: &dyn SkillScriptEngine,
    _workspace_repository: &dyn WorkspaceRepository,
    _run_id: &str,
    _prompt_snapshot: Option<&Value>,
    _call: &ToolInvocation,
    _session: &mut AgentToolSession,
    _profile: &ResolvedAgentProfile,
) -> Result<(AgentToolResult, AgentToolEffect), ApplicationError> {
    unreachable!("implemented in step 5.3")
}

fn is_valid_script_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_ascii_lowercase() || first.is_ascii_digit() => {}
        _ => return false,
    }
    chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
}
```

测试模块（同文件底部）：

```rust
#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use async_trait::async_trait;
    use serde_json::{Value, json};
    use tokio::sync::Mutex;

    use super::super::super::structured::structured_value;
    use super::*;
    use crate::services::skill_service::SkillService;
    use tt_domain::models::agent::profile::{
        AgentSkillPolicy, AgentToolPolicy, AgentWorkspacePolicy,
    };
    use tt_domain::models::skill::{
        SkillExportResult, SkillFileRef, SkillImportInput, SkillImportPreview,
        SkillIndexEntry, SkillInstallRequest, SkillInstallResult, SkillMoveRequest,
        SkillReadRequest, SkillReadResult, SkillScopeFilter, SkillScopeRetargetRequest,
        SkillScopeRetargetResult, SkillSearchRequest, SkillSearchResult, SkillWriteRequest,
    };
    use tt_domain::models::tool::ToolId;
    use tt_ports::repositories::skill_repository::SkillRepository;

    // ---- fakes ----------------------------------------------------------

    enum FakeOutcome {
        Ok(Value),
        Failed(String),
        TooLarge { actual_bytes: usize, limit_bytes: usize },
    }

    struct FakeScriptEngine {
        outcome: FakeOutcome,
        requests: Mutex<Vec<SkillScriptRequest>>,
    }

    #[async_trait]
    impl SkillScriptEngine for FakeScriptEngine {
        async fn execute(
            &self,
            request: SkillScriptRequest,
        ) -> Result<SkillScriptResult, DomainError> {
            self.requests.lock().await.push(request);
            match &self.outcome {
                FakeOutcome::Ok(value) => Ok(SkillScriptResult { value: value.clone() }),
                FakeOutcome::Failed(message) => Err(DomainError::skill_script_execution_failed(message.clone())),
                FakeOutcome::TooLarge { actual_bytes, limit_bytes } => {
                    Err(DomainError::SkillScriptResultTooLarge {
                        actual_bytes: *actual_bytes,
                        limit_bytes: *limit_bytes,
                    })
                }
            }
        }
    }

    struct FakeSkillRepo {
        script_path: Option<PathBuf>,
    }

    #[async_trait]
    impl SkillRepository for FakeSkillRepo {
        async fn list_skills(
            &self,
            _filter: SkillScopeFilter,
        ) -> Result<Vec<SkillIndexEntry>, DomainError> {
            Ok(Vec::new())
        }
        async fn list_skill_files(
            &self,
            _scope: SkillScope,
            _name: &str,
        ) -> Result<Vec<SkillFileRef>, DomainError> {
            unreachable!("not needed")
        }
        async fn preview_import(
            &self,
            _input: SkillImportInput,
            _target: SkillScope,
        ) -> Result<SkillImportPreview, DomainError> {
            unreachable!("not needed")
        }
        async fn install_import(
            &self,
            _request: SkillInstallRequest,
        ) -> Result<SkillInstallResult, DomainError> {
            unreachable!("not needed")
        }
        async fn read_skill_file(
            &self,
            _request: SkillReadRequest,
        ) -> Result<SkillReadResult, DomainError> {
            unreachable!("not needed")
        }
        async fn write_skill_file(
            &self,
            _request: SkillWriteRequest,
        ) -> Result<SkillReadResult, DomainError> {
            unreachable!("not needed")
        }
        async fn search_skill_files(
            &self,
            _request: SkillSearchRequest,
        ) -> Result<SkillSearchResult, DomainError> {
            unreachable!("not needed")
        }
        async fn export_skill(
            &self,
            _scope: SkillScope,
            _name: &str,
        ) -> Result<SkillExportResult, DomainError> {
            unreachable!("not needed")
        }
        async fn delete_skill(&self, _scope: SkillScope, _name: &str) -> Result<(), DomainError> {
            unreachable!("not needed")
        }
        async fn move_skill(
            &self,
            _request: SkillMoveRequest,
        ) -> Result<SkillInstallResult, DomainError> {
            unreachable!("not needed")
        }
        async fn retarget_scope(
            &self,
            _request: SkillScopeRetargetRequest,
        ) -> Result<SkillScopeRetargetResult, DomainError> {
            unreachable!("not needed")
        }
        async fn delete_skills_for_source(
            &self,
            _kind: &str,
            _id: &str,
        ) -> Result<Vec<String>, DomainError> {
            unreachable!("not needed")
        }
        async fn skill_file_path(
            &self,
            _scope: SkillScope,
            _name: &str,
            _relative_path: &str,
        ) -> Result<PathBuf, DomainError> {
            match &self.script_path {
                Some(path) => Ok(path.clone()),
                None => Err(DomainError::NotFound("Skill file not found".to_string())),
            }
        }
    }

    struct FakeWorkspaceRepo {
        root: PathBuf,
    }

    #[async_trait]
    impl WorkspaceRepository for FakeWorkspaceRepo {
        async fn run_workspace_root(&self, _run_id: &str) -> Result<PathBuf, DomainError> {
            Ok(self.root.clone())
        }
        // 其余方法按 trait 定义补 unreachable!("not needed") ——
        // initialize_run / read_manifest / write_text / write_text_guarded /
        // append_text / read_text / list_files / commit_persistent_changes。
    }

    // ---- helpers --------------------------------------------------------

    fn session_with_skill(name: &str) -> AgentToolSession {
        AgentToolSession::new(vec![SkillIndexEntry {
            scope: SkillScope::Global,
            name: name.to_string(),
            description: "test".to_string(),
            display_name: None,
            source_kind: None,
            license: None,
            author: None,
            version: None,
            tags: Vec::new(),
            installed_hash: "hash".to_string(),
            file_count: 1,
            total_bytes: 1,
            has_scripts: true,
            has_binary: false,
            installed_at: chrono::Utc::now(),
            source_refs: Vec::new(),
        }])
    }

    fn profile(visible: bool) -> ResolvedAgentProfile {
        // 复用 registry.rs 测试中 profile_with_skill_budget 的构造方式；
        // 此处给最小字段：skills.visible 按 visible 参数、workspace roots = ["output"]。
        ResolvedAgentProfile {
            // ... 与 registry.rs::profile_with_skill_budget 相同的字段集，
            // 仅以下字段不同：
            // skills.visible = if visible { vec!["*"] } else { vec![] },
            // workspace.visible_roots = vec!["output"], writable_roots = vec!["output"],
            // tools.allow 默认（处理器不检查 tools.allow；policy 层已门控）
            ...base_profile()
        }
    }

    fn call(arguments: Value) -> ToolInvocation {
        ToolInvocation {
            call_id: "call_skill_script".to_string(),
            tool_id: ToolId::builtin("skill.script").unwrap(),
            arguments,
            provider_metadata: Value::Null,
        }
    }

    // ---- tests ----------------------------------------------------------

    #[tokio::test]
    async fn invalid_script_name_is_rejected() {
        let (result, _) = run(
            json!({ "skill": "demo", "script": "Bad_Name" }),
            session_with_skill("demo"),
            profile(true),
        ).await;
        assert!(result.is_error);
        assert_eq!(result.error_code.as_deref(), Some("skill.script_invalid_name"));
        assert!(result.content.contains("SKILL.md"));
    }

    #[tokio::test]
    async fn invisible_skill_is_rejected() {
        let (result, _) = run(
            json!({ "skill": "demo", "script": "helper" }),
            session_with_skill("demo"),
            profile(false),
        ).await;
        assert_eq!(
            result.error_code.as_deref(),
            Some("skill.script_skill_not_visible")
        );
    }

    #[tokio::test]
    async fn missing_script_file_reports_not_found() {
        // FakeSkillRepo.script_path = None -> NotFound
        let (result, _) = run_with_repo(
            json!({ "skill": "demo", "script": "helper" }),
            FakeSkillRepo { script_path: None },
        ).await;
        assert_eq!(result.error_code.as_deref(), Some("skill.script_not_found"));
    }

    #[tokio::test]
    async fn execution_failure_keeps_full_message() {
        let (result, _) = run_with_outcome(
            json!({ "skill": "demo", "script": "helper" }),
            FakeOutcome::Failed("TypeError: x is not a function\n    at helper.js:3:9".to_string()),
        ).await;
        assert_eq!(
            result.error_code.as_deref(),
            Some("skill.script_execution_failed")
        );
        assert!(result.content.contains("TypeError: x is not a function"));
        assert!(result.content.contains("helper.js:3:9"));
    }

    #[tokio::test]
    async fn result_too_large_maps_dedicated_code() {
        let (result, _) = run_with_outcome(
            json!({ "skill": "demo", "script": "helper" }),
            FakeOutcome::TooLarge { actual_bytes: 300_000, limit_bytes: 262_144 },
        ).await;
        assert_eq!(
            result.error_code.as_deref(),
            Some("skill.script_result_too_large")
        );
        assert!(result.content.contains("$fs.writeText"));
    }

    #[tokio::test]
    async fn success_builds_result_and_passes_workspace_context() {
        let engine = Arc::new(FakeScriptEngine {
            outcome: FakeOutcome::Ok(json!({ "answer": 42 })),
            requests: Mutex::new(Vec::new()),
        });
        // ... 构造 service/repo/session/profile，调用 script(...)
        let requests = engine.requests.lock().await;
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].script_path, PathBuf::from("/fake/scripts/helper.js"));
        assert_eq!(requests[0].visible_roots, vec!["output".to_string()]);
        assert_eq!(requests[0].writable_roots, vec!["output".to_string()]);
        // result 组装
        // assert!(!result.is_error);
        // assert_eq!(result.structured, json!({ "answer": 42 }));
        // assert!(result.content.contains("demo/scripts/helper.js"));
    }
}
```

**实现者注**：测试 helper 中 `run` / `run_with_repo` / `run_with_outcome` / `base_profile` 由上述注释语义补全为具体函数——`base_profile` 直接照抄 `registry.rs` 测试里 `profile_with_skill_budget` 的字段字面量（那个函数是私有的，不能跨模块引用，需要复制），仅改 `skills.visible`、`workspace` 两处。`FakeWorkspaceRepo` 其余 trait 方法按 `workspace_repository.rs` 的 trait 定义逐个补 `unreachable!("not needed")`。

- [ ] **步骤 5.2：运行测试验证失败**

运行：`cargo test -p tt-application --lib services::agent_tools::skill`
预期：FAIL（`script` 为 `unreachable!`）

- [ ] **步骤 5.3：实现处理器**

替换 `script` 函数体：

```rust
const SKILL_SCRIPT_INVALID_NAME: &str = "skill.script_invalid_name";
const SKILL_SCRIPT_SKILL_NOT_VISIBLE: &str = "skill.script_skill_not_visible";
const SKILL_SCRIPT_NOT_FOUND: &str = "skill.script_not_found";
const SKILL_SCRIPT_EXECUTION_FAILED: &str = "skill.script_execution_failed";
const SKILL_SCRIPT_RESULT_TOO_LARGE: &str = "skill.script_result_too_large";

pub(in crate::services::agent_tools) async fn script(
    skill_service: &SkillService,
    engine: &dyn SkillScriptEngine,
    workspace_repository: &dyn WorkspaceRepository,
    run_id: &str,
    prompt_snapshot: Option<&Value>,
    call: &ToolInvocation,
    session: &mut AgentToolSession,
    profile: &ResolvedAgentProfile,
) -> Result<(AgentToolResult, AgentToolEffect), ApplicationError> {
    let Some(args) = object_args(call) else {
        return Ok((
            tool_error(call, "tool.invalid_arguments", "arguments must be an object"),
            AgentToolEffect::None,
        ));
    };
    if let Err(message) = ensure_only_args(args, &["skill", "script", "args"]) {
        return Ok((
            tool_error(call, "tool.invalid_arguments", &message),
            AgentToolEffect::None,
        ));
    }
    let Some(skill) = required_trimmed_string_arg(args, "skill") else {
        return Ok((
            tool_error(call, "tool.invalid_arguments", "skill is required"),
            AgentToolEffect::None,
        ));
    };
    let Some(script) = required_trimmed_string_arg(args, "script") else {
        return Ok((
            tool_error(call, "tool.invalid_arguments", "script is required"),
            AgentToolEffect::None,
        ));
    };
    let script_args = match args.get("args") {
        None => Value::Object(serde_json::Map::new()),
        Some(value) if value.is_object() => value.clone(),
        Some(_) => {
            return Ok((
                tool_error(call, "tool.invalid_arguments", "args must be an object"),
                AgentToolEffect::None,
            ));
        }
    };

    if !is_valid_script_name(script) {
        return Ok((
            tool_error(
                call,
                SKILL_SCRIPT_INVALID_NAME,
                &format!(
                    "script name `{script}` is invalid: use lowercase letters, digits, and hyphens (pattern ^[a-z0-9][a-z0-9-]*$). Check the exact script name in this skill's SKILL.md."
                ),
            ),
            AgentToolEffect::None,
        ));
    }
    if !skill_is_visible(&profile.skills, skill) {
        return Ok((
            tool_error(
                call,
                SKILL_SCRIPT_SKILL_NOT_VISIBLE,
                &format!(
                    "Skill `{skill}` is not available under the current policy. Call skill_list to see available skills."
                ),
            ),
            AgentToolEffect::None,
        ));
    }
    let Some(scope) = session.effective_skill_scope(skill) else {
        return Ok((
            tool_error(
                call,
                SKILL_SCRIPT_SKILL_NOT_VISIBLE,
                &format!(
                    "Skill `{skill}` is not available in the current Skill set. Call skill_list to see available skills."
                ),
            ),
            AgentToolEffect::None,
        ));
    };

    let relative_path = format!("scripts/{script}.js");
    let script_path = match skill_service
        .skill_file_path(scope, skill, &relative_path)
        .await
    {
        Ok(path) => path,
        Err(ApplicationError::NotFound(_)) | Err(ApplicationError::ValidationError(_)) => {
            return Ok((
                tool_error(
                    call,
                    SKILL_SCRIPT_NOT_FOUND,
                    &format!(
                        "Script `{relative_path}` was not found in skill `{skill}`. Call skill_read on this skill's SKILL.md to check which scripts it ships."
                    ),
                ),
                AgentToolEffect::None,
            ));
        }
        Err(error) => return Err(error),
    };

    let work_dir = workspace_repository
        .run_workspace_root(run_id)
        .await
        .map_err(ApplicationError::from)?;

    let world_info_entries = prompt_snapshot
        .and_then(|snapshot| snapshot.get("worldInfoActivation"))
        .and_then(|batch| batch.get("entries"))
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .enumerate()
                .filter_map(|(index, entry)| ActivatedWorldInfoEntry::from_value(index, entry))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let outcome = engine
        .execute(SkillScriptRequest {
            script_path,
            args: script_args,
            work_dir,
            visible_roots: profile.workspace.visible_roots.clone(),
            writable_roots: profile.workspace.writable_roots.clone(),
            world_info_entries,
        })
        .await;

    let value = match outcome {
        Ok(result) => result.value,
        Err(DomainError::SkillScriptExecutionFailed { message }) => {
            return Ok((
                tool_error(call, SKILL_SCRIPT_EXECUTION_FAILED, &message),
                AgentToolEffect::None,
            ));
        }
        Err(DomainError::SkillScriptResultTooLarge {
            actual_bytes,
            limit_bytes,
        }) => {
            return Ok((
                tool_error(
                    call,
                    SKILL_SCRIPT_RESULT_TOO_LARGE,
                    &format!(
                        "Skill script result is {actual_bytes} bytes, exceeding the {limit_bytes}-byte limit. Return less data from the script and write large output to the workspace with $fs.writeText instead."
                    ),
                ),
                AgentToolEffect::None,
            ));
        }
        Err(error) => return Err(ApplicationError::from(error)),
    };

    let rendered = serde_json::to_string(&value).unwrap_or_else(|_| value.to_string());
    let content = format!(
        "Executed skill script `{skill}/scripts/{script}.js`. Result:\n{rendered}"
    );

    Ok((
        AgentToolResult {
            call_id: call.call_id.clone(),
            tool_id: call.tool_id.clone(),
            content,
            structured: value,
            is_error: false,
            error_code: None,
            resource_refs: Vec::new(),
        },
        AgentToolEffect::None,
    ))
}
```

- [ ] **步骤 5.4：运行测试验证通过**

运行：`cargo test -p tt-application --lib services::agent_tools::skill`
预期：PASS（含既有 skill 测试）

- [ ] **步骤 5.5：descriptor + 常量 + 注册**

`skill/mod.rs` 改为：

```rust
mod descriptors;
mod list;
mod read;
mod script;
mod search;

pub(super) use self::descriptors::{
    skill_list_descriptor, skill_read_descriptor, skill_script_descriptor,
    skill_search_descriptor,
};
pub(super) use self::list::list;
pub(super) use self::read::read;
pub(super) use self::script::script;
pub(super) use self::search::search;

pub(super) const SKILL_LIST: &str = "skill.list";
pub(super) const SKILL_SEARCH: &str = "skill.search";
pub(super) const SKILL_READ: &str = "skill.read";
pub(super) const SKILL_SCRIPT: &str = "skill.script";
```

`skill/descriptors.rs` 追加（`use super::{...}` 中加入 `SKILL_SCRIPT`）：

```rust
pub(in crate::services::agent_tools) fn skill_script_descriptor() -> ToolDescriptor {
    ToolDescriptor {
        id: ToolId::builtin(SKILL_SCRIPT).expect("builtin tool name must be valid"),
        title: Some("Skill Script".to_string()),
        description: Some(
            "Run a JavaScript script shipped by a visible installed Agent Skill in a sandboxed engine. The skill's SKILL.md documents each script's arguments and return value. Scripts run with $fs access limited to this run's workspace and cannot access the network.".to_string(),
        ),
        input_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "skill": {
                    "type": "string",
                    "description": "Visible installed Skill name from skill_list that ships this script."
                },
                "script": {
                    "type": "string",
                    "description": "Script file name under the skill's scripts/ directory, without the .js extension."
                },
                "args": {
                    "type": "object",
                    "description": "Arguments object passed to the script's default/main export.",
                    "additionalProperties": true
                }
            },
            "required": ["skill", "script"]
        }),
        output_schema: None,
        annotations: json!({ "readOnly": false, "sourceKind": "skill" }),
    }
}
```

`registry.rs`：导入改为 `use super::skill::{skill_list_descriptor, skill_read_descriptor, skill_script_descriptor, skill_search_descriptor};`，`all()` 的 descriptors 列表在 `skill_read_descriptor(),` 之后插入一行：

```rust
            skill_script_descriptor(),
```

同文件测试 `registryDeclaresCanonicalBuiltinDescriptors` 中 `assert_eq!(registry.catalog().len(), 19);` 改为 `20`，并在断言后追加：

```rust
        assert!(
            registry
                .catalog()
                .get(&ToolId::builtin("skill.script").unwrap())
                .is_some()
        );
```

- [ ] **步骤 5.6：dispatcher + runtime service**

`dispatcher.rs`：
1. 导入区追加：

```rust
use std::sync::Arc 已有;
use tt_ports::skill_script::SkillScriptEngine;
```

2. 结构体追加字段（在 `skill_service` 之后）：

```rust
    skill_script_engine: Arc<dyn SkillScriptEngine>,
```

3. `new` 追加参数 `skill_script_engine: Arc<dyn SkillScriptEngine>,` 并存入字段。
4. 分发分支（在 `skill::SKILL_READ` 分支之后）：

```rust
            skill::SKILL_SCRIPT => {
                let prompt_snapshot = self.read_run_prompt_snapshot(run_id).await?;
                skill::script(
                    self.skill_service.as_ref(),
                    self.skill_script_engine.as_ref(),
                    self.workspace_repository.as_ref(),
                    run_id,
                    Some(&prompt_snapshot),
                    call,
                    session,
                    profile,
                )
                .await?
            }
```

`agent_runtime_service.rs`：`new` 参数列表末尾追加 `skill_script_engine: Arc<dyn SkillScriptEngine>,`（导入 `use tt_ports::skill_script::SkillScriptEngine;`），并在 `AgentToolDispatcher::new(...)` 调用末尾传入 `skill_script_engine,`。`#[expect(clippy::too_many_arguments)]` 已存在，无需改动。

- [ ] **步骤 5.7：运行全部应用层测试**

运行：`cargo test -p tt-application`
预期：PASS（含 registry 20 断言与所有既有测试）

- [ ] **步骤 5.8：Commit**

```bash
git add src-tauri/crates/tt-application/
git commit -m "feat(skill-script): register skill.script builtin tool with dispatcher wiring"
```

---

### 任务 6：host composition 装配 + 全量验证

**文件：**
- 修改：`src-tauri/crates/tauritavern/Cargo.toml`
- 修改：`src-tauri/crates/tauritavern/src/app/composition/services/mod.rs`
- 修改：`src-tauri/crates/tauritavern/src/app/composition/services/agent.rs`

- [ ] **步骤 6.1：host 依赖**

`tauritavern/Cargo.toml` `[dependencies]` 中按现有 tt-adapter 依赖格式追加：

```toml
tt-adapter-quickjs = { path = "../tt-adapter-quickjs" }
```

- [ ] **步骤 6.2：装配引擎（编译即验证，无独立单测）**

`composition/services/mod.rs`：导入区追加：

```rust
use tt_adapter_quickjs::QuickJsScriptEngine;
use tt_ports::skill_script::SkillScriptEngine;
```

`build` 函数中，在 `let agent_services = agent::build(` 之前构造引擎（D6：公共 libs 目录约定为 `{data_root}/skill-libs/`，`data_root` 即 `data_directory.root()`，与 `_tauritavern/skills` 同级布局）：

```rust
    let skill_script_engine: Arc<dyn SkillScriptEngine> = Arc::new(QuickJsScriptEngine::new(
        data_directory.root().join("skill-libs"),
    ));
    let agent_services = agent::build(
        repositories,
        skill_service,
        chat_completion_service,
        llm_connection_service,
        mcp_service,
        skill_script_engine,
    );
```

（保留 `agent::build` 调用原有参数，仅追加末位参数。）

`composition/services/agent.rs`：`build` 签名末尾追加 `skill_script_engine: Arc<dyn SkillScriptEngine>,`，导入 `use tt_ports::skill_script::SkillScriptEngine;`，并在 `AgentRuntimeService::new(...)` 调用末尾追加 `skill_script_engine,`。

运行：`cargo check -p tauritavern`
预期：通过

- [ ] **步骤 6.3：crate 边界检查**

运行：`node scripts/check-rust-crate-boundaries.mjs`
预期：通过（tauritavern host 允许依赖 adapter；tt-application 未新增 adapter 依赖——它只依赖 tt-ports）

- [ ] **步骤 6.4：全量 harness**

```bash
cargo test -p tt-domain -p tt-ports -p tt-adapter-quickjs -p tt-adapter-storage-userdata -p tt-application
pnpm run check
```
预期：全部通过（`pnpm run check` 为项目强制门槛）

- [ ] **步骤 6.5：Commit**

```bash
git add src-tauri/crates/tauritavern/Cargo.toml src-tauri/crates/tauritavern/src/app/composition/services/mod.rs src-tauri/crates/tauritavern/src/app/composition/services/agent.rs src-tauri/Cargo.lock
git commit -m "feat(skill-script): assemble quickjs engine in host composition"
```

---

## 自检记录

- **规格覆盖度**：设计 §2 D1（scripts/ 范围）→ 任务 2（skill_file_path 强制 `scripts/` 前缀）+ 任务 4（入口仅来自该路径）；D2（模块白名单）→ 任务 3 resolve_module + 任务 4 loader；D3（workdir 动态绑定 + roots 门控）→ 任务 2 run_workspace_root + 任务 3 check_read/check_write + 任务 5 传参；D4（零发现）→ descriptor 描述 + SKILL.md 提示；D5（profile allow 门控）→ 既有 policy 快照机制自动生效（registry 注册即可），handler 内不重复检查；D6（libs 目录）→ 任务 6；D7（30s 超时）→ 任务 4 interrupt handler；D8（零前端）→ 无前端文件改动。§3 契约 → 任务 5.5 schema。§4 流程 → 任务 5.3 逐步对应。§5 port → 任务 2/3。§6 沙箱 → 任务 3/4（删除 tool/ 在 4.1）。§7 装配 → 任务 6。§8 错误表 → 任务 1（变体）+ 任务 4（异常详情/超时措辞）+ 任务 5.3（五个 error_code 与恢复建议措辞，`tool_error` helper）。§9 测试策略 → 各任务 TDD 步骤 + 19→20 断言（5.5）+ 三类门槛（6.3/6.4）。§10 排除项 → 未引入任何前端/发现/管理改动。无遗漏。
- **占位符扫描**：任务 5.1 测试 helper（`base_profile` 等）标注为"照抄 registry.rs 现有字段字面量并指出需改动的两个字段"——这是对既有代码的复制引用而非行为留白；任务 6.2 标注保留原参数仅追加末位——同为对现有调用的明确增量。其余步骤均含完整代码。
- **类型一致性**：`SkillScriptRequest` 字段名在任务 3.1 定义、任务 4.5 测试、任务 5.3 构造三处一致；`skill_file_path(scope, name, relative_path)` 在任务 2.3 port、2.9 service、5.3 调用一致；错误变体名任务 1 定义、4.7/5.1/5.3 使用一致；`run_workspace_root(run_id)` 任务 2.7 port 与 5.3 调用一致；`SKILL_SCRIPT` 常量任务 5.5/5.6 一致。
