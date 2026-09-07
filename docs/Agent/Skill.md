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

Skill Manager 可以从本地目录或 ZIP 导入，预览内容后安装；支持一次选择多个来源。角色卡和预设也可以携带 Skill。导出得到包含原始文件的 ZIP。

Skill 可以属于全局、预设、Profile 或角色。运行时按 `global → preset → profile → character` 解析，同名 Skill 由靠后的作用域覆盖，再按 Profile 的 `skills.visible` 和 `deny` 筛选。

子 Agent 使用目标 Profile 的 Skill 配置；角色与当前预设等环境信息来自 Run 冻结的输入。这样同一名称可以在某个角色或 Profile 中提供更具体的工作方法。

## 在运行中使用

模型先通过 `skill.list` 查看索引，使用 `skill.read` 阅读正文，或用 `skill.search` 查找片段。读取支持行范围，长文返回预览和续读位置。

普通 Skill 文本会展开本次输入冻结的宏，脚本源码保持原文。已安装文件供模型读取，需要摘录或修改的内容写入工作区。模型显式调用 `skill.run_script` 时才会执行包内脚本。

宿主侧的管理接口是 `api.skill`，包括预览、安装、编辑、移动、导出与删除，见 [Skill API](../API/Skill.md)。

## 运行脚本

`scripts/` 中的 JavaScript 可以处理文件、计算或生成材料。例如 `scripts/list-scenes.js`：

```js
import { workspace } from '@tauritavern/runtime';

export default function ({ path }) {
  const text = workspace.readText(path);
  const scenes = text.split('\n').filter(line => line.startsWith('## '));
  workspace.writeText('summaries/scenes.md', scenes.join('\n'));
  return { count: scenes.length, path: 'summaries/scenes.md' };
}
```

模型通过 `skill.run_script` 调用：

```json
{
  "skill": "scene-review",
  "script": "list-scenes",
  "args": { "path": "output/main.md" }
}
```

`script` 对应 `scripts/<name>.js`，名称使用小写字母、数字和连字符。入口导出 `default(args)` 或 `main(args)`，同时存在时使用 `default`。返回值为可序列化的 JSON；没有内容时返回 `null`，较大正文写入文件并返回路径。

### 可用能力

每次调用在独立 QuickJS 环境中执行，宿主能力从 `@tauritavern/runtime` 导入。标准 JavaScript 和脚本内部可完成的 `async` / `await` 可用；环境不提供网络、进程、Node、DOM 或定时器 API。

| 接口 | 用途 |
| --- | --- |
| `workspace.readText(path)` | 读取 UTF-8 文件，返回字符串 |
| `workspace.writeText(path, text)` | 写入文件，自动创建父目录 |
| `workspace.listFiles(path?)` | 无参数时列出可见顶层条目；指定目录时返回相对于该目录的文件路径 |
| `workspace.exists(path)` | 查询文件或目录，可见范围外返回 `false` |
| `context.worldInfo.entries` | Run 启动时激活的世界书 |
| `context.variables.local` / `global` | 保留原始 JSON 类型的 SillyTavern 变量 |
| `context.macro` | 冻结的名称、角色、聊天位置等宏数据 |
| `macros.render(text)` | 使用冻结值展开模板中的宏 |
| `log.info(text)` / `warn` / `error` / `debug` | 将字符串写入宿主日志 |

路径相对于 Run 工作区，读写范围来自 Invocation 的 Profile。脚本在调用时的文件快照上工作，同一路径多次写入保留最后的内容；执行成功后按快照 SHA 写回。并发修改会报告冲突，多文件写入中途失败时会列出已写入的文件。

`context` 是当前执行的副本，修改它不会写回宿主。`macros.render()` 只展开一层，未知语法保持原文，`\{{char}}` 得到字面量 `{{char}}`；支持的宏与查询见 [frozen_macros](../../src-tauri/crates/tt-domain/src/frozen_macros.rs)。

### 模块与工具箱

相对导入可以引用同一 Skill 的 `scripts/**/*.js`，例如 `import { format } from './helpers.js'`。常用库已随应用提供：

| 模块 | 用途 |
| --- | --- |
| `@tauritavern/kit/dayjs` | 日期处理 |
| `@tauritavern/kit/es-toolkit` | 数组、对象和字符串处理 |
| `@tauritavern/kit/fast-xml-parser` | XML 解析、校验与构建 |
| `@tauritavern/kit/marked` | Markdown 转 HTML |
| `@tauritavern/kit/papaparse` | CSV 解析与生成 |
| `@tauritavern/kit/slugify` | 生成适合路径或标识符的文本 |

这些库保留各自的 API，功能仍受 QuickJS 环境限制。其他库可打包成 ESM 放入 `scripts/vendor/`，通过相对路径导入。

脚本每次执行限时 30 秒，内存 32 MiB，返回值 256 KiB。模块和输入输出的具体预算见下方引擎源码。

## 源码

Skill 保存在 `_tauritavern/skills/`，按作用域组织安装目录，索引位于 `index/skills.json`。

- [SkillService](../../src-tauri/crates/tt-application/src/services/skill_service.rs)：管理与有效 Skill 解析。
- [file_skill_repository](../../src-tauri/crates/tt-adapter-storage-userdata/src/repositories/file_skill_repository)：包格式、安装、索引与文件读写。
- [skill_scope.rs](../../src-tauri/crates/tt-application/src/services/agent_runtime_service/skill_scope.rs)：Invocation 的作用域。
- [Skill 工具](../../src-tauri/crates/tt-application/src/services/agent_tools/skill)：模型读取和脚本调用。
- [script.rs](../../src-tauri/crates/tt-application/src/services/agent_tools/skill/script.rs)：脚本输入快照与工作区写回。
- [QuickJS engine](../../src-tauri/crates/tt-adapter-quickjs/src/engine.rs)：脚本执行与资源预算。
