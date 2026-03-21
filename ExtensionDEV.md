# Extension Development (TauriTavern)

本文档面向 **SillyTavern 风格的 third-party 前端扩展作者**，描述在 TauriTavern 中开发/适配扩展时的关键约束与专属 API。

> 目标：让扩展作者在 windowed payload（聊天记录分片加载）下仍能“正确拿到历史 + 能持久化状态 + 能做检索”，并且对移动端友好。

## 1. 扩展目录与资源契约

TauriTavern 当前的 third-party 扩展目录布局与优先级：

- local third-party 扩展：`data/default-user/extensions/<folder>`
- global third-party 扩展：`data/extensions/third-party/<folder>`
- 同名覆盖：**local 优先**（会覆盖 global）

扩展资源在浏览器侧必须通过同源端点加载：

- `/scripts/extensions/third-party/<folder>/<path>`

更完整的“发现/激活/资源端点”现状说明见：

- `docs/CurrentState/ThirdPartyExtensions.md`

## 2. 运行时探测与 ready

TauriTavern 会在宿主层安装稳定 ABI：

- `window.__TAURITAVERN__`
- `window.__TAURITAVERN_MAIN_READY__ : Promise<void>`

扩展侧推荐：

```js
await (window.__TAURITAVERN__?.ready ?? window.__TAURITAVERN_MAIN_READY__);
```

## 3. TauriTavern 专属 API（唯一入口）

为避免与上游 SillyTavern 的 `getContext()` 语义混淆，TauriTavern 的扩展专属 API **不做 alias**，唯一入口是：

- `window.__TAURITAVERN__.api.chat`

它提供与“记忆/数据库类扩展”强相关的能力：

- 只读摘要与稳定 ID：`summary()` / `stableId()`
- 历史按需读取：`history.tail/before/beforePages`
- 后端定位：`locate.findLastMessage(...)`
- 状态持久化（每 chat）：`metadata.*` / `store.*`
- 纯文本检索（不向量，CJK 优化）：`searchMessages(...)`

API 参考：

- `docs/API/Chat.md`

适配指南（如何把 `getContext().chat` 扫描迁移到后端分页/定位/检索）：

- `docs/API/Migration.md`

## 4. 移动端（Android/iOS）性能建议

- 永远不要假设 `getContext().chat` 是全量数组；在 windowed 模式下它只代表最近窗口。
- 检索/定位尽量交给 `window.__TAURITAVERN__.api.chat`，并使用 `scanLimit` 控制“向前回溯”的上限。
- 大状态不要塞进消息体；优先 `store.setJson()`，小状态优先 `metadata.setExtension()`。

