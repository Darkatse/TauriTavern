# Skill

Skill 是按需读取的本地知识包。`SKILL.md` 说明它适合什么任务、如何使用，其他文件保存参考材料、示例和脚本。

## 创建一个 Skill

```text
scene-review/
  SKILL.md
  references/checklist.md
  scripts/list-scenes.js
```

最小的 `SKILL.md`：

```markdown
---
name: scene-review
description: 检查场景中的人物动机与叙事衔接。
---

阅读 references/checklist.md，按其中的问题检查草稿。
将具体修改建议写入 summaries/review.md，并引用对应段落。
```

包名使用小写字母、数字、`-` 或 `_`。其他目录按内容需要添加；有脚本时，在 `SKILL.md` 中说明用途、参数和返回值。

## 安装与作用域

Skill Manager 可从目录或 ZIP 递归发现 Skill，逐项预览和安装；支持一次选择多个来源。角色卡和预设也可以携带 Skill。导出得到包含原始文件的 ZIP。

Skill 可以属于全局、预设、Profile 或角色。运行时按 `global → preset → profile → character` 解析，同名 Skill 由靠后的作用域覆盖，再按 Profile 的 `skills.visible` 和 `deny` 筛选。

子 Agent 使用目标 Profile 的 Skill 配置，环境信息来自 Run 冻结输入。同名 Skill 可在不同 Invocation 中绑定到不同作用域。绑定在准备时确定，后续轮次与恢复沿用；读取使用安装包当前内容，缺失时失败，不改选其他同名包。

## 在运行中使用

有效 Skill 的名称、描述和 `SKILL.md` 路径随系统提示词提供，见 [Prompt assembly](PromptAssembly.md)。模型通过工作区文件工具读取 `skills/<name>/SKILL.md` 及支持材料；相对引用以 Skill 目录为起点，Shell 中 `/skills/<name>/` 指向同一视图。

`skills/` 只读。普通 UTF-8 文本展开 Run 冻结宏，`scripts/` 下的源码和非 UTF-8 文件保持原样；读取、文件大小与复制使用同一内容视图，文本搜索报告跳过数量。需要编辑或发布的材料先复制到普通工作目录，见 [Workspace](Workspace.md)。

安装包由 `api.skill` 管理，管理读取与编辑使用原始内容，见 [Skill API](../API/Skill.md)。

## 运行脚本

通过 `workspace.shell` 执行包内脚本。例如 `scripts/list-scenes.js`：

```js
import { workspace } from '@tauritavern/runtime';

export default function ({ path }) {
  const text = workspace.readText(path);
  const scenes = text.split('\n').filter(line => line.startsWith('## '));
  workspace.writeText('summaries/scenes.md', scenes.join('\n'));
  return { count: scenes.length, path: 'summaries/scenes.md' };
}
```

Shell 命令：

```sh
js --call default --args-json '{"path":"output/main.md"}' /skills/scene-review/scripts/list-scenes.js
```

导出 `main` 时使用 `--call main`。脚本遵循统一的工作区与 Shell 语义，见 [JavaScript](Workspace.md#javascript)；命令选项见 `js --help`。

### 可用能力

脚本在 QuickJS 中执行，支持 ES 模块和 `async` / `await`，不提供 Node/Deno 标准库、DOM、网络、子进程或定时器。宿主能力从统一模块导入：

```js
import { workspace, context, macros, log } from '@tauritavern/runtime';
```

| 接口 | 用途 |
| --- | --- |
| `workspace.readText(path)` | 读取 UTF-8 文本 |
| `workspace.writeText(path, text)` | 创建或替换文件，自动创建父目录 |
| `workspace.listFiles(path?)` | 无参数时列出工作区根目录；指定目录时递归列出文件，返回相对于该目录的路径 |
| `workspace.exists(path)` | 检查可访问路径是否存在 |
| `context.worldInfo.entries` | Run 启动时激活的世界书 |
| `context.variables.local` / `global` | 保留原始 JSON 类型的 SillyTavern 变量 |
| `context.macro` | 冻结的名称、角色和聊天位置等宏数据 |
| `macros.render(text)` | 使用 Run 冻结值展开宏 |
| `log.info(text)` / `warn` / `error` / `debug` | 将日志写入 stderr |

文件 API 的路径相对于工作区根。`context` 是冻结输入的副本，修改它不会写回宿主；缺少所需上下文时，访问对应字段会报错。

普通执行时，`console.log/info/debug` 输出到 stdout，`console.warn/error` 输出到 stderr。使用 `--call` 时，返回值以 JSON 写入 stdout，所有日志写入 stderr，便于后续命令处理结果。

### 模块与工具箱

相对 import 从导入模块所在目录解析，路径须写明 `.js` 或 `.mjs` 扩展名。以下库随应用内置，通过完整模块名导入：

| 模块 | 用途 |
| --- | --- |
| `@tauritavern/kit/dayjs` | 日期处理 |
| `@tauritavern/kit/es-toolkit` | 数组、对象和字符串处理 |
| `@tauritavern/kit/fast-xml-parser` | XML 解析、校验与构建 |
| `@tauritavern/kit/marked` | Markdown 转 HTML |
| `@tauritavern/kit/papaparse` | CSV 解析与生成 |
| `@tauritavern/kit/slugify` | 生成适合路径或标识符的文本 |

这些库保留各自 API，仍受上述运行环境限制。其他依赖可预先打包成兼容的 ESM 放入 `scripts/vendor/`，通过相对路径导入；运行时不安装 npm 包。内置模块清单见 [kit.rs](../../src-tauri/crates/tt-adapter-workspace-shell/src/kit.rs)。

## 源码

Skill 保存在 `_tauritavern/skills/`，按作用域组织安装目录，索引位于 `index/skills.json`。

- [SkillService](../../src-tauri/crates/tt-application/src/services/skill_service.rs)：管理与有效 Skill 解析。
- [file_skill_repository](../../src-tauri/crates/tt-adapter-storage-userdata/src/repositories/file_skill_repository)：包格式、安装、索引与原始文件读写。
- [skill_scope.rs](../../src-tauri/crates/tt-application/src/services/agent_runtime_service/skill_scope.rs)：Invocation 的作用域。
- [Skill 文件视图](../../src-tauri/crates/tt-application/src/services/agent_workspace_scope/skills.rs)：绑定路径、宏投影与目录查询。
- [Workspace 工具](../../src-tauri/crates/tt-application/src/services/agent_tools/workspace)：模型文件操作与 Shell 调用。
- [WorkspaceShell adapter](../../src-tauri/crates/tt-adapter-workspace-shell/src)：统一脚本执行与文件桥。
