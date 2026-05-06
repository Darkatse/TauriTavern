# `window.__TAURITAVERN__.api.skill` — Skill API

Skill API 用于管理本地 Agent Skill。它不是 Agent run API；Agent 只是 Skill 的消费者之一。

## 入口

```js
await (window.__TAURITAVERN__?.ready ?? window.__TAURITAVERN_MAIN_READY__);

const skill = window.__TAURITAVERN__.api.skill;
```

## 方法

```ts
type TauriTavernSkillApi = {
  list(): Promise<TauriTavernSkillIndexEntry[]>;
  listFiles(options: { name: string }): Promise<TauriTavernSkillFileRef[]>;
  pickImportArchive(): Promise<TauriTavernSkillImportInput | null>;
  discardPickedImport(input?: TauriTavernSkillImportInput | null): Promise<void>;
  previewImport(input: TauriTavernSkillImportInput): Promise<TauriTavernSkillImportPreview>;
  installImport(request: {
    input: TauriTavernSkillImportInput;
    conflictStrategy?: 'skip' | 'replace';
  }): Promise<TauriTavernSkillInstallResult>;
  readFile(options: {
    name: string;
    path: string;
    startLine?: number;
    lineCount?: number;
    startChar?: number;
    maxChars?: number;
  }): Promise<TauriTavernSkillReadResult>;
  export(options: { name: string }): Promise<TauriTavernSkillExportPayload>;
  exportSkill(options: { name: string }): Promise<TauriTavernSkillExportPayload>;
  delete(options: { name: string }): Promise<void>;
  deleteSkill(options: { name: string }): Promise<void>;
};
```

## 导入输入

用户从本机选择 `.ttskill` / `.zip` 时应优先调用 `pickImportArchive()`。它只负责唤起系统文件选择器并返回：

```ts
{ kind: 'archiveFile', path: string }
```

用户取消选择时返回 `null`。实际解包、校验、hash、冲突判断与安装仍必须走 `previewImport()` / `installImport()`。

移动端文件选择器可能返回宿主私有的临时归档路径。调用方如果放弃这次导入（例如关闭面板、重新选择、删除当前 Skill）必须调用 `discardPickedImport(input)` 释放该临时文件；`installImport()` 成功或失败后会自动释放对应的已选归档。

```ts
type TauriTavernSkillImportInput =
  | {
      kind: 'inlineFiles';
      files: Array<{
        path: string;
        encoding?: 'utf8' | 'utf-8' | 'base64';
        content: string;
        mediaType?: string;
        sizeBytes?: number;
        sha256?: string;
      }>;
      source?: unknown;
    }
  | {
      kind: 'directory';
      path: string;
      source?: unknown;
    }
  | {
      kind: 'archiveFile';
      path: string;
      source?: unknown;
    };
```

`source` 用于记录来源引用。Preset / Character embedded import 会传入稳定来源 ID，以便删除 Preset / Character 时清理仅由该来源引用的 Skill。

## 冲突语义

`previewImport()` 会返回：

```ts
type conflict.kind = 'new' | 'same' | 'different';
```

- `new`：同名 Skill 不存在。
- `same`：同名且内容 hash 相同。
- `different`：同名但内容 hash 不同，安装时必须传 `conflictStrategy`。

`installImport()` 的结果：

```ts
type action = 'installed' | 'replaced' | 'already_installed' | 'skipped';
```

不同 hash 冲突没有显式 `skip` / `replace` 时会 reject，不会自动改名。

## 读取与导出

`readFile()`：

- 只能读取已安装 Skill 内的 UTF-8 文本文件。
- `path` 必须是 Skill 相对路径。
- 支持 `startLine` / `lineCount` 行范围，或 `startChar` / `maxChars` 字符范围；两种范围不能混用。
- `maxChars` 省略时默认 20000，后端最大 80000。
- 二进制文件、非法路径、symlink escape、缺失文件都会 reject。

`export()` / `exportSkill()`：

- 返回 base64 编码的 `.ttskill` zip。
- `.ttskill` 只包含 Skill 文件本身；不会写入会改变内容 hash 的导出诊断文件。

`delete()` / `deleteSkill()`：

- 删除一个已安装 Skill 的索引记录与文件目录。
- 不会触发 source-ref 的增量解绑；这是用户显式删除 Skill 的管理动作。

## 兼容边界

- Skill 管理不是 SillyTavern 上游 API。
- Skill import/export 不触发上游 `GENERATION_*`、`TOOL_CALLS_*` 或 regex 事件。
- Agent Mode off 时，Legacy Generate 不读取 Skill。
- 模型不能通过 `api.skill` 安装或替换 Skill；当前 Skill 安装只由用户 UI / Host ABI 显式触发。
