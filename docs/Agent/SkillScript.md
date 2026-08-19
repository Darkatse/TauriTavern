# TauriTavern Skill Script 指南

本文档面向 Skill 开发者，说明如何在 Skill 包的 `scripts/` 目录中编写可被 Agent 执行的 JavaScript 脚本。它覆盖脚本格式、可访问的全局变量、文件系统读写边界、模块导入规则、可用的第三方库以及沙箱限制。

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

脚本使用 ES Module 语法（`export` / `import`），必须导出一个 `default` 函数或一个具名的 `main` 函数。引擎优先调用 `default(args)`，不存在时回退到 `main(args)`。

`args` 是调用时传入的参数对象（JSON 可序列化），由 Agent 在 `skill.run_script` 的 `args` 字段中提供。返回值必须是 JSON 可序列化的值（`undefined`、函数、`Symbol` 等无法序列化）。

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

## 3. 可访问的全局变量

沙箱在每次执行时注入以下全局对象。除此之外**没有** `process`、`Buffer`、`fs`、`http`、`crypto`、`setTimeout`、`setInterval` 等 Node 或浏览器 API。

| 全局变量 | 读写 | 说明 |
| --- | --- | --- |
| `$fs` | 读写 | 受沙箱策略门控的文件系统 API |
| `$worldInfo` | 只读 | 当前 run 预取的激活世界书条目快照 |
| `$variables` | 只读 | SillyTavern 变量快照（`local` / `global` 两个作用域） |
| `$log` | 只写 | 日志经宿主 tracing 收集输出 |

### 3.1 `$fs` — 文件系统 API

`$fs` 提供受限的文件读写能力。所有路径相对于当前 run 的 workspace 根目录，经过路径清洗后必须落在 Agent Profile 的 `visible_roots`（读）或 `writable_roots`（写）内。绝对路径和 `..` 逃逸会被拒绝。

```js
// 读取文件内容（路径相对 workspace 根目录）
const content = $fs.readText('output/config.json');

// 写入文件内容（自动创建父目录）
$fs.writeText('output/result.txt', 'Hello World');

// 列出目录下的条目名（相对路径前缀）
// 无参：列出 workspace 根目录顶层条目名
// 有参：列出指定目录下条目的 workspace 相对路径
const files = $fs.listFiles('output');
// 返回: ['a.md', 'b.txt', ...]

// 检查文件或目录是否存在
const exists = $fs.exists('output/config.json');
```

| 方法 | 签名 | 说明 |
| --- | --- | --- |
| `readText(path)` | `(path: string) → string` | 读取 UTF-8 文本文件；路径必须在 visible roots 内 |
| `writeText(path, content)` | `(path: string, content: string) → void` | 写入 UTF-8 文本文件；路径必须在 writable roots 内；自动创建父目录 |
| `listFiles(path?)` | `(path?: string) → string[]` | 列出目录条目；无参列出根目录条目名，有参列出相对路径；读权限同 `readText` |
| `exists(path)` | `(path: string) → boolean` | 检查文件或目录是否存在；路径不在 visible roots 内时返回 `false` 而非抛错 |

### 3.2 `$worldInfo` — 世界书快照（只读）

`$worldInfo` 提供当前 run 启动时预取的激活世界书条目快照。数据是冻结的，脚本无法修改世界书。

```js
// 读取所有激活的世界书条目
const result = $worldInfo.readActivated();
// 返回: { entries: [{ uid, ref, content, constant, position, displayName, world }, ...] }

// 按 ref 读取特定的世界书条目
const result = $worldInfo.readEntries(['worldinfo:lore#1', 'worldinfo:chars#2']);
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

### 3.3 `$variables` — SillyTavern 变量快照（只读）

`$variables` 提供当前 run 的 SillyTavern 变量快照，接口签名与 `getContext().variables` 保持一致。`local` 和 `global` 两个作用域各提供 `get` / `has`（只读）方法。

```js
// 读取 local 变量（不存在时返回空字符串，与 ST getLocalVariable 行为一致）
const score = $variables.local.get('score');

// 检查 local 变量是否存在
const hasName = $variables.local.has('name');

// 读取 global 变量
const theme = $variables.global.get('theme');

// 检查 global 变量是否存在
const hasGlobal = $variables.global.has('theme');
```

| 方法 | 签名 | 说明 |
| --- | --- | --- |
| `.local.get(name)` | `(name: string) → any` | 读取 local 变量值；缺失时返回空字符串 `''` |
| `.local.has(name)` | `(name: string) → boolean` | 检查 local 变量是否存在 |
| `.global.get(name)` | `(name: string) → any` | 读取 global 变量值；缺失时返回空字符串 `''` |
| `.global.has(name)` | `(name: string) → boolean` | 检查 global 变量是否存在 |

写操作（`set` / `del` / `add` / `inc` / `dec`）存在但会 fail-fast 抛出 `"variables are read-only in skill script sandbox"` 错误。这些方法仅为保持接口签名一致，脚本不能修改变量。

### 3.4 `$log` — 日志 API

`$log` 将日志输出到宿主的日志系统，供开发者调试。日志不会进入 Agent 上下文或聊天消息。

```js
$log.info('Processing started');
$log.warn('Deprecated format detected');
$log.error('Failed to parse input');
$log.debug('Debug value: ' + JSON.stringify(someValue));
```

`$log` 的各方法接受一个 `string` 参数；传入非字符串会抛出类型错误。输出前缀为 `[skill-script]`，写入宿主日志系统。

## 4. 文件系统读写边界

`$fs` 的读写权限由 Agent Profile 的 `workspace.visible_roots` 和 `workspace.writable_roots` 控制。这两个列表是相对 workspace 根目录的子目录名。

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
  tool-results/← 不可读写
  ...
```

规则：

- 读操作（`readText` / `listFiles`）：路径清洗后必须落在某个 visible root 内。
- 写操作（`writeText`）：路径清洗后必须落在某个 writable root 内。
- 绝对路径（如 `/etc/passwd`）一律拒绝。
- `..` 路径逃逸（如 `../outside` 或 `output/../../escape`）一律拒绝。
- 路径中包含 NUL 字符一律拒绝。
- `exists` 是例外：路径不在 visible roots 内时返回 `false` 而非抛错，方便脚本做条件判断。

Profile 的 visible / writable roots 可被自定义 Profile 覆盖；脚本开发者应以实际运行的 Profile 配置为准。如果脚本需要写入某个目录，确保该目录在 Profile 的 `writable_roots` 中。

## 5. 模块导入

脚本支持 ES Module 的 `import` 语法，模块解析受沙箱白名单门控。有两种导入方式：

### 5.1 相对导入（`./` 或 `../`）

相对导入只能引用**当前 Skill 自身 `scripts/` 目录内**的文件。清洗后的路径必须仍位于 `scripts/` 目录内，否则拒绝。

```js
// 导入同目录下的 helper.js
import { format } from './helper.js';

// 导入子目录下的模块
import { parse } from './lib/parser.js';
```

解析规则（按顺序尝试）：

1. `./helper.js` → `scripts/helper.js`
2. `./helper` → `scripts/helper.js`
3. `./helper` → `scripts/helper/index.js`

```text
my-skill/
  scripts/
    main.js          ← 入口脚本
    helper.js        ← 可被 main.js import
    lib/
      parser.js      ← 可被 main.js import (./lib/parser.js)
```

### 5.2 裸模块名导入（公共库）

裸模块名（不以 `./` 或 `../` 开头的模块名）从公共库目录解析。公共库目录为 `{data_root}/skill-libs/`，由 TauriTavern 在启动时从内嵌资源释放。

```js
import { marked } from 'marked';
import dayjs from 'dayjs';
import { chunk, uniq } from 'es-toolkit';
```

解析规则（按顺序尝试）：

1. `import ... from 'libname'` → `{skill-libs}/libname.js`
2. `import ... from 'libname'` → `{skill-libs}/libname/index.js`

绝对路径作为模块名一律拒绝。裸模块名不能在公共库之间或库内部使用相对路径互相引用——公共库是单文件自包含的。

## 6. 可用的第三方库

公共库目录当前提供以下库，均以单文件 ESM bundle 形式分发，自包含、零外部依赖：

| 模块名 | 版本 | 用途 |
| --- | --- | --- |
| `marked` | 18.x | Markdown → HTML（剧情摘要、世界书渲染等） |
| `dayjs` | 1.11.x | 时间解析 / 格式化 / 计算（不可变 API） |
| `es-toolkit` | 1.x | 通用工具库（lodash 的轻量替代）：数组 / 字符串 / 对象 / 函数 |
| `slugify` | 1.x | 字符串 slug 化（标题 → URL / 文件名） |
| `fast-xml-parser` | 5.x | XML ↔ JS 对象双向转换（含属性 / CDATA / 命名空间） |
| `papaparse` | 5.x | CSV ↔ JSON 双向转换（自动分隔符检测、RFC 4180） |

> JSON 处理不需要库：QuickJS 内置 `JSON.parse` / `JSON.stringify`。
> 正则表达式不需要库：QuickJS 内置完整 `RegExp` 支持。

### 6.1 用法示例

```js
// marked — Markdown → HTML
import { marked } from 'marked';

export default function (args) {
  const html = marked.parse(args.markdown ?? '');
  return { html };
}
```

```js
// dayjs — 时间处理
import dayjs from 'dayjs';

export default function (args) {
  const d = dayjs(args.date);
  return {
    formatted: d.format('YYYY-MM-DD HH:mm:ss'),
    plusDays: d.add(3, 'day').format('YYYY-MM-DD'),
  };
}
```

```js
// es-toolkit — 通用工具
import { chunk, uniq, pick, sum, groupBy } from 'es-toolkit';

export default function () {
  return {
    chunk: chunk([1, 2, 3, 4, 5], 2),           // [[1,2],[3,4],[5]]
    uniq: uniq([1, 1, 2, 3, 3]),                 // [1,2,3]
    pick: pick({ a: 1, b: 2, c: 3 }, ['a', 'c']),// { a:1, c:3 }
    sum: sum([1, 2, 3]),                          // 6
  };
}
```

```js
// slugify — 标题 → slug
import slugify from 'slugify';

export default function (args) {
  return { slug: slugify(args.title ?? '') };
}
```

```js
// fast-xml-parser — XML ↔ JS 对象
import { XMLParser, XMLBuilder } from 'fast-xml-parser';

export default function (args) {
  const parser = new XMLParser({ ignoreAttributes: false });
  const obj = parser.parse(args.xml);
  const builder = new XMLBuilder({ ignoreAttributes: false });
  const xml = builder.build({ note: { to: 'Tove' } });
  return { obj, xml };
}
```

```js
// papaparse — CSV ↔ JSON
import Papa from 'papaparse';

export default function (args) {
  const result = Papa.parse(args.csv, { header: true, skipEmptyLines: true });
  const csv = Papa.unparse([{ name: 'A', age: 20 }, { name: 'B', age: 30 }]);
  return { rows: result.data, csv };
}
```

### 6.2 dayjs 插件说明

dayjs 默认只带核心功能。当前提供的是**核心单文件**，插件（如 `relativeTime`、`utc`、`timezone`）与 locale 未打包。如需这些能力，应在公共库目录中以单独的裸模块名形式提供打包后的插件文件。

## 7. 沙箱限制

### 7.1 资源限制

| 限制项 | 默认值 | 说明 |
| --- | --- | --- |
| 内存上限 | 32 MB | 超限时 QuickJS 自动中断 |
| 栈大小上限 | 256 KB | 超限时 QuickJS 自动中断 |
| 执行超时 | 30 秒 | 超时后通过 interrupt handler 中断（如死循环） |
| 返回值大小上限 | 256 KB | 返回值 JSON 序列化后超过此大小则 fail-fast |

超时和返回值超限分别以专用错误传播给 Agent：

- 超时：`skill.run_script_execution_failed`，消息包含 `timed out`。
- 返回值超限：`skill.run_script_result_too_large`，提示用 `$fs.writeText` 将大输出写入 workspace 而非直接返回。

### 7.2 隔离语义

- 每次执行创建**全新的 QuickJS Runtime + Context**，不存在跨执行的共享状态。
- 脚本在 `spawn_blocking` 中同步执行，不阻塞 tokio 运行时。
- 没有网络访问能力：不能发起 HTTP 请求、不能使用 `fetch` / `XMLHttpRequest`。
- 没有定时器：不能使用 `setTimeout` / `setInterval` / `setImmediate`。
- 没有进程访问能力：不能执行 shell 命令、不能 spawn 子进程。
- 没有 Node / 浏览器内置对象：`process`、`Buffer`、`module`、`require`（CommonJS）、`window`、`document` 等均不可用；脚本只能通过 ES Module `import` 和注入的 `$fs` / `$worldInfo` / `$variables` / `$log` 访问外部能力。
- `eval()` / `new Function()` 是 QuickJS 的标准语言特性、并未禁用，但它们无法逃逸沙箱（没有 Node 对象、没有网络、文件读写受 `$fs` 门禁）。不要依赖它们访问外部资源。

### 7.3 安全边界汇总

| 能力 | 是否可用 |
| --- | --- |
| 读 visible roots 内文件 | 是 |
| 写 writable roots 内文件 | 是 |
| 读 visible roots 外文件 | 否（fail-fast） |
| 绝对路径 / `..` 逃逸 | 否（fail-fast） |
| 导入 Skill scripts/ 内的模块 | 是 |
| 导入公共库 | 是 |
| 导入 scripts/ 外的 Skill 文件 | 否（fail-fast） |
| 网络请求 | 否 |
| 修改变量 / 世界书 | 否（fail-fast） |
| 访问 Node / 浏览器内置对象 | 否 |
| 访问进程 / shell | 否 |

## 8. 完整示例

### 8.1 数据处理脚本

```js
// skills/data-processor/scripts/process.js

import { chunk } from 'es-toolkit';

export default function (args) {
  const { text, format } = args;

  $log.info('Processing text');

  // 读取配置文件
  const config = JSON.parse($fs.readText('output/config.json'));

  // 处理文本
  let result = text.trim();
  if (format === 'uppercase') {
    result = result.toUpperCase();
  }

  // 分块处理
  const lines = result.split('\n');
  const batches = chunk(lines, config.batchSize ?? 10);

  // 写入结果
  $fs.writeText('output/result.txt', batches.map(b => b.join('\n')).join('\n---\n'));

  // 访问世界书
  const worldData = $worldInfo.readActivated();
  const loreEntries = worldData.entries.filter(e => e.constant);

  // 读取变量
  const counter = $variables.local.get('counter');

  return {
    processed: result,
    batchCount: batches.length,
    loreCount: loreEntries.length,
    counter,
  };
}
```

### 8.2 使用多模块的脚本

```js
// skills/markdown-builder/scripts/main.js

import { marked } from 'marked';
import dayjs from 'dayjs';
import { formatHeader } from './format.js';

export default function (args) {
  const header = formatHeader(args.title);
  const timestamp = dayjs().format('YYYY-MM-DD HH:mm:ss');
  const markdown = `# ${header}\n\n_Generated at ${timestamp}_\n\n${args.body}`;
  const html = marked.parse(markdown);

  $fs.writeText('output/article.html', html);

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

### 8.3 SKILL.md 中的脚本说明

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

## 9. 约束清单

1. **脚本必须是单文件 ESM**：入口脚本可以 `import` 相对模块和公共库，但公共库自身必须是单文件自包含、无相对导入。
2. **返回值必须 JSON 可序列化**：`undefined`、函数、`Symbol`、循环引用等无法序列化的值会导致失败。
3. **大输出写入文件**：返回值超过 256 KB 会 fail-fast；大输出应通过 `$fs.writeText` 写入 workspace，返回值只携带摘要或文件路径。
4. **路径必须相对 workspace 根目录**：不要使用绝对路径或 `..` 逃逸。
5. **变量和世界书是只读的**：脚本不能修改 SillyTavern 变量或世界书条目。
6. **脚本名称必须匹配 `^[a-z0-9][a-z0-9-]*$`**：小写字母、数字、连字符，字母或数字开头；不带 `.js` 扩展名。
7. **不要依赖跨执行状态**：每次 `skill.run_script` 调用都是全新的 Runtime，没有全局变量、缓存或闭包持久化。
8. **不要使用 Node / 浏览器 API**：没有 `process`、`Buffer`、`fs`、`http`、`crypto`、`setTimeout`、`fetch` 等。
9. **在 SKILL.md 中记录脚本契约**：模型通过 SKILL.md 了解脚本参数和返回值；缺少文档会导致模型无法正确调用。
