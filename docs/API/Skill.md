# Skill API

`api.skill` 管理本地知识包，包括导入、文件编辑、作用域和导出。知识包与脚本的编写方法见 [Skill](../Agent/Skill.md)。

## 入口

```js
await (window.__TAURITAVERN__?.ready ?? window.__TAURITAVERN_MAIN_READY__);
const skill = window.__TAURITAVERN__.api.skill;
const installed = await skill.list();
```

## 管理与文件

| 方法 | 行为 |
| --- | --- |
| `list({ scope? }?)` | 返回安装索引列表，可按作用域筛选 |
| `listFiles({ scope?, name })` | 列出包内文件 |
| `readFile({ scope?, name, path, startLine?, lineCount? })` | 读取 UTF-8 文件，支持 1-based 行范围 |
| `writeFile({ scope?, name, path, content, expectedSha256? })` | 写入文件，可按读取到的 SHA 检查并发修改 |
| `move({ name, fromScope, toScope, conflictStrategy? })` | 移动到另一个作用域 |
| `export({ scope?, name })` | 返回 `{ fileName, contentBase64, sha256 }`，内容为 ZIP |
| `delete({ scope?, name })` | 删除安装索引与包目录 |

作用域有 `global`、`preset`、`profile`、`character`，省略时按全局处理。完整结构见 [src/types.d.ts](../../src/types.d.ts) 的 `TauriTavernSkillScope`。

文件路径相对于 Skill 包。`readFile` 省略范围时读取全文，较长内容返回行预览和 `nextStartLine`，供调用方续读。`writeFile` 的 SHA 不匹配时返回错误。

## 选择导入来源

| 方法 | 返回内容 |
| --- | --- |
| `pickImportArchive()` | 单个归档输入，取消时为 `null` |
| `pickImportArchives()` | 一个或多个归档输入，取消时为 `null` |
| `pickImportDirectories()` | 桌面端选择一个或多个 Skill 目录，取消时为 `null` |
| `downloadImport({ url })` | 下载 HTTPS raw `SKILL.md`，返回单文件导入输入 |
| `discardPickedImport(input?)` | 释放放弃导入的临时归档；无参数时释放全部待处理输入 |

导入输入有三种形式：

```ts
type SkillImportInput =
  | { kind: 'directory'; path: string; source?: unknown }
  | { kind: 'archiveFile'; path: string; source?: unknown }
  | {
      kind: 'inlineFiles';
      files: Array<{
        path: string;
        content: string;
        encoding?: 'utf8' | 'utf-8' | 'base64';
        mediaType?: string;
        sizeBytes?: number;
        sha256?: string;
      }>;
      source?: unknown;
    };
```

`source` 记录来源关系，角色卡和预设用它关联嵌入的 Skill。ZIP 和历史 `.ttskill` 归档都可导入，导出统一使用 ZIP。

## 预览与安装

先用 `previewImport({ input, targetScope? })` 查看内容和同名冲突，再调用 `installImport({ input, targetScope?, conflictStrategy? })`。

| `conflict.kind` | 含义 | 安装方式 |
| --- | --- | --- |
| `new` | 同名包不存在 | 安装 |
| `same` | 同名且内容相同 | 合并来源，保留已有内容 |
| `different` | 同名但内容不同 | 指定 `skip` 或 `replace` |

安装结果的 `action` 为 `installed`、`replaced`、`already_installed` 或 `skipped`。多个来源逐项预览和安装，每项单独提交并返回结果；已完成的安装会保留。

`installImport` 完成后自动释放该输入的临时归档。用户放弃预览或取消整个批次时，由调用方使用 `discardPickedImport` 释放。

模型在 Run 中通过 `skill.list`、`skill.read`、`skill.search` 和 `skill.run_script` 使用已安装内容，运行过程见 [Agent 工具](../Agent/ToolSystem.md)。
