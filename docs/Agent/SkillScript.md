# TauriTavern Skill Script 指南

本文档面向 Skill 开发者，说明如何在 Skill 包的 `scripts/` 目录中编写可被 Agent 执行的 JavaScript 脚本。它覆盖脚本格式、Runtime 模块 API、文件系统读写边界、脚本工具箱、模块导入以及沙箱限制。

> 本文记录的是**当前已落地**的 `skill.run_script` 能力，不是方案讨论。长期开发以本文、`docs/Agent/Skill.md` 与 `docs/API/Skill.md` 为准。

## 1. 概述

Skill 是 Agent 按需读取的本地知识包，一个 Skill 的目录结构如下：

```text
my-skill/
  SKILL.md
  references/
  examples/
  assets/
  scripts/
  agents/tauritavern.json
```

`scripts/` 目录下可以放置 `.js` 脚本文件。Agent 在运行期间通过 `skill.run_script` 工具在**隔离的 QuickJS 沙箱**中执行这些脚本，执行结果以 JSON 形式返回给模型作为后续上下文。

脚本不是自动执行的——只有当 Agent 显式调用 `skill.run_script` 时才会运行。Skill 通过 `SKILL.md` 告诉模型有哪些脚本可用、每个脚本接受什么参数、返回什么结果。

## 2. 脚本格式

脚本使用 ES Module 语法（`export` / `import`），必须导出一个 `default` 函数或一个具名的 `main` 函数。引擎优先调用 `default(args)`，不存在时回退到 `main(args)`。如果两者都不存在，脚本会得到明确报错（而非静默返回 `undefined`）。

入口函数可以是同步或 `async` 函数。如果返回的是 `Promise`，引擎会 `await` 到它 settle（rejection 作为 JS 异常传播）。脚本也支持**顶层 `await`**（top-level await），但仅限能 settle 的 Promise——沙箱内没有宿主异步 API，永远 pending 的 `await` 会导致执行错误。

`args` 是调用时传入的参数对象（JSON 可序列化），由 Agent 在 `skill.run_script` 的 `args` 字段中提供。返回值经 `JSON.stringify` 序列化后传回宿主，因此必须是 JSON 可序列化的值。循环引用、`BigInt`、函数、`Symbol` 会在 `JSON.stringify` 阶段报错；`undefined` 返回值会被明确拒绝（返回 `null` 显式表示空值）。

```js
// 方式 1：导出 default 函数（推荐）
export default function (args) {
  const { input, options } = args;
  return {
    success: true,
    data: processInput(input),
  };
}

// 方式 2：导出 main 函数
export function main(args) {
  return { result: 'processed' };
}
```

脚本名称（`skill.run_script` 的 `script` 参数）是 `scripts/` 目录下不带 `.js` 扩展名的文件名，且必须匹配 `^[a-z0-9][a-z0-9-]*$`（小写字母、数字、连字符，字母或数字开头）。例如 `scripts/helper.js` 对应 script 名 `helper`，`scripts/parse-xml.js` 对应 `parse-xml`。

## 3. Runtime 模块（`@tauritavern/runtime`）

宿主能力经 ES Module 导入，沙箱不注入任何全局对象。脚本通过 `import` 从 `@tauritavern/runtime` 获取宿主能力：

```js
import { context, workspace, log } from '@tauritavern/runtime';
```

导出表：

| 导出 | 读写 | 说明 |
| --- | --- | --- |
| `workspace` | 读写 | 受沙箱策略门控的文件 API |
| `context` | 只读 | 与宿主状态隔离的 `worldInfo` + `variables` + `macro` 快照 |
| `macros` | 只读 | `render(text)` 使用冻结值展开宏 |
| `log` | 只写 | 经宿主 tracing 输出的日志 API |

除此之外**没有** `process`、`Buffer`、`fs`、`http`、`crypto`、`setTimeout`、`setInterval` 等 Node 或浏览器 API。

### 3.1 `workspace` — 文件 API

`workspace` 提供受限的文件读写能力。所有路径相对于当前 run 的 workspace 根目录，经过路径清洗后必须落在当前 invocation Workspace policy 的 `visible_roots`（读）或 `writable_roots`（写）内。绝对路径和 `..` 逃逸会被拒绝。

```js
import { workspace } from '@tauritavern/runtime';

// 读取文件内容（路径相对 workspace 根目录）
const content = workspace.readText('output/config.json');

// 写入文件内容（自动创建父目录）
workspace.writeText('output/result.txt', 'Hello World');

// 列出目录下的条目名（相对路径前缀）
// 无参：列出 workspace 根目录顶层条目名
// 有参：列出指定目录下条目的 workspace 相对路径
const files = workspace.listFiles('output');
// 返回: ['a.md', 'b.txt', ...]

// 检查文件或目录是否存在
const exists = workspace.exists('output/config.json');
```

| 方法 | 签名 | 说明 |
| --- | --- | --- |
| `readText(path)` | `(path: string) → string` | 读取 UTF-8 文本文件；路径必须在 visible roots 内 |
| `writeText(path, content)` | `(path: string, content: string) → void` | 写入 UTF-8 文本文件；路径必须是 writable root 的子项；自动创建父目录 |
| `listFiles(path?)` | `(path?: string) → string[]` | 列出目录条目；无参列出根目录条目名，有参列出相对路径；读权限同 `readText` |
| `exists(path)` | `(path: string) → boolean` | 检查文件或目录是否存在；路径不在 visible roots 内时返回 `false` 而非抛错 |

### 3.2 `context.worldInfo` — 世界书快照（只读）

`context.worldInfo` 是当前 run 启动时预取的激活世界书快照。它是普通 JSON；脚本可以在本次执行中处理这份副本，但不会修改宿主世界书。

```js
import { context } from '@tauritavern/runtime';

const entries = context.worldInfo.entries;
// [{ uid, ref, content, constant, position, displayName, world }, ...]

const wanted = new Set(['worldinfo:lore#1', 'worldinfo:chars#2']);
const selected = entries.filter(entry => wanted.has(entry.ref));
```

每个条目的字段：

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `uid` | `string` | 条目 UID |
| `ref` | `string` | 条目引用键，格式 `worldinfo:{world}#{uid}` |
| `content` | `string` | 条目正文内容 |
| `constant` | `boolean` | 是否为常驻条目 |
| `position` | `string?` | 插入位置（可能为空） |
| `displayName` | `string?` | 显示名称（可能为空） |
| `world` | `string` | 所属世界书名称 |

### 3.3 `context.variables` — SillyTavern 变量快照（只读）

`context.variables` 是当前 run 的 SillyTavern 变量快照，包含 `local` 和 `global` 两个普通 JSON 对象。值保持快照中的原始 JSON 类型。

```js
import { context } from '@tauritavern/runtime';

// 读取 local 变量
const score = context.variables.local.score;
const name = context.variables.local['name'];

// 检查 local 变量是否存在
const hasName = Object.hasOwn(context.variables.local, 'name');

// 读取 global 变量
const theme = context.variables.global.theme;
```

变量不存在时得到标准 JavaScript `undefined`，可按需使用 `??` 提供默认值。修改这份对象只影响本次脚本执行，不会写回 SillyTavern。

### 3.4 `context.macro` — 宏上下文快照（只读）

`context.macro` 提供本次 run 启动时冻结的前端宏数据，保留原始 JSON 类型。修改这份对象只影响本次脚本执行，不会写回宿主。

```js
import { context } from '@tauritavern/runtime';

const charName = context.macro.names?.char;
const description = context.macro.character?.description;
const lastMessageId = context.macro.chat?.lastMessageId;
```

| 字段 | 说明 |
| --- | --- |
| `schemaVersion` | 宏上下文版本号 |
| `names` | user、char、group 等名称 |
| `character` | description、scenario、firstMessage 等角色字段；personaPosition 为数值，alternateGreetings 为字符串数组 |
| `system.model` | 前端捕获时使用的模型 |
| `chat.lastMessageId` | 最后一条合格消息的 0-based 索引，跳过正在生成新 swipe 的消息 |
| `chat.lastSwipeId` | 末尾消息已有的 swipe 数量 |
| `chat.currentSwipeId` | 末尾消息的 1-based swipe 编号，包含正在生成的新 swipe |
| `builtins` | 捕获时的时间、聊天、输入、Token 上限与提示词模板文本 |
| `variableValues.local` / `variableValues.global` | 按前端变量读取规则转换后的宏文本；原始值仍在 `context.variables` |

三个 `chat` 值均为字符串，不可用时为 `""`；生成新 swipe 时，它们可能指向不同消息。缺少宏快照时 `context.macro` 为 `{}`，字段可用可选链读取。

### 3.5 `macros.render(text)` — 冻结宏重放

```js
import { macros, workspace } from '@tauritavern/runtime';
const text = macros.render(workspace.readText('output/template.md'));
```

返回展开后的字符串。除名称、角色／Persona 字段、模型与聊天位置外，还支持：

- 时间与状态：`time`、`date`、`weekday`、`isotime`、`isodate`、`idleDuration` / `idle_duration`、`input`、`isMobile`、`lastGenerationType`、`maxPrompt` / `maxContext` / `maxResponse` 及其 `Tokens` 别名，均使用捕获时的值。
- 聊天：`lastMessage`、`lastUserMessage`、`lastCharMessage`、`firstIncludedMessageId`、`firstDisplayedMessageId`、`allChatRange`。
- 提示词：内置 `instruct*` 序列及别名、`systemPrompt`、`defaultSystemPrompt`、`exampleSeparator` / `chatSeparator`、`chatStart`、`reasoningPrefix` / `reasoningSuffix` / `reasoningSeparator`。
- 固定文本：无参数 `space`、`newline`、`noop`。
- 单层参数查询：`{{greeting::1}}`（0 为主开场白，1 起为备选；也支持 `charFirstMessage`）、`{{getvar::Name}}` / `{{getglobalvar::Name}}`、`{{hasvar::Name}}` / `{{hasglobalvar::Name}}`（别名 `varexists` / `globalvarexists`），以及 `{{outlet::key}}`。

宏名不区分大小写，变量名和 outlet key 区分大小写。参数使用 `::` 加字面量；已捕获的查询集合中，缺失项或越界 greeting 返回空字符串，存在性查询返回 `true` / `false`。替换值不再递归展开；未知、嵌套或不支持的语法保持原文，`\{{char}}` 保留字面量 `{{char}}`。扩展注册宏与变量写入宏不参与重放。单次展开上限与脚本工作区输出上限相同。

### 3.6 `log` — 日志 API

`log` 将日志输出到宿主的日志系统，供开发者调试。日志不会进入 Agent 上下文或聊天消息。

```js
import { log } from '@tauritavern/runtime';

log.info('Processing started');
log.warn('Deprecated format detected');
log.error('Failed to parse input');
log.debug('Debug value: ' + JSON.stringify(someValue));
```

`log` 的各方法接受一个 `string` 参数；传入非字符串会抛出类型错误。输出前缀为 `[skill-script]`，写入宿主日志系统。

## 4. 文件系统读写边界

`workspace` 的基础读写权限来自 Agent Profile，并由当前 invocation 的 Workspace manifest 投影为最终 policy。这些 roots 是相对 workspace 根目录的子目录名。

默认 Agent Profile 的 visible / writable roots 为：

| Root | 说明 |
| --- | --- |
| `output` | Agent 最终输出（artifact）目录 |
| `scratch` | 临时草稿目录 |
| `plan` | 计划文件目录 |
| `summaries` | 摘要文件目录 |
| `persist` | 持久化目录 |

```text
run-workspace/
  output/      ← 可读可写
  scratch/     ← 可读可写
  plan/        ← 可读可写
  summaries/   ← 可读可写
  persist/     ← 可读可写
  input/       ← 不可读写
  tool-args/   ← 不可读写
  tool-results/← 默认不可见；return-mode 子 Agent 中只读
  ...
```

规则：

- 读操作（`readText` / `listFiles`）：路径清洗后必须落在某个 visible root 内（root 本身或其子项）。
- 写操作（`writeText`）：路径清洗后必须是某个 writable root 的**子项**（root 本身不可写，与宿主 canonical 写策略一致）。
- 绝对路径（如 `/etc/passwd`）一律拒绝。
- `..` 路径逃逸（如 `../outside` 或 `output/../../escape`）一律拒绝。
- 路径中包含 NUL 字符一律拒绝。
- `exists` 是例外：路径不在 visible roots 内时返回 `false` 而非抛错，方便脚本做条件判断。

Profile 可以自定义基础 visible / writable roots；调用级 policy 还可能进一步投影只读 root。脚本开发者应以实际 invocation 为准。如果脚本需要写入某个目录，确保该目录最终可写。

### 4.1 写入语义

`workspace.writeText` 的写入采用**最终状态语义**：

- **同路径多次写**：脚本对同一文件多次调用 `writeText` 时，引擎只保留最后一次的内容。落盘的 delta 是最终状态，而非多次追加。
- **写入冲突检测**：引擎在执行前对工作区做文件快照（含 SHA-256）。落盘时，如果文件在快照后被外部修改（SHA-256 不匹配），写入会以 `stale` 冲突报错 fail-fast——不会覆盖外部修改。
- **部分失败语义**：批量写入时如果中途某个文件失败，已成功写入的文件不会被回滚，也不会自动提交到聊天——错误消息中包含已写入文件列表与失败文件，调用者需重新读取已写入文件后再重试。

## 5. 模块导入

脚本支持 ES Module 的 `import` 语法。模块解析全部在内存中完成：常用库从 TauriTavern 脚本工具箱导入，Skill 自己的模块通过相对路径导入，脚本不接触物理文件系统。

### 5.1 相对导入（`./` 或 `../`）

相对导入引用当前 Skill `scripts/` 目录内的其他模块（含自带的第三方库文件）。执行时，Application 将 skill 的 `scripts/**/*.js` 全部读取为**内存模块快照**（逻辑模块名 → 源码字符串），相对导入按 importer 的逻辑模块名规范化解析，且只能命中这张快照中的模块——快照外的模块（含越界 `../` 与裸模块名）解析失败，模块声明/求值报错。

```js
// 导入同目录下的 helper.js
import { format } from './helper.js';

// 导入子目录下的模块
import { parse } from './lib/parser.js';

// 导入 Skill 自带的第三方库（见 5.3）
import { marked } from './vendor/marked.js';
```

模块快照受 [6.1 资源限制](#61-资源限制) 约束，超过即拒绝执行。

```text
my-skill/
  scripts/
    main.js          ← 入口脚本
    helper.js        ← 可被 main.js import
    lib/
      parser.js      ← 可被 main.js import (./lib/parser.js)
    vendor/
      marked.js      ← 自带的第三方库 (./vendor/marked.js)
```

### 5.2 脚本工具箱（`@tauritavern/kit/*`）

TauriTavern 内嵌六个常用的单文件 ESM 库。Skill 可以直接导入，不需要复制依赖，也不占用 Skill 自身的模块数量与源码预算：

| 模块 | 版本 | 用途 |
| --- | --- | --- |
| `@tauritavern/kit/dayjs` | 1.11.21 | 日期解析、格式化与计算 |
| `@tauritavern/kit/es-toolkit` | 1.50.0 | 数组、对象、字符串等通用处理 |
| `@tauritavern/kit/fast-xml-parser` | 5.10.1 | XML 校验、解析与构建 |
| `@tauritavern/kit/marked` | 18.0.9 | Markdown 转换 |
| `@tauritavern/kit/papaparse` | 5.6.0 | CSV 解析与生成 |
| `@tauritavern/kit/slugify` | 1.6.9 | 将标题转换为适合路径或标识符的文本 |

```js
import dayjs from '@tauritavern/kit/dayjs';
import { marked } from '@tauritavern/kit/marked';
import { chunk } from '@tauritavern/kit/es-toolkit';

export default function (args) {
  return {
    date: dayjs(args.date).format('YYYY-MM-DD'),
    html: marked.parse(args.markdown ?? ''),
    pages: chunk(args.items ?? [], 10),
  };
}
```

这些模块保留各自的上游 API。沙箱没有浏览器、Node 或定时器，因此依赖 `window`、`File`、`Worker`、`setTimeout` 等环境能力的库功能不可用。dayjs 只包含核心模块；插件和 locale 需要由 Skill 自带。Marked 生成 HTML，但不负责净化 HTML。

### 5.3 自带第三方库

工具箱之外的库，或必须固定为其他版本的库，可以随 Skill 的 `scripts/` 目录分发，由 Skill 作者自行管理。

放置位置：skill 的 `scripts/` 下任意子目录（推荐 `scripts/vendor/`），经相对导入使用。

打包要求：

- **单文件 ESM bundle**：把库及其全部依赖打进一个 `.js` 文件（`import`/`export` 语法保留，不能有残留的外部裸模块导入）。
- **零 Node / 浏览器依赖**：不能引用 `process`、`Buffer`、`fs`、DOM 等宿主对象。
- **计入模块快照上限**：64 个文件 / 2 MB 总量（见 5.1），带多个大库时注意精简。

推荐用 [esbuild](https://esbuild.github.io/) 打包：

```bash
# 在含 node_modules 的目录下，把 marked 连同依赖打成单文件 ESM
npx esbuild --bundle marked --format=esm --outfile=my-skill/scripts/vendor/marked.js
```

```js
// my-skill/scripts/main.js
import { marked } from './vendor/marked.js';

export default function (args) {
  return { html: marked.parse(args.markdown ?? '') };
}
```

> JSON 处理不需要库：QuickJS 内置 `JSON.parse` / `JSON.stringify`。
> 正则表达式不需要库：QuickJS 内置完整 `RegExp` 支持。
> 打包后建议本地冒烟执行一次，确认无外部依赖残留。

## 6. 沙箱限制

### 6.1 资源限制

| 限制项 | 默认值 | 说明 |
| --- | --- | --- |
| 内存上限 | 32 MB | 超限时 QuickJS 自动中断 |
| 栈大小上限 | 256 KB | 超限时 QuickJS 自动中断 |
| 执行超时 | 30 秒 | 超时后通过 interrupt handler 中断（如死循环） |
| 返回值大小上限 | 256 KB（262,144 字节） | 返回值经 `JSON.stringify` 序列化后超过此大小则 fail-fast |
| 模块快照数量上限 | 64 个 | skill `scripts/` 下 `.js` 文件数超过此上限则拒绝执行 |
| 模块快照字节上限 | 2 MiB（2,097,152 字节） | skill `scripts/` 下所有 `.js` 源码总字节数超过此上限则拒绝执行 |
| 总输入预算 | 8 MiB | 模块源码 + 工作区快照 + args + context 的总字节数，超过直接终止 |
| 总输出预算 | 1 MiB | 最终 delta + 日志 + 返回值的总字节数（每项含少量固定记账成本），超过直接终止 |
| 全局并发上限 | 2 | 多个 Agent / 子 Agent 同时执行脚本时排队 |

返回值经 JavaScript `JSON.stringify` 序列化后传回宿主。以下值会导致序列化失败并报错：

- **循环引用**：`JSON.stringify` 抛出 `Converting circular structure to JSON` TypeError。
- **`BigInt`**：`JSON.stringify` 抛出 `Do not know how to serialize a BigInt` TypeError。
- **`Symbol` / 函数**：作为对象属性值时被丢弃，作为数组元素时变成 `null`；顶层返回时得到 `undefined` 并被明确拒绝。
- **数组中的 `undefined`**：序列化为 `null`，与标准 `JSON.stringify` 一致。
- **`undefined`**：返回 `undefined` 会被明确拒绝——返回 `null` 显式表示空值。

超时和返回值超限分别以专用错误传播给 Agent：

- 超时：`skill.run_script_execution_failed`，消息包含 `timed out`。
- 返回值超限：`skill.run_script_result_too_large`，提示用 `workspace.writeText` 将大输出写入 workspace 而非直接返回。

### 6.2 隔离语义

- 每次执行创建**全新的 QuickJS Runtime + Context**，不存在跨执行的共享状态。
- 脚本在 `spawn_blocking` 中同步执行，不阻塞 tokio 运行时。
- 没有网络访问能力：不能发起 HTTP 请求、不能使用 `fetch` / `XMLHttpRequest`。
- 没有定时器：不能使用 `setTimeout` / `setInterval` / `setImmediate`。
- 没有进程访问能力：不能执行 shell 命令、不能 spawn 子进程。
- 没有 Node / 浏览器内置对象：`process`、`Buffer`、`module`、`require`（CommonJS）、`window`、`document` 等均不可用；脚本只能通过 ES Module `import` 从 `@tauritavern/runtime` 导入 `workspace` / `context` / `log` 访问外部能力。
- `eval()` / `new Function()` 是 QuickJS 的标准语言特性、并未禁用，但它们无法逃逸沙箱（没有 Node 对象、没有网络、文件读写受 `workspace` 门禁）。不要依赖它们访问外部资源。

### 6.3 安全边界汇总

| 能力 | 是否可用 |
| --- | --- |
| 读 visible roots 内文件 | 是 |
| 写 writable roots 内文件 | 是 |
| 读 visible roots 外文件 | 否（fail-fast） |
| 绝对路径 / `..` 逃逸 | 否（fail-fast） |
| 导入 Skill scripts/ 内的模块（含自带库） | 是（内存快照解析） |
| 导入 `@tauritavern/kit/*` 工具箱模块 | 是（内存加载） |
| 导入快照外 / scripts/ 外的模块 | 否（fail-fast） |
| 网络请求 | 否 |
| 修改宿主变量 / 世界书 / 宏上下文 | 否；`context` 只是本次执行的副本 |
| 访问 Node / 浏览器内置对象 | 否 |
| 访问进程 / shell | 否 |

## 7. 完整示例

### 7.1 数据处理脚本

```js
// skills/data-processor/scripts/process.js

import { context, workspace, log } from '@tauritavern/runtime';

export default function (args) {
  const { text, format } = args;

  log.info('Processing text');

  // 读取配置文件
  const config = JSON.parse(workspace.readText('output/config.json'));

  // 处理文本
  let result = text.trim();
  if (format === 'uppercase') {
    result = result.toUpperCase();
  }

  // 分块处理（原生实现，无需第三方库）
  const lines = result.split('\n');
  const size = config.batchSize ?? 10;
  const batches = [];
  for (let i = 0; i < lines.length; i += size) {
    batches.push(lines.slice(i, i + size));
  }

  // 写入结果
  workspace.writeText('output/result.txt', batches.map(b => b.join('\n')).join('\n---\n'));

  // 访问世界书
  const loreEntries = context.worldInfo.entries.filter(e => e.constant);

  // 读取变量
  const counter = context.variables.local.counter ?? '';

  return {
    processed: result,
    batchCount: batches.length,
    loreCount: loreEntries.length,
    counter,
  };
}
```

### 7.2 使用多模块与脚本工具箱

```js
// skills/markdown-builder/scripts/main.js
import { marked } from '@tauritavern/kit/marked';
import dayjs from '@tauritavern/kit/dayjs';
import { workspace } from '@tauritavern/runtime';
import { formatHeader } from './format.js';

export default function (args) {
  const header = formatHeader(args.title);
  const timestamp = dayjs().format('YYYY-MM-DD HH:mm:ss');
  const markdown = `# ${header}\n\n_Generated at ${timestamp}_\n\n${args.body}`;
  const html = marked.parse(markdown);

  workspace.writeText('output/article.html', html);

  return {
    markdown,
    html,
    bytes: html.length,
  };
}
```

```js
// skills/markdown-builder/scripts/format.js

export function formatHeader(title) {
  return title.trim().replace(/\s+/g, ' ');
}
```

### 7.3 SKILL.md 中的脚本说明

在 `SKILL.md` 中应明确记录每个脚本的参数和返回值，以便模型正确调用：

```markdown
---
name: data-processor
description: Text processing utilities with configurable format and batching.
---

## Scripts

### process

Processes input text with configurable format and batch size.

**Arguments:**
- `text` (string, required): Input text to process.
- `format` (string, optional): `"uppercase"` to uppercase the text.

**Returns:**
- `processed` (string): The processed text.
- `batchCount` (number): Number of batches generated.
- `loreCount` (number): Number of constant world info entries.
- `counter` (string): Value of the local "counter" variable.

**Side effects:**
- Reads `output/config.json`.
- Writes `output/result.txt`.
```

## 8. 最小约定

1. 使用 ES module，并导出 `default(args)` 或 `main(args)`。
2. 返回 JSON 可序列化的小结果；大内容写入 workspace。
3. workspace 路径使用当前 policy 允许的相对路径。
4. `context` 是隔离快照（`worldInfo` / `variables` / `macro`），不会把修改写回宿主。
5. 优先使用 `@tauritavern/kit/*`；其他依赖随 Skill 打包，且不能依赖 Node、浏览器或网络 API。
6. 在 `SKILL.md` 中写清脚本参数、返回值和文件副作用。
