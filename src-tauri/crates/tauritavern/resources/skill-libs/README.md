# skill-libs 公共库

本目录是 TauriTavern skill 脚本（`skill.script`）可用的公共库集合。放在这里的库会被注入 QuickJS 沙箱的模块加载白名单，skill 的 `scripts/` 内任意脚本都可以通过**裸模块名**直接 `import` 使用。

## 使用方式

在 skill 脚本中：

```js
import { marked } from 'marked';
import dayjs from 'dayjs';
import { chunk, uniq } from 'es-toolkit';
```

裸模块名解析规则（见 `tt-adapter-quickjs/src/sandbox.rs` 的 `resolve_module`）：

- `import ... from 'libname'` → 解析到 `{libs_dir}/libname.js`（本目录）；
- 如果 `libname.js` 不存在，再尝试 `{libs_dir}/libname/index.js`；
- 相对导入（`./`、`../`）只允许在 skill 自身的 `scripts/` 目录内，**不能**在公共库之间或库内部使用相对路径互相引用。

## 沙箱约束（写库和用库时都要注意）

1. **单文件自包含**：每个库必须是单个 ESM 文件，内部**不能有相对导入**（会被沙箱门控拒绝）。当前这些库已用 esbuild 打成单文件 bundle，自包含、零外部依赖。
2. **无 Node API**：QuickJS 沙箱没有 `process`、`Buffer`、`fs`、`http`、`crypto` 等 Node 内置能力。库内只允许纯 ECMAScript。
3. **无网络/定时器/进程**：沙箱只注入 `$fs`（受限文件读写）、`$worldInfo`（世界书快照）、`$log`（日志）。库不能发起网络请求或使用 `setTimeout` 等。

## 库清单

| 模块名 | 版本 | 来源 | 用途 |
|---|---|---|---|
| `marked` | 18.x | https://github.com/markedjs/marked | Markdown → HTML（剧情摘要、世界书渲染等） |
| `dayjs` | 1.11.x | https://github.com/iamkun/dayjs | 时间解析 / 格式化 / 计算（不可变 API） |
| `es-toolkit` | 1.x | https://github.com/toss/es-toolkit | 通用工具库（lodash 的轻量替代）：数组 / 字符串 / 对象 / 函数 |
| `slugify` | 1.x | https://github.com/simov/slugify | 字符串 slug 化（标题 → URL / 文件名） |
| `fast-xml-parser` | 5.x | https://github.com/NaturalIntelligence/fast-xml-parser | XML ↔ JS 对象双向转换（含属性 / CDATA / 命名空间） |
| `papaparse` | 5.x | https://github.com/mholt/PapaParse | CSV ↔ JSON 双向转换（自动分隔符检测、RFC 4180） |

> JSON 处理不需要库：QuickJS 内置 `JSON.parse` / `JSON.stringify`。
> 正则表达式不需要库：QuickJS 内置完整 `RegExp` 支持。

## 用法示例

### marked（Markdown → HTML）

```js
import { marked } from 'marked';

export default function (args) {
  const html = marked.parse(args.markdown ?? '');
  return { html };
}
```

### dayjs（时间处理）

```js
import dayjs from 'dayjs';

export default function (args) {
  const d = dayjs(args.date);
  return {
    formatted: d.format('YYYY-MM-DD HH:mm:ss'),
    plusDays: d.add(3, 'day').format('YYYY-MM-DD'),
    fromNow: d.fromNow(), // 需要 relativeTime 插件，见下文
  };
}
```

注意：dayjs 默认只带核心功能。本目录提供的是**核心单文件**，插件（如 `relativeTime`、`utc`、`timezone`）与 locale 未打包。如需这些能力，应重新打包或自行在库文件中以裸模块名方式拆分（见“更新库”）。

### es-toolkit（通用工具）

```js
import { chunk, uniq, pick, sum, groupBy } from 'es-toolkit';

export default function () {
  return {
    chunk: chunk([1, 2, 3, 4, 5], 2),          // [[1,2],[3,4],[5]]
    uniq: uniq([1, 1, 2, 3, 3]),               // [1,2,3]
    pick: pick({ a: 1, b: 2, c: 3 }, ['a', 'c']), // { a:1, c:3 }
    sum: sum([1, 2, 3]),                       // 6
  };
}
```

### slugify（标题 → slug）

```js
import slugify from 'slugify';

export default function (args) {
  // 默认只处理 ASCII；中文需配合 remove/strict 自行处理，或先用 transliteration
  return slugify(args.title ?? '');
}
```

### fast-xml-parser（XML ↔ JS 对象）

```js
import { XMLParser, XMLBuilder } from 'fast-xml-parser';

export default function (args) {
  const parser = new XMLParser({ ignoreAttributes: false });
  const obj = parser.parse(args.xml);            // XML → 对象
  const builder = new XMLBuilder({ ignoreAttributes: false });
  const xml = builder.build({ note: { to: 'Tove' } }); // 对象 → XML
  return { obj, xml };
}
```

### papaparse（CSV ↔ JSON）

```js
import Papa from 'papaparse';

export default function (args) {
  const result = Papa.parse(args.csv, { header: true, skipEmptyLines: true });
  const csv = Papa.unparse([{ name: 'A', age: 20 }, { name: 'B', age: 30 }]);
  return { rows: result.data, csv };
}
```

## 更新 / 新增库

库由 esbuild 打包生成（单文件 ESM、`platform: neutral`、`target: es2020`），打包命令与说明见临时构建目录的 `build.mjs`（构建完成后已清理，需要时按下列步骤重建）：

1. 在临时目录 `npm install <lib> esbuild`；
2. 用 esbuild `--bundle --format=esm --platform=neutral` 打包成单文件；
3. 产物放入本目录，命名为 `{模块名}.js`；
4. 用 skill.script（或 `tt-adapter-quickjs` 的引擎测试）验证可正常 import 和调用。

约束：新增库也必须是**单文件、自包含、无 Node 依赖、纯 ESM**，否则沙箱会拒绝加载。

## 许可

各库遵循其各自的许可证（marked: MIT, dayjs: MIT, es-toolkit: MIT, slugify: MIT, fast-xml-parser: MIT, papaparse: MIT）。本目录仅分发这些库的打包产物，供沙箱内使用。
