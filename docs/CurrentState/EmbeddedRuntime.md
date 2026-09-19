# 嵌入式运行时维护指南

Embedded Runtime（ER）按可见性和资源预算管理静态聊天中的 iframe 页面，目前适配 JS-Slash-Runner（JSR）和 LittleWhiteBox（LWB）。开启 `chat_virtualization_enabled` 时，聊天页面改由 [ChatSurface participant](../API/ChatSurface.md) 管理。

## 配置与启用

配置项为 `tauritavern-settings.embedded_runtime_profile`：

| 值 | 行为 |
| --- | --- |
| `off` | 关闭 ER，由扩展管理 iframe |
| `auto` | 桌面使用 `compat`，移动设备使用 `mobile-safe` |
| `compat` | 使用较宽松的驻留预算 |
| `mobile-safe` | 使用较小的驻留预算，并扩大视口附近的预加载范围 |

预算参数定义在 [embedded-runtime-profiles.js](../../src/tauri/main/services/embedded-runtime/embedded-runtime-profiles.js)，包括 slot 数量、iframe 数量、权重上限，以及软停车的容量和保留时间。

启动时，bootstrap 读取 `localStorage` 中的 `tt:embeddedRuntimeProfile` 镜像；服务就绪后读取正式配置、创建 manager，并在 `APP_READY` 时安装聊天适配器。

## 运行流程

### 发现与同步

扩展生成的 wrapper 是 slot 的 `host`：JSR 使用 `.TH-render`，LWB 使用 `.xiaobaix-iframe-wrapper`。适配器根据消息编号、源码签名和块序号生成稳定的 slotId。

聊天事件和 DOM 观察器负责发现渲染块。首次发现时注册 slot；已有 slot 的 iframe 被替换，或 `src/srcdoc` 发生变化时，调用 `manager.invalidate(id)`，同步 `refresh()` 当前来源并安排调度。

用户点击暂停占位时，`manager.touch(id)` 提高该 slot 的交互优先级。普通来源更新只同步资源，不改变优先级。

### 调度与占位

manager 根据可见性、优先级和预算选择驻留页面。`isResident()` 返回实际驻留情况，`canHydrate()` 表示当前是否可以恢复。`appliedState` 记录上次应用的调度结果，其中 `cold` 表示首次应用或需要重新应用。

源码未就绪或读取失败时，现有页面继续驻留并计入预算。已经移除的页面在具备恢复条件后参与名额竞争。manager 先回收落选页面，再按实际驻留量分配剩余容量。

| 情况 | 页面表现 |
| --- | --- |
| 超出预算 | `.tt-runtime-placeholder` 保留高度，支持点击恢复 |
| 离开可见范围 | `.tt-runtime-ghost` 保留高度 |
| 页面已移除且源码不可用 | 当前块显示重新打开聊天的提示 |

`hydrate()` 和 `dehydrate()` 同步执行 DOM 操作。软停车池按 profile 暂存可复用的 iframe 元素；`dispose()` 释放该 slot 持有的元素、源码缓存和 URL。

### 源码与冷恢复

Blob 页面在注册时开始读取原始源码。读取结果分为 `pending`、`ready`、`failed`；完成后通过 `onSourceSettled` 使当前 slot 失效，再由 manager 调度恢复或回收。

恢复使用原 renderer 元素及其加载监听，Blob 地址由 slot 自己持有，并在同一来源内复用。来源替换或 slot 销毁时释放该地址，旧来源的读取结果也随之失效。

冷恢复会重新加载当前页面，缓存保存的是顶层源码，依赖资源仍由渲染器管理。恢复过程只处理当前 slot，不发送 `MESSAGE_UPDATED`，同楼其他页面不随这次恢复一起重建。

上游移除 iframe 后，聊天适配器会检查 wrapper。若 JSR 的源码块或折叠按钮仍被隐藏，适配器还原对应的源码 UI 并安排局部恢复；其余情况注销 slot。

## 代码入口

| 维护内容 | 入口 |
| --- | --- |
| 启动装配 | [install.js](../../src/tauri/main/services/embedded-runtime/install.js) |
| 配置读取与启动镜像 | [embedded-runtime-profile-state.js](../../src/tauri/main/services/embedded-runtime/embedded-runtime-profile-state.js) |
| 聊天事件、DOM 变化与点击处理 | [chat-embedded-runtime-adapter.js](../../src/tauri/main/adapters/embedded-runtime/chat-embedded-runtime-adapter.js) |
| 渲染器识别与注册 | [JSR adapter](../../src/tauri/main/adapters/embedded-runtime/js-slash-runner-runtime-adapter.js)、[LWB adapter](../../src/tauri/main/adapters/embedded-runtime/littlewhitebox-runtime-adapter.js) |
| 预算与调度 | [embedded-runtime-manager.js](../../src/tauri/main/services/embedded-runtime/embedded-runtime-manager.js) |
| 源码、恢复与回收 | [managed-iframe-slot.js](../../src/tauri/main/adapters/embedded-runtime/managed-iframe-slot.js) |
| 软停车池 | [managed-iframe-parking-lot.js](../../src/tauri/main/adapters/embedded-runtime/managed-iframe-parking-lot.js) |
| slot 接口定义 | [types.js](../../src/tauri/main/services/embedded-runtime/types.js) |

消息 HTML 写入使用 [mes-text-write.js](../../src/scripts/tauri/message/mes-text-write.js) 中的 `replaceMesTextHtmlWithRuntimePolicy()`，其中的 wrapper 复用由 [message-render-transaction.js](../../src/tauri/main/adapters/embedded-runtime/message-render-transaction.js) 处理。聊天打开时的源码遮罩由 [frontend-source-handoff.js](../../src/tauri/main/adapters/chat-surface/frontend-source-handoff.js) 管理。

## 调试与验证

ER 启用后，可在控制台查看驻留量和调度计数：

```js
globalThis.__TAURITAVERN_EMBEDDED_RUNTIME__.getPerfSnapshot();
```

`Ctrl+Alt+P` 打开性能 HUD，其中的 `Runtime:` 行使用同一份快照。定位单个渲染块时，可查看 host 上的 `data-tt-runtime-slot-id`。

在仓库根目录运行 `pnpm run test:contracts` 执行前端契约测试，`pnpm run check` 执行完整检查。

浏览器验收使用同一目录启动本地服务：

```sh
python3 -m http.server 8000 --bind 127.0.0.1
```

打开[冷恢复验收页面](http://127.0.0.1:8000/tests/browser/embedded-runtime-recovery.html)，各项应显示 `PASS`。页面通过真实浏览器检查 JSR/LWB 的冷恢复，以及未就绪或失败候选竞争预算时，健康页面的 Document、表单值和加载次数是否保持不变。
