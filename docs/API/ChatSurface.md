# ChatSurface Participant Project Contract v1

ChatSurface 只建立一个边界：完整 `chat[]` 是数据事实，`#chat > .mes` 是可丢弃的有界投影。扩展在消息、内容或重型 runtime 存活期间持有的资源，必须在相应寿命结束时同步释放。投影变化不是消息业务变化，因此不会伪造 SillyTavern 事件。

## Host API

```js
window.__TAURITAVERN__.api.chatSurface = {
    protocolVersion: 1,
    isManagedOwnershipRequired(),
    registerParticipant(definition),
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
