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

包名使用小写字母、数字、`-` 或 `_`。其他目录按内容需要添加；有脚本时，在 `SKILL.md` 中说明调用命令、参数、输出和失败条件。

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

const args = process.argv.slice(2);
if (args.length !== 1) {
  throw new Error('Usage: js list-scenes.js WORKSPACE_PATH');
}
const text = workspace.readText(args[0]);
const scenes = text.split('\n').filter(line => line.startsWith('## '));
workspace.writeText('summaries/scenes.md', scenes.join('\n'));
console.log(JSON.stringify({ count: scenes.length, path: 'summaries/scenes.md' }));
```

Shell 命令：

```sh
js /skills/scene-review/scripts/list-scenes.js output/main.md
```

Skill 脚本遵循统一的 [JavaScript 工作区契约](Workspace.md#javascript)。复杂输入可保存为 JSON 文件，再传入路径；命令和 API 细节见 `js --help`。

### 可用能力

脚本在 QuickJS 中执行，支持 ES 模块和 `async` / `await`，不提供 Node/Deno 标准库、DOM、网络、子进程或定时器。宿主能力从统一模块导入：

```js
import { workspace, context, macros, chat, log } from '@tauritavern/runtime';
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
| `chat.getMessage(index, options?)` | 按索引读取当前角色聊天的一条消息 |
| `chat.getMessages(indices, options?)` | 一次读取多条消息，共享同一次文件扫描 |
| `log.info(text)` / `warn` / `error` / `debug` | 将日志写入 stderr |

文件 API 的路径相对于工作区根。`context` 是冻结输入的副本，修改它不会写回宿主；缺少所需上下文时，访问对应字段会报错。

### 读取聊天消息

`chat` 只提供当前这次 Run 所属角色聊天的只读访问，索引是从 0 开始的绝对历史索引。`ok: true` 与 `chat.message_not_found` 的结果带 `totalMessages`，即 Run 冻结输入决定的上界。Profile 未授予 `chat.read_messages`、或目标是群聊与非角色聊天时，合法的 `chat` 调用都返回 `chat.unsupported`。

```js
import { chat } from '@tauritavern/runtime';

const probe = chat.getMessage(0);
if (probe.ok) {
  const latest = chat.getMessage(probe.totalMessages - 1);
  if (latest.ok) console.log(latest.role, latest.text);
}

const window = chat.getMessages([10, 11, 12], { startLine: 1, lineCount: 50 });
if (window.ok) {
  for (const message of window.messages) console.log(message.index, message.ref);
}
```

`options` 可含 `startLine`（1-based）与 `lineCount`：给出 `lineCount` 时必须同时给出 `startLine`，只给 `startLine` 表示读到消息末尾，两者都省略表示读整条；[`chat.read_messages`](ToolSystem.md#内置工具) 使用同一规则。`ref` 字段是 `chat:current#{index}:L{start}-L{end}`。

脚本读取没有行数上限，一次调用受三层预算约束：单条消息 1 MiB；一次调用 8 MiB，按每条消息的返回文本加固定开销计费，条目数上限由该预算隐含给出，请求条数超过它就按参数错误抛错；聊天读取与文件读取共用同一份 Shell 聚合输入预算与 30 秒执行预算，超出会让脚本以异常结束。

窗口因字节预算在请求的最后一行之前结束，或某一行被字节预算截断时，结果带 `preview: true`，应据此用 `startLine` 接续读取。注意工具侧 `chat.read_messages` 的 `(preview)` 还包含窗口短于消息末尾的情况，与这里的判据不同。

失败返回 `ok: false` 与 `reason`：`chat.not_found`、`chat.message_not_found`、`chat.invalid_message_range`、`chat.message_too_large`、`chat.call_too_large`、`chat.unsupported`。只有参数格式错误（例如给 `getMessages` 传空索引数组）和读取本身失败（聊天内容无法渲染，或 Run 记录缺少冻结消息计数而报 `agent.chat_input_count_missing`）才抛错。不带区间读取一条超过 1 MiB 的消息返回 `chat.message_too_large`（附 `totalBytes` 与 `maxBytes`），一次调用超过 8 MiB 返回 `chat.call_too_large`（附 `usedBytes` 与 `maxBytes`）；带 `startLine` / `lineCount` 的分段读取不受整条消息上限限制，可据此逐段读取长文本——长文本应写入工作区文件而不是日志。命令摘要见 `js --help`。

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

## 迁移旧脚本

原 `skill.run_script` 使用 `default(args)` / `main(args)` 导出函数。现在统一使用普通模块执行和命令行参数。

迁移时一起更新脚本和 `SKILL.md` 中的命令：

1. 将输入从函数参数对接到 `process.argv.slice(2)`；复杂对象可以改用 JSON 文件，并保留原有业务校验。
2. 显式调用业务函数。可以保留原函数作为命名函数或导出函数，但解释器不会自动调用 `default` 或 `main`。
3. 将需要交给调用者的返回值显式输出；日志使用 `console.error()` 或 runtime 的 `log`，避免混入 JSON stdout。
4. 用异常或非零 `process.exitCode` 报告失败，更新帮助信息与使用示例。

例如，保留原来的业务函数并命名为 `main`，在同一模块末尾增加入口：

```js
const args = process.argv.slice(2);
if (args.length !== 1) {
  throw new Error('Usage: js task.js WORKSPACE_PATH');
}
console.log(JSON.stringify(await main({ path: args[0] })));
```

调用改为 `js /skills/<name>/scripts/task.js output/main.md`。如果原函数只修改文件、不返回结果，直接 `await main(...)`，无需输出 `undefined`。

仅移除旧命令选项并不能完成迁移：只导出函数的模块可能以 0 退出，但业务函数没有执行。使用代表性输入验证 stdout、stderr、退出码和最终文件结果；移植后的文件操作仍遵循工作区即时生效、失败不回滚的规则。Agent 运行中的 `skills/` 是只读视图，包内源码与说明应在原包或 Skill Manager 中更新。

## 源码

Skill 保存在 `_tauritavern/skills/`，按作用域组织安装目录，索引位于 `index/skills.json`。

- [SkillService](../../src-tauri/crates/tt-application/src/services/skill_service.rs)：管理与有效 Skill 解析。
- [file_skill_repository](../../src-tauri/crates/tt-adapter-storage-userdata/src/repositories/file_skill_repository)：包格式、安装、索引与原始文件读写。
- [skill_scope.rs](../../src-tauri/crates/tt-application/src/services/agent_runtime_service/skill_scope.rs)：Invocation 的作用域。
- [Skill 文件视图](../../src-tauri/crates/tt-application/src/services/agent_workspace_scope/skills.rs)：绑定路径、宏投影与目录查询。
- [Workspace 工具](../../src-tauri/crates/tt-application/src/services/agent_tools/workspace)：模型文件操作与 Shell 调用。
- [WorkspaceShell adapter](../../src-tauri/crates/tt-adapter-workspace-shell/src)：统一脚本执行与文件桥。
