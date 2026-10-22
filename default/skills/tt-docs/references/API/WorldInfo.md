# `window.__TAURITAVERN__.api.worldInfo` — API 参考

TauriTavern 为角色卡作者、世界书工具与相关扩展提供的规范化 World Info API。

> 设计目标：只暴露“最近一次激活结果 / 实时订阅 / 跳转”这三个真正稳定的平台能力，不把上游扫描循环内部态直接变成公共契约。

## 0. 快速上手

```js
await (window.__TAURITAVERN__?.ready ?? window.__TAURITAVERN_MAIN_READY__);
const worldInfo = window.__TAURITAVERN__.api.worldInfo;
```

## 1. 数据结构

### `WorldInfoEntryRef`

```ts
type WorldInfoEntryRef = {
  world: string;
  uid: string | number;
};
```

### `WorldInfoActivationEntry`

```ts
type WorldInfoActivationEntry = {
  world: string;
  uid: string | number;
  displayName: string;
  constant: boolean;
  position?:
    | 'before'
    | 'after'
    | 'an_top'
    | 'an_bottom'
    | 'depth'
    | 'em_top'
    | 'em_bottom'
    | 'outlet';
};
```

### `WorldInfoActivationBatch`

```ts
type WorldInfoActivationBatch = {
  timestampMs: number;
  trigger: string;
  entries: WorldInfoActivationEntry[];
};
```

## 2. `getLastActivation()`

```js
const last = await worldInfo.getLastActivation();
if (last) {
  console.table(last.entries);
}
```

### 返回值

- `Promise<WorldInfoActivationBatch | null>`
- `null` 仅表示当前会话尚未捕获到任何一次最终激活结果

### 语义

- 返回的是最近一次真实生成流程对应的最终结果。
- 不返回扫描过程中的中间循环态。

## 3. `subscribeActivations(handler)`

```js
const unsubscribe = await worldInfo.subscribeActivations((batch) => {
  console.log('Activated entries:', batch.entries.map((entry) => entry.displayName));
});
```

### 返回值

- `Promise<unsubscribe>`

### 语义

- 只推送最终激活批次。
- 不复播历史结果。
- 若 UI 初始化时需要先显示最近一次结果，应先调用 `getLastActivation()`。

## 4. `openEntry(ref)`

```js
const result = await worldInfo.openEntry({
  world: 'My Lorebook',
  uid: 42,
});

if (!result.opened) {
  console.warn('World info entry not found');
}
```

### 返回值

```ts
Promise<{ opened: boolean }>
```

### 语义

- `opened: true`
  - 表示宿主已成功打开目标世界书，并尝试定位到目标条目。
- `opened: false`
  - 表示目标世界书或目标条目不存在。
- 其他异常直接抛出
  - 这是刻意的，便于开发阶段及时暴露问题。

## 5. v1 收缩边界

- `api.worldInfo` 不直接暴露 `WORLD_INFO_ACTIVATED` 或 `WORLDINFO_SCAN_DONE` 原始载荷。
- 只承诺激活条目的稳定显示字段：
  - `world`
  - `uid`
  - `displayName`
  - `constant`
  - `position?`
- 不把扫描预算、递归控制、中间态对象升格为 Public Contract。
- `openEntry()` 必须复用上游 World Info 模块自身的导航能力；宿主 ABI 层不直接承诺 DOM 结构细节。
