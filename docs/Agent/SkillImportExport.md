# TauriTavern Agent Skill Import and Export

本文档记录 TauriTavern 中 Skill 导入导出格式、存储布局，以及 Preset / 角色卡嵌入 Skill 的推荐格式。它是后续实现的设计参考，不表示当前能力已经落地。

当前状态（2026-05-02）：

- Agent runtime 已落地 workspace、journal、checkpoint、commit bridge、canonical model IR 与内建 workspace/chat/worldinfo 工具。
- `skill.list` / `skill.read` 尚未实现。
- `SkillRepository` 尚未实现。
- Preset / 角色卡内嵌 Skill 的导入确认 UI 尚未实现。

相关契约：

- `docs/AgentArchitecture.md`
- `docs/AgentContract.md`
- `docs/Agent/McpSkill.md`
- `docs/Agent/ProfilesAndPreset.md`
- `docs/Agent/Workspace.md`
- `docs/CurrentState/AgentFramework.md`

## 1. 问题定义

TauriTavern 的 Agent Skill 不是“另一段 prompt”，也不是“可执行插件”。它的第一性定义应该是：

> 一个可被 Agent 按需学习和引用的本地知识包。它教会 Agent 如何以可重复的方式完成某类任务，并可携带示例、模板、参考文件和少量宿主可理解的元数据。

它要解决的问题是：

1. 写作、角色扮演、剧情规划、风格控制等领域存在大量可复用方法论。
2. 这些方法论不应该每次都被用户手工复制到 system prompt。
3. 它们也不应该默认全文塞进上下文，造成 token 浪费和 prompt 污染。
4. Skill 可能来自 Preset 作者、角色卡作者、扩展作者、Agent Profile 作者、用户自己和未来 marketplace，必须具备交换格式。
5. Skill 可能只有一个 `SKILL.md`，也可能携带多级目录、示例、资源和模板。
6. 导入 Skill 需要用户明确知情，尤其是来自角色卡、Preset 或扩展的外部内容。

因此，Skill 格式必须同时满足：

- 简洁：普通作者可以手写。
- 灵活：支持目录和多文件。
- 可审计：导入前可以列出名称、描述、文件、大小、来源和风险。
- 可迁移：可随 TauriTavern 数据目录备份/恢复。
- 可兼容：尽量靠近已有 Agent Skill 生态，而不是发明孤岛格式。
- 可治理：不能绕过 Agent Profile、Tool Policy、Visible Resource Policy 或用户 deny。

## 2. 非目标

第一期不解决这些问题：

- 不执行 Skill 自带脚本。
- 不让 Skill 自动安装 MCP server。
- 不让 Skill 直接授予工具权限。
- 不实现远端 URL 自动拉取和自动更新。
- 不设计完整 marketplace、签名、信任链和版本解析。
- 不把 Skill 做成 SillyTavern 兼容的上游格式。
- 不让 Legacy Generate 在 Agent Mode off 时读取或使用 Skill。

这些能力都可以未来扩展，但第一期必须先把格式、存储和导入语义做干净。

## 3. 外部生态对齐

OpenAI Codex Skills、Agent Skills 规范和 Claude Agent Skills 在核心模型上高度一致：

- 一个 Skill 是一个目录。
- 根目录必须有 `SKILL.md`。
- `SKILL.md` 使用 YAML frontmatter 提供 `name` 与 `description` 等索引信息。
- 除 `SKILL.md` 外，可以有额外文件、资源、脚本或参考资料。
- Agent 默认不应把所有 Skill 全文读入上下文，而应先看到索引，再按需读取。

TauriTavern 应采用这个共同子集作为基础，而不是把 Skill 设计成纯 JSON。

推荐原则：

- `SKILL.md` 是跨生态核心。
- TauriTavern 专属元数据放 sidecar 文件，不污染通用 frontmatter。
- 导入导出保持普通目录结构，可以被人类直接查看和编辑。
- `.ttskill` 只是目录包 zip，不是 opaque binary。

## 4. Skill 本体格式

### 4.1 推荐目录结构

最小 Skill：

```text
long-form-romance/
  SKILL.md
```

带参考文件的 Skill：

```text
long-form-romance/
  SKILL.md
  references/
    pacing.md
    emotional-beats.md
  examples/
    short-scene.md
    revision-example.md
  assets/
    outline-template.json
  agents/
    tauritavern.json
```

目录语义：

| 路径 | 语义 |
| --- | --- |
| `SKILL.md` | 必需。Skill 的主说明与索引 frontmatter |
| `references/` | 可选。长参考、规则、写作指南、风格表 |
| `examples/` | 可选。输入输出示例、修改前后对照 |
| `assets/` | 可选。模板、图片、JSON 资源 |
| `scripts/` | 可选但第一期不执行。未来可由工具策略显式控制 |
| `agents/tauritavern.json` | 可选。TauriTavern 专属元数据 |

第一期实现中，`scripts/` 可以被导入和导出，但 runtime 不执行，也不暴露为工具。导入 UI 应显示“包含脚本文件；当前版本仅存储，不执行”。

### 4.2 `SKILL.md` frontmatter

推荐遵循开放 Agent Skills 的核心字段：

```yaml
---
name: long-form-romance
description: Use when writing long-form romantic roleplay scenes that need emotional pacing, continuity, and subtle internal monologue.
license: CC-BY-4.0
metadata:
  author: example-author
  version: "1.0.0"
  tags:
    - writing
    - romance
---
```

必需字段：

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `name` | string | Skill 稳定 ID。建议 ASCII kebab-case |
| `description` | string | 何时使用该 Skill。必须足够具体，供 `skill.list` 暴露 |

可选字段：

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `license` | string | 许可证声明 |
| `metadata.author` | string | 作者 |
| `metadata.version` | string | 作者声明的版本 |
| `metadata.tags` | string[] | 标签 |

不建议放在 `SKILL.md` 顶层的字段：

| 字段 | 原因 |
| --- | --- |
| `allowed-tools` | 容易被误解为授权。TauriTavern 中工具权限必须由 profile/plan/user/platform policy resolve |
| `mcpServers` | Skill/Preset/角色卡不得直接写 MCP stdio command |
| `autoRun` | Skill 不应自动执行行为 |
| `systemPromptPriority` | ContextFrame 和 profile policy 才决定 prompt 位置与预算 |

### 4.3 TauriTavern sidecar

TauriTavern 专属信息放在：

```text
agents/tauritavern.json
```

建议 schema：

```json
{
  "version": 1,
  "displayName": "Long-form Romance",
  "sourceKind": "user",
  "allowImplicitInvocation": true,
  "recommendedTools": [
    "skill.read",
    "workspace.read_file",
    "workspace.apply_patch"
  ],
  "recommendedContext": {
    "defaultBudgetTokens": 2000,
    "preferredMode": "lazy"
  },
  "tags": ["writing", "romance"],
  "notes": "recommendedTools is advisory; runtime policy still decides."
}
```

字段语义：

| 字段 | 类型 | 语义 |
| --- | --- | --- |
| `version` | number | sidecar schema 版本。第一版为 `1` |
| `displayName` | string | UI 展示名；不参与唯一性 |
| `sourceKind` | string | `user` / `preset` / `character` / `extension` / `marketplace` / `unknown` |
| `allowImplicitInvocation` | boolean | 是否允许 profile/router 在匹配场景时自动显示给 Agent |
| `recommendedTools` | string[] | 作者建议工具；不授予权限 |
| `recommendedContext.defaultBudgetTokens` | number | 建议读取预算 |
| `recommendedContext.preferredMode` | string | `lazy` / `index_only` / `auto_include`；最终仍由 policy 裁决 |
| `tags` | string[] | UI 标签 |
| `notes` | string | 给用户或开发者看的说明 |

`agents/tauritavern.json` 无效时，导入应 fail-fast，而不是静默忽略。原因是该文件一旦存在，就代表作者希望表达 TauriTavern 专属语义；静默忽略会让用户误以为策略生效。

## 5. Skill ID 与命名

Skill 的稳定身份以 `SKILL.md` frontmatter 的 `name` 为准。

建议约束：

- 非空。
- 长度不超过 128。
- 建议使用 ASCII 小写、数字、`-`、`_`。
- 第一版可以对非 ASCII name 直接拒绝，或允许但落盘目录使用 hash。为了简单和可交换，推荐第一版要求 ASCII。
- 不允许 `/`、`\`、`.`、`..`、NUL、Windows drive prefix。

物理目录名建议为：

```text
<normalized-name>/
```

如果未来允许非 ASCII 或更宽松 name，可以改为：

```text
<slug>--<hash8>/
```

但第一期不建议上来就做复杂映射。Skill 引用需要稳定，自动改名会增加维护成本。

## 6. 独立导入导出格式

### 6.1 支持的输入

第一期建议支持：

```text
my-skill/
  SKILL.md
```

以及：

```text
my-skill.ttskill
```

`.ttskill` 本质是 zip。它不是新的二进制协议。

zip 内部允许两种 layout：

```text
SKILL.md
references/...
assets/...
```

或：

```text
my-skill/
  SKILL.md
  references/...
  assets/...
```

扫描规则：

1. 解压前先扫描 entry。
2. 忽略 `__MACOSX` 与资源 fork。
3. 找到所有候选 `SKILL.md`。
4. 若没有候选，失败。
5. 若多个同级候选导致歧义，失败。
6. 选择唯一 skill root 后，其他识别到的独立 skill root 不自动导入，除非未来支持 multi-skill bundle。

第一期不建议支持一个 `.ttskill` 包含多个 skill。多 Skill bundle 会立即引入批量冲突、部分成功、回滚和 UI 复杂度，ROI 不高。

### 6.2 导入校验

导入必须在 staging 中完成：

```text
data_root/_tauritavern/skills/.staging/<import-id>/
```

校验项：

- `SKILL.md` 必须存在。
- frontmatter 必须能解析。
- `name` 与 `description` 必须存在且类型正确。
- `name` 必须通过 Skill ID 校验。
- 所有 entry path 必须是相对路径。
- 禁止 `..`、绝对路径、Windows drive prefix、NUL。
- 禁止 symlink。
- 限制 entry 数量。
- 限制单文件大小。
- 限制总解压大小。
- 限制压缩比，防 zip bomb。
- 可选拒绝隐藏文件，或至少拒绝 `.git/`、`.ssh/`、`.env`。

推荐第一期限制：

```text
max files: 1000
max single file: 16 MiB
max total uncompressed: 256 MiB
max SKILL.md: 1 MiB
```

这些值比完整数据归档小得多，因为单个 Skill 不应承担数据备份职责。

### 6.3 冲突策略

以 `name` 判断冲突。

推荐策略：

| 情况 | 行为 |
| --- | --- |
| 同名不存在 | 安装 |
| 同名存在且内容 hash 相同 | 视为已安装，可提示无需导入 |
| 同名存在且内容 hash 不同 | 弹窗让用户选择 `跳过` / `替换` |

不建议自动改名为 `name-2`。原因：

- `name` 是 Agent 可见引用。
- Preset / 角色卡可能通过 `name` 推荐 Skill。
- 自动改名会破坏作者意图，且难以解释为什么 Agent 找不到原名。

第一期不做多版本并存。多版本并存需要 resolution policy、profile pin、UI 选择和清理策略，复杂度明显高于收益。

### 6.4 原子写入

安装流程：

```text
scan archive
  -> extract to staging
  -> validate staging
  -> compute manifest/hash
  -> conflict decision
  -> rename existing to backup? 或直接替换
  -> atomic rename staging to installed/<name>
  -> write index
```

若目标文件系统不支持跨目录 rename，应复用现有 temp+rename/copy fallback 基础设施，而不是写第二套复制逻辑。

失败必须清理 staging，并返回明确错误。

### 6.5 导出格式

导出单个 Skill：

```text
long-form-romance.ttskill
```

内部推荐：

```text
SKILL.md
references/
assets/
agents/tauritavern.json
```

不要把外层目录名作为语义。接收方应以 `SKILL.md` 的 `name` 为准。

导出时可以附带一个非必需清单：

```text
agents/tauritavern-export.json
```

示例：

```json
{
  "version": 1,
  "exportedAt": "2026-05-02T00:00:00Z",
  "exportedBy": "TauriTavern",
  "format": "ttskill",
  "skillName": "long-form-romance"
}
```

该文件只用于诊断，不参与运行时语义。

## 7. 本地存储布局

推荐存储在 data root 下：

```text
data_root/
  _tauritavern/
    skills/
      installed/
        long-form-romance/
          SKILL.md
          references/
          assets/
          agents/
            tauritavern.json
      index/
        skills.json
      .staging/
```

理由：

- `_tauritavern` 是 TauriTavern 自有运行和扩展数据空间。
- 不污染 SillyTavern 的 `default-user` 布局。
- 现有数据归档导出整个 `data_root`，会自然携带 `_tauritavern/skills`。
- 现有导入 layout 已把 `_tauritavern` 识别为 data-root 特征。
- 与 `_tauritavern/agent-workspaces`、`extension-store`、`prompt-cache` 的职责一致。

不建议放在：

| 位置 | 问题 |
| --- | --- |
| `default-user/skills` | 容易误认为上游 SillyTavern 用户数据；未来上游同步可能冲突 |
| `default-user/extensions` | Skill 不是前端扩展 |
| `agent-workspaces/chats/.../resources/skills` | 这是对话级资源，不适合作为全局安装库 |
| app bundle 内 | 用户导入内容不应写应用资源目录 |

## 8. Repository 与 Domain 边界

建议新增 domain trait：

```rust
#[async_trait]
pub trait SkillRepository: Send + Sync {
    async fn list_skills(&self) -> Result<Vec<SkillIndexEntry>, DomainError>;
    async fn get_skill(&self, name: &str) -> Result<SkillPackage, DomainError>;
    async fn read_skill_file(&self, name: &str, path: &str) -> Result<SkillFileContent, DomainError>;
    async fn preview_import(&self, input: SkillImportInput) -> Result<SkillImportPreview, DomainError>;
    async fn install_import(&self, decision: SkillImportDecision) -> Result<SkillInstallResult, DomainError>;
    async fn export_skill(&self, name: &str, output: SkillExportTarget) -> Result<SkillExportResult, DomainError>;
}
```

Domain model 只表达逻辑语义：

```rust
SkillIndexEntry {
    name,
    description,
    display_name,
    source_kind,
    tags,
    installed_hash,
}

SkillPackage {
    name,
    description,
    root_ref,
    files,
    tauritavern_metadata,
}

SkillFileRef {
    path,
    kind,
    media_type,
    size_bytes,
    hash,
}
```

Infrastructure 实现文件系统、zip、staging 和原子替换。

Presentation command 只做：

- DTO 反序列化。
- 获取 service。
- 调用 service。
- 错误映射。

不要把 zip 解压或目录扫描写进 Tauri command。

## 9. Agent Runtime 消费方式

Skill 进入 Agent 的路径应保持与 `docs/Agent/McpSkill.md` 一致：

```text
skill.list / profile visible skill policy
  -> SkillService
  -> PromptComponent::SkillIndex 或 WorkspaceResource
  -> ContextFrame
  -> AgentModelRequest
```

读取全文：

```text
model calls skill.read
  -> ToolRegistry policy check
  -> SkillService.read
  -> journal: tool_call_requested / tool_call_completed
  -> ToolResult with content or resource refs
```

重要约束：

- Agent 默认只看到 Skill 索引，不看到所有全文。
- `skill.read` 必须写 journal。
- `skill.read` 受 budget 限制。
- `skill.read` 只能读 visible skill。
- Skill 文件作为 read-only virtual resource 暴露。
- Agent 不能修改已安装 Skill。
- 如果需要摘录、摘要、改写，必须写 `scratch/`、`summaries/` 或 `output/`。

模型可见路径示例：

```text
skills/long-form-romance/SKILL.md
skills/long-form-romance/references/pacing.md
```

这些是逻辑路径，不是系统路径。读取仍经 SkillRepository / WorkspaceResource 层，不允许工具直接拼文件系统路径。

## 10. Preset 嵌入格式

上游 SillyTavern preset 是宽松 JSON。TauriTavern 当前 Rust 后端也把 preset `data` 作为 `serde_json::Value` 保存，并按上游目录映射落盘。因此，Preset 内嵌 Skill 应放在扩展字段，而不是改变 preset 主体 schema。

推荐路径：

```text
preset.extensions.tauritavern.skills
```

示例：

```json
{
  "name": "Narrative Writer",
  "temperature": 0.8,
  "extensions": {
    "regex_scripts": [],
    "tauritavern": {
      "skills": {
        "version": 1,
        "items": [
          {
            "bundleFormat": "inline-files-v1",
            "source": {
              "kind": "preset",
              "label": "Narrative Writer"
            },
            "files": [
              {
                "path": "SKILL.md",
                "encoding": "utf8",
                "content": "---\nname: long-form-romance\ndescription: Use when writing long romantic scenes with emotional continuity.\n---\n\n# Long-form Romance\n\n..."
              },
              {
                "path": "references/pacing.md",
                "encoding": "utf8",
                "content": "# Pacing\n\n..."
              }
            ]
          }
        ],
        "recommended": [
          {
            "name": "long-form-romance",
            "autoVisible": true
          }
        ]
      }
    }
  }
}
```

字段说明：

| 字段 | 说明 |
| --- | --- |
| `version` | 嵌入 schema 版本。第一版为 `1` |
| `items` | 内嵌 Skill 包列表 |
| `items[].bundleFormat` | 第一版只支持 `inline-files-v1` |
| `items[].source` | 供导入 UI 展示来源 |
| `items[].files` | 文件列表 |
| `recommended` | Preset 对已安装 Skill 的推荐引用，不代表已安装 |

`inline-files-v1` 文件字段：

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `path` | string | Skill 包内相对路径 |
| `encoding` | string | `utf8` 或 `base64` |
| `content` | string | 文件内容 |
| `mediaType` | string | 可选，主要用于二进制资源 |
| `sizeBytes` | number | 可选，用于预览和校验 |
| `sha256` | string | 可选，用于完整性提示 |

为什么不用 base64 zip：

- JSON 中 inline files 更容易审计和 diff。
- 用户导入前可以逐文件预览。
- 前端和后端都能直接做路径级校验。
- 写作类 Skill 主要是文本，base64 zip 会降低可维护性。

什么时候可以用 base64：

- 单个二进制资源，例如图片。
- 未来为了兼容外部 `.ttskill` 直接嵌入时，可新增 `bundleFormat: "base64-zip-v1"`，但不作为第一期默认。

## 11. 角色卡嵌入格式

角色卡应优先使用 Tavern Card V2 的 `data.extensions`。

推荐路径：

```text
character.data.extensions.tauritavern.skills
```

示例：

```json
{
  "spec": "chara_card_v2",
  "spec_version": "2.0",
  "data": {
    "name": "Example Character",
    "description": "...",
    "extensions": {
      "regex_scripts": [],
      "tauritavern": {
        "skills": {
          "version": 1,
          "items": [
            {
              "bundleFormat": "inline-files-v1",
              "source": {
                "kind": "character",
                "label": "Example Character"
              },
              "files": [
                {
                  "path": "SKILL.md",
                  "encoding": "utf8",
                  "content": "---\nname: example-character-voice\ndescription: Use when preserving Example Character's speech cadence and recurring motifs.\n---\n\n# Character Voice\n\n..."
                }
              ]
            }
          ],
          "recommended": [
            {
              "name": "example-character-voice",
              "autoVisible": true
            }
          ]
        }
      }
    }
  }
}
```

原因：

- 上游角色卡生态已经把 `data.extensions` 作为扩展兼容面。
- 当前 TauriTavern Rust `CharacterExtensions` 使用 flatten 保留未知扩展字段。
- 导入 normalize 过程中 top-level unknown 字段更容易丢失。
- `regex_scripts` 也在 `data.extensions` 下，这是已有兼容模式。

PNG 写入仍遵守上游行为：

- `tEXt` chunk keyword `chara` 保存 base64 UTF-8 JSON。
- 可同时写 `ccv3`。
- 读取优先 `ccv3`，再 fallback `chara`。

Skill 嵌入不应该放在：

| 路径 | 问题 |
| --- | --- |
| top-level `skills` | 可能被导入转换丢弃，也不是 Tavern Card 兼容面 |
| `data.skills` | 不属于上游约定，且 Rust model 不保证保留 |
| `data.character_book` | 世界书语义，不应用来塞 Skill |
| `creator_notes` | 会污染角色作者说明，也无法携带多文件 |

## 12. 嵌入 Skill 与推荐 Skill 的区别

必须区分两种概念：

```text
embedded skill
  包含完整文件内容，可以导入安装。

recommended skill
  只引用 name，表示作者建议使用；不包含内容。
```

Preset / 角色卡可以同时包含：

```json
{
  "items": [
    { "bundleFormat": "inline-files-v1", "files": [] }
  ],
  "recommended": [
    { "name": "long-form-romance", "autoVisible": true }
  ]
}
```

导入行为：

- `items` 触发“是否导入 Skill”弹窗。
- `recommended` 不安装任何东西，只在已安装时可作为 profile/context policy 的候选。
- 未安装的 recommended skill 可以在 UI 中显示为“推荐但未安装”。

这样可以避免角色卡作者用一个轻量引用强迫用户安装未知内容。

## 13. Preset / 角色卡导入确认流程

目标体验类似 embedded regex：

> 用户导入 Preset / 角色卡后，若发现内嵌 Skill，弹窗询问是否导入。

但 Skill 与 regex 的差异是：

- regex 是当前对象内的脚本启用许可。
- Skill 是安装到 TauriTavern 全局 Skill 库的资源包。
- Skill 可能包含多文件、二进制资源、脚本文件和潜在大内容。

推荐流程：

```text
Preset / character imported
  -> scan extensions.tauritavern.skills
  -> build import preview
  -> show popup
  -> user selects items and conflict decisions
  -> backend validates staging
  -> install selected skills
  -> show result
```

弹窗应展示：

- 来源：Preset 名称 / 角色名 / 文件名。
- Skill `name`。
- `description`。
- 文件数。
- 总大小。
- 是否包含 `scripts/`。
- 是否包含二进制资源。
- 冲突状态：新安装 / 已安装相同 / 同名不同。
- 目标安装位置。

弹窗按钮建议：

- `导入选中 Skill`
- `暂不导入`
- `查看详情`

冲突项需要单独选择：

- `跳过`
- `替换已安装版本`

不建议默认“全部替换”。

### 13.1 一次性提醒 key

可以借鉴 regex：

```text
AlertSkillPreset_<apiId>_<presetName>_<embeddedHash>
AlertSkillCharacter_<avatar>_<embeddedHash>
```

加入 `<embeddedHash>` 是为了避免同一个 Preset / 角色卡更新了内嵌 Skill 后，用户永远不再看到提醒。

如果用户拒绝导入：

- 不删除嵌入数据。
- 记录提醒 key。
- 未来用户可在 Skill 管理界面重新查看“来自当前 Preset/角色卡的可导入 Skill”。

### 13.2 事件语义

不要伪装成 SillyTavern 上游事件。

可以在 TauriTavern 前端 bridge 内监听现有事件：

- `PRESET_CHANGED`
- `CHAT_CHANGED`
- Preset import 成功路径
- Character import 成功路径

然后启动 TauriTavern 自有 Skill preview/import 流程。

Agent run/timeline 事件仍属于 `api.agent`，Skill 安装不是 Agent run event，除非它发生在某次 Agent run 内的受控工具调用中。第一期建议 Skill 安装只由用户 UI 触发，不由模型触发。

## 14. Profile / Preset / Character 的 Skill Policy

Preset 和角色卡可以声明：

```json
{
  "recommended": [
    {
      "name": "long-form-romance",
      "autoVisible": true,
      "priority": 50
    }
  ]
}
```

但这不等于：

- 自动导入。
- 自动全文进入 prompt。
- 自动授予工具。
- 自动覆盖用户设置。

最终 runtime resolution 应遵守：

```text
Built-in defaults
  < Preset agent schema
  < Character agent schema
  < User profile override
  < Per-run override
```

以及：

- user deny 最高。
- deny 优先 allow。
- 未显式 visible 的 skill 默认不可见。
- required skill 缺失时，如果 profile 标记为 required，应 fail-fast；如果只是 recommended，应记录 warning 或 UI 提示。

建议在 Agent Profile 中表达：

```json
{
  "skills": {
    "visible": ["long-form-romance", "example-character-voice"],
    "autoInclude": [],
    "deny": ["unsafe-skill"],
    "readBudgetTokens": 3000
  }
}
```

第一期可以只支持：

- `visible`
- `deny`
- `readBudgetTokens`

不要过早实现复杂 routing。

## 15. Security 与 Fail-fast

Skill 是本地知识包，但来源可能不可信。导入时应按“安装外部内容”处理。

必须 fail-fast：

- `SKILL.md` 缺失。
- frontmatter 无法解析。
- `name` / `description` 缺失。
- path traversal。
- symlink。
- zip entry 超限。
- 解压后发现文件逃逸 staging。
- `agents/tauritavern.json` 存在但 schema 无效。
- 同名冲突但没有用户 decision。
- 原子安装失败。

可作为 warning：

- 包含 `scripts/`，但当前不执行。
- 包含未知目录。
- 缺少 license。
- recommended skill 未安装。
- sidecar 中存在当前版本不支持的字段。

禁止：

- Skill 自动执行脚本。
- Skill 自动注册 MCP command。
- Skill 自动扩大工具权限。
- Skill 自动读取用户 secrets。
- Skill 被模型修改后直接覆盖安装库。
- 导入失败时“跳过坏文件继续安装”。

## 16. 性能与索引

`skill.list` 应非常轻量。不要每次运行扫描和读取所有文件全文。

建议维护：

```text
data_root/_tauritavern/skills/index/skills.json
```

示例：

```json
{
  "version": 1,
  "updatedAt": "2026-05-02T00:00:00Z",
  "skills": [
    {
      "name": "long-form-romance",
      "description": "Use when writing long romantic scenes with emotional continuity.",
      "displayName": "Long-form Romance",
      "root": "installed/long-form-romance",
      "hash": "sha256:...",
      "sourceKind": "user",
      "tags": ["writing", "romance"],
      "fileCount": 4,
      "totalSizeBytes": 12000
    }
  ]
}
```

索引损坏时建议 fail-fast 于 Skill 管理操作，但 Agent run 可以返回明确错误“Skill index invalid”。不要静默重建后继续，因为重建可能掩盖磁盘损坏或部分安装失败。

可以提供用户显式“重建 Skill 索引”操作。

## 17. 测试策略

Domain tests：

- `SKILL.md` frontmatter 解析。
- name 校验。
- sidecar schema 解析。
- embedded schema 解析。
- recommended vs embedded 区分。

Infrastructure tests：

- `.ttskill` root layout。
- `.ttskill` single-folder layout。
- 无 `SKILL.md` 失败。
- 多 `SKILL.md` 歧义失败。
- path traversal 失败。
- symlink 失败。
- zip bomb/entry limit 失败。
- 同名同 hash。
- 同名不同 hash 要求 decision。
- staging cleanup。
- atomic replace。

Application tests：

- preview import 不写 installed。
- install decision 写 installed。
- `skill.list` 只返回索引。
- `skill.read` 受 visible policy 限制。
- `skill.read` 受 budget 限制。
- tool call 写 journal。

Frontend tests：

- Preset import 后发现 embedded skill。
- Character import 后发现 embedded skill。
- 用户拒绝后记录 alert key。
- embedded hash 改变后再次提醒。
- 冲突替换需要明确选择。
- 包含 scripts 时 UI 显示风险提示。

Regression tests：

- Agent Mode off 不改变 Legacy Generate。
- Preset 保存仍保持上游字段。
- 角色卡导入导出保留 `data.extensions.tauritavern`。
- regex embedded prompt 语义不变。
- world/lorebook embedded prompt 语义不变。

## 18. 推荐实施顺序

### 18.1 格式与 parser

先实现纯解析：

- `SkillManifest`。
- `TauriTavernSkillMetadata`。
- `EmbeddedSkillBundle`。
- `SkillImportPreview`。

不接 UI，不接 Agent。

### 18.2 FileSkillRepository

新增：

```text
src-tauri/src/domain/repositories/skill_repository.rs
src-tauri/src/infrastructure/repositories/file_skill_repository/
```

实现：

- list installed。
- preview `.ttskill`。
- install from staging。
- export `.ttskill`。

### 18.3 Skill 管理 API

新增 Tauri commands 或 Host ABI：

```ts
api.skill.previewImport(...)
api.skill.installImport(...)
api.skill.list()
api.skill.export(...)
```

是否挂在 `window.__TAURITAVERN__.api.skill`，还是 `api.agent.skill`，需要结合 Host ABI 风格决定。倾向 `api.skill`，因为 Skill 管理不是只属于 Agent run；Agent 只是消费者之一。

### 18.4 Preset / Character embedded scan

在前端导入成功路径扫描：

- `preset.extensions.tauritavern.skills`
- `character.data.extensions.tauritavern.skills`

调用 preview/import UI。

### 18.5 Agent tool 接入

实现：

- `skill.list`
- `skill.read`

并把结果纳入 journal 和 ContextFrame。

### 18.6 Profile policy

加入最小 policy：

- visible skills。
- denied skills。
- read budget。

### 18.7 后续增强

- marketplace。
- 签名。
- 多版本并存。
- Skill update。
- base64 zip embedded bundle。
- Skill dependencies。

这些都不应阻塞第一期。

## 19. 主要取舍

### 19.1 为什么采用目录包

优点：

- 对齐 OpenAI Codex Skills、Agent Skills 和 Claude Skills。
- 普通作者可以手写。
- 多文件自然表达。
- 易审计、易 diff、易导入导出。

缺点：

- 需要 zip/staging/path 校验。
- 需要处理目录冲突和索引。

判断：收益明显大于复杂度，因为 Skill 天然不是单 JSON 对象。

### 19.2 为什么不用纯 JSON

纯 JSON 的优点是便于嵌入 Preset/角色卡。

但缺点更大：

- Markdown 指令会被 JSON escaping 污染。
- 多文件和二进制资源表达别扭。
- 难以与外部 Agent Skill 生态互通。
- 作者手写和版本管理体验差。

因此，独立格式应以目录为准；嵌入格式只是把目录文件列表内联进 JSON。

### 19.3 为什么不自动改名解决冲突

自动改名看似友好，但会破坏 Skill 的核心身份。

如果角色卡推荐 `example-character-voice`，用户导入后变成 `example-character-voice-2`，Profile 和 Agent 都难以解释该如何匹配。

所以第一期只允许：

- 跳过。
- 替换。
- 已安装相同内容则忽略。

### 19.4 为什么 Skill 权限只是建议

Skill 来自外部作者。若它能声明 `allowed-tools` 并直接生效，就等于角色卡/Preset 能扩大运行时能力，违反 AgentContract。

正确模型是：

```text
Skill recommendedTools
  -> profile resolver input
  -> user/platform/tool policy
  -> final visible tools
```

## 20. 参考链接

- OpenAI Codex Skills: `https://developers.openai.com/codex/skills`
- OpenAI Codex Customization: `https://developers.openai.com/codex/concepts/customization`
- Agent Skills Specification: `https://agentskills.io/specification`
- Agent Skills Client Implementation: `https://agentskills.io/client-implementation/adding-skills-support`
- Claude Agent Skills Overview: `https://platform.claude.com/docs/en/agents-and-tools/agent-skills/overview`
