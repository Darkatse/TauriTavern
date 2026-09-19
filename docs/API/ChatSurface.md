# ChatSurface Project Contract v1

ChatSurface 只建立一个边界：完整 `chat[]` 是数据事实，`#chat > .mes` 是可丢弃的有界投影。扩展在消息、内容或重型 runtime 存活期间持有的资源，必须在相应寿命结束时同步释放。投影变化不是消息业务变化，因此不会伪造 SillyTavern 事件。

## Host API

```js
window.__TAURITAVERN__.api.chatSurface = {
    protocolVersion: 1,
    isManagedOwnershipRequired(),
    registerParticipant(definition),
    registerContentProcessor(definition),
};
```

TauriTavern 不提供第二套 kit 或 wrapper。扩展直接 feature-detect raw API，从而仍可在上游 SillyTavern 中加载：

```js
const api = window.__TAURITAVERN__?.api?.chatSurface;
const managed = api?.isManagedOwnershipRequired?.() === true;

if (!managed) {
    startLegacyRenderer();
}

const registration = managed ? api.registerParticipant({
    id: 'my-extension/frontend-runtime',
    protocolVersion: api.protocolVersion,

    prepareContent({ content }, claims) {
        rewriteMacros(content);
        for (const source of content.querySelectorAll('[data-my-runtime]')) {
            claims.claim(source, ({ element, mesid, source, signal }) => {
                const runtime = mountRuntime({ element, mesid, source, signal });
                return () => runtime.dispose();
            });
        }
    },

    didMount({ element, signal }) {
        const observer = observeMessage(element);
        return () => observer.disconnect();
    },

    didCommitContent({ content, signal }) {
        return decorateContent(content, signal);
    },
}) : null;

registration?.fault(error); // participant 已无法继续履约
```

`isManagedOwnershipRequired()` 是本页启动时冻结的决策：

- `true`：只启用 participant owner，不再启动 legacy DOM watcher；
- `false` 或 API 不存在：保留上游 static renderer。

扩展不能从当前 DOM 形状推断 ownership。所有 participant 必须在第一次 projection 前完成注册；本页不支持热注册或热注销。

## 异步内容预处理

异步模板通过 `registerContentProcessor()` 计算显示 HTML，在 participant 申领 runtime source 之前完成。只处理内容的扩展无需注册 participant，也不参与通用代码块 renderer 的互斥判断。

扩展在 manifest 中声明独立的启动 hook：

```json
{ "hooks": { "chatSurface": "registerChatSurface" } }
```

bounded 启动会提前激活这些扩展，按现有 manifest 顺序等待 hook 完成，再允许首次聊天投影。hook 必须是 JS 入口的真实命名导出；初始化失败会阻止启动，不使用普通 activation 的诊断超时继续执行。上游 SillyTavern 忽略此 hook，扩展仍需保留自己的 static 路径。

```js
let registration;

export async function registerChatSurface() {
    await initializeTemplateData();
    registration = window.__TAURITAVERN__.api.chatSurface.registerContentProcessor({
        id: 'my-extension/message-content',
        async prepare({ message, mesid, signal }, renderBase) {
            await processRawMessage(message, mesid, signal);
            const html = await renderBase();
            return evaluateDisplayTemplate(html, { message, mesid, signal });
        },
    });
}

// 世界书、设置或其他模板依赖发生变化。
await registration.refresh();
```

- `prepare` 接收稳定的消息对象、本次请求的 `mesid` 和 `AbortSignal`，返回 HTML 字符串；不读写 live DOM，也不创建运行中的 iframe、observer 或 timer。
- `renderBase()` 取得后续处理器和宿主格式化的结果；同一次 `prepare` 中重复读取共用同一个 Promise，不会重复执行下游。先注册的处理器包裹后注册的处理器；消息请求串行执行。需要修改原文时，在首次调用 `renderBase()` 前完成；不要在 hook 中调用 `updateMessageBlock()`、重绘或 `refresh()`。
- 显示结果按消息对象保存，包含原文、活动 swipe、显示文本与格式化输入的版本。滚动卸载保留结果，重新挂载只重建 DOM；内容变化与 `refresh()` 使结果失效。世界书和变量依赖由扩展显式刷新，宿主不追踪这些依赖。
- 宿主仅保存 participant 加入 wrapper/runtime 之前的 HTML 字符串，不保存 DOM、不写回 `extra.display_text`，聊天 epoch 结束后释放结果。
- 首次准备或更新等待期间，`.mes_text` 为空且标记 `aria-busy`；`didMount` 仍表达楼层挂载。准备完成后，通过现有内容事务提交，再运行 `prepareContent`、`didCommitContent` 与 runtime admission。
- 编辑与流式中间内容不执行处理器或启动 runtime。核心生成路径在 `MESSAGE_RECEIVED` 处理完成后提交最终内容，再发送 rendered 事件；同步 `addOneMessage()` / `updateMessageBlock()` 仍同步返回，异步显示完成以 `didCommitContent` 为准。
- `refresh()` 只更新已挂载消息的最终内容，保留消息 root 与 mount lease，跳过编辑器和流式中间内容。被跳过的消息在结束编辑或生成时使用新的显示结果。
- 内容被替代、处理中消息被删除或重编号、聊天切换时，signal abort，旧结果不得回写。扩展需要配合取消自己的异步操作；宿主无法回滚模板已经产生的业务副作用。
- 注册必须早于首次投影；重复 id、非法返回值与处理异常立即报错，处理异常带处理器 id 和消息位置并 fault 当前 epoch。`refresh()` 返回的 Promise 等待当前挂载消息完成准备并传播错误。

生成普通 HTML 代码块后，JSR 可继续按 participant v1 申领它们。扩展若有自己的 iframe 功能，也必须输出可申领的惰性 source，并通过现有 participant 的 grant/revoke 生命周期创建和释放 iframe。

## Participant

```ts
interface ChatSurfaceParticipantV1 {
    id: string;
    protocolVersion: 1;
    prepareContent?(context: DetachedContext, claims: RuntimeClaims): void;
    didMount?(context: MountedContext): void | Disposable;
    didCommitContent?(context: MountedContext): void | Disposable;
}
```

### `prepareContent`

`DetachedContext` 只包含 `mesid` 与 detached `.mes_text` `content`。该 hook 可以同步改写内容并声明 runtime source，但不得创建 iframe、timer、observer 或启动异步任务。

```ts
claims.claim(source, activate)
```

- `source` 必须是当前 detached content 的后代；
- 同一 source 只能由一个 participant claim；
- claims 只在 `prepareContent` 同步调用期间有效；
- hook 必须返回 `undefined`，不得返回 Promise；
- source 会保留对象身份并随内容提交到 live DOM。

### `didMount` 与 `didCommitContent`

connected context 包含：

```ts
{ mesid, element, content, signal }
```

hook 可以返回一个 cleanup function 或带 `dispose()` 的对象。宿主先 abort `signal`，再同步调用 disposer：

| hook | 寿命 | 适合持有 |
| --- | --- | --- |
| `didMount` | 整楼 mount → unmount | root observer、楼层按钮、element 引用 |
| `didCommitContent` | 当前内容版本 → 替换或 unmount | 内容 decorator、wrapper/source 引用 |

扩展只有一种 cleanup 交付方式：返回 disposer。内部 lease stack 不属于 Host API。

### Runtime activation

被 claim 的 source 只有获得 runtime grant 后才会 activation：

```ts
activate({ source, mesid, element, content, signal }) => Disposable
```

- activation 时 source 已 connected，且仍属于对应内容；
- activation 必须同步返回 disposer，不得返回 `void` 或 Promise；
- 每次 `grant → revoke → grant` 都获得新的 signal 与 disposer；
- revoke 返回时必须已经撤销 iframe、timer、listener、observer 和强引用；
- active runtime 被 revoke 时，其稳定 host 应留下不持有上述资源的等高 `inert` placeholder；regrant 先把该高度交给新 runtime，再由 renderer 自己的高度协议校准；
- source 是可重建 runtime 的锚点，不得转移给 detached stash 或其他消息。

宿主可以不 activation 某个 candidate。DOM residency 与 runtime admission 是两个独立上限，预算、顺序和 viewport policy 不进入 participant API。

## 错误与事件边界

- registration 字段、协议版本和 hook 参数不合法时立即抛错；
- 必需 renderer 激活失败时向启动调用者传播原始错误；只有激活流程正常返回后才检查 required participant，避免掩盖加载或 hook 的实际失败原因；
- duplicate claim、ownership 分歧、异步 disposer 或同步写重入会 fault 当前有界 epoch；
- participant 在 hook 外无法继续履约时调用 `registration.fault(error)`；
- fault 保留完整 `chat[]` 和当前有界 DOM，不自动展开完整历史；
- mount/remount/content/runtime lifecycle 不发送 `MESSAGE_UPDATED`、`MORE_MESSAGES_LOADED`、`USER_MESSAGE_RENDERED` 或 `CHARACTER_MESSAGE_RENDERED`。

## Capability 与发布边界

已知 renderer 必须在第一次聊天投影前注册精确 identity：

| Extension | required participant id |
| --- | --- |
| JS-Slash-Runner | `js-slash-runner/message-runtime` |
| LittleWhiteBox | `littlewhitebox/message-runtime` |

缺少 required participant、协议不匹配或 participant fault 会拒绝 bounded epoch，不会退回 full DOM。static 策略仍运行原 renderer。

managed 首版只承诺 capability matrix 中已经验证的核心 frontend/iframe runtime；不宣称支持 JSR managed streaming renderer，也不宣称支持 LWB immersive、TTS、draw、story-outline 或 custom-template 等未接入能力。
