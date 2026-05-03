# TauriTavern Agent Skill

本文档记录当前已落地的 Agent Skill 能力。长期开发以本文、`docs/CurrentState/AgentFramework.md`、`docs/Agent/ToolSystem.md` 与 `docs/API/Skill.md` 为准。

## 定义

Skill 是 Agent 按需读取的本地知识包，不是可执行插件，也不是默认注入 prompt 的大段文本。

一个 Skill 是一个目录：

```text
my-skill/
  SKILL.md
  references/
  examples/
  assets/
  scripts/
  agents/tauritavern.json
```

当前要求：

- `SKILL.md` 必须存在，并以 YAML frontmatter 开头。
- frontmatter 必须包含 `name` 与 `description`。
- `name` 使用小写 ASCII、数字、`-`、`_`，最长 128。
- `agents/tauritavern.json` 可选；一旦存在，schema 无效就 fail-fast。
- `scripts/` 会随 Skill 导入、导出，并在预览中提示风险；当前不会执行。

## 当前实现

已落地：

- Domain：`SkillIndexEntry`、`SkillImportInput`、`SkillImportPreview`、`SkillInstallRequest`、`SkillReadResult` 等模型。
- Repository：`SkillRepository` trait 与文件实现 `FileSkillRepository`。
- Service：`SkillService`。
- Host ABI：`window.__TAURITAVERN__.api.skill`。
- Agent tools：`skill.list` / `skill.read`，模型侧 alias 为 `skill_list` / `skill_read`。
- Preset / Character embedded skill 扫描与导入确认 UI。
- Preset / Character 删除时，删除仅由该来源引用的已安装 Skill。

本地存储：

```text
data_root/_tauritavern/skills/
  installed/<skill-name>/
  index/skills.json
  .staging/
```

`FileSkillRepository` 只负责文件系统、zip、staging、索引与原子安装；运行时可见性、profile policy、journal 由 Agent tool/runtime 层负责。

## 导入导出

支持输入：

- `inlineFiles`：Preset / 角色卡嵌入 Skill 的文件列表。
- `directory`：本地 Skill 目录。
- `archiveFile`：`.ttskill` zip 包。

导入流程：

```text
materialize input into .staging
  -> validate package
  -> compute files/hash/warnings
  -> compare installed index
  -> install / replace / skip
  -> update index
```

冲突策略：

- 同名不存在：安装。
- 同名且 hash 相同：视为已安装，并合并 source refs。
- 同名但 hash 不同：必须显式 `skip` 或 `replace`。

导出：

- `api.skill.export({ name })` 返回 `{ fileName, contentBase64, sha256 }`。
- `.ttskill` 内只包含 Skill 文件本身，不写入会改变内容 hash 的诊断 sidecar。

## Agent 读取

`skill.list`：

- 只读。
- 返回已安装 Skill 的索引摘要。
- 当前尚未接入 profile visible/deny policy，因此列出全局已安装 Skill。

`skill.read`：

- 只读。
- 参数：`name`、可选 `path`、可选 `max_chars`。
- `path` 默认 `SKILL.md`。
- 只能读取 UTF-8 文本文件；二进制文件返回可恢复 tool error。
- `max_chars` 默认 20000，最大 80000。
- 结果写入 Agent journal / tool result，并作为后续模型上下文的一部分回填。

Skill 文件对 Agent 是只读 virtual resource。Agent 不能修改 installed Skill；需要摘录、总结或改写时写入 workspace 的 `scratch/`、`summaries/` 或 `output/`。

## 安全边界

导入、安装、导出与 repository 层必须 fail-fast：

- `SKILL.md` 缺失、frontmatter 无效、`name` / `description` 缺失。
- path traversal、绝对路径、Windows drive prefix、NUL、symlink escape。
- zip entry 超限、压缩比超限、总大小超限。
- `agents/tauritavern.json` schema 无效。
- 同名冲突但没有用户决策。
- index 缺失但 installed 目录已存在。

Agent tool 层的模型可修正读取错误，例如缺失文件、二进制文件、非法 path 或超出 read budget，应返回 recoverable tool error；repository 内部 IO、index 损坏和安装一致性错误仍 fail-fast。

当前限制：

- 不执行 Skill 自带脚本。
- 不让 Skill 自动安装 MCP server。
- 不让 Skill 授予工具权限。
- 不支持 marketplace、自动更新、多版本并存或依赖解析。
- 不支持模型在 run 内安装/替换 Skill。

## 后续开发

下一步只做必要能力：

- profile/preset/character 的最小 visible / deny / read budget policy。
- `skill.list` 按 visible policy 收窄结果。
- `skill.read` 按 policy 与 budget 拒绝不可见或过量读取。
- 明确 recommended skill 与 embedded skill 在 profile resolver 中的合流规则。

不要把 Skill 扩展成脚本执行、权限授予或 MCP 配置入口；这些能力必须走独立的 tool/policy/approval 体系。
