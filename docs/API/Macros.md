# 宏求值

`SillyTavern.getContext().macros` 提供宏注册、引擎和独立求值接口。

普通 `substituteParams()` 读取当前聊天、角色和变量，本文称之为 live 环境。需要按一份固定输入求值时，可以先用 `captureContext()` 保存输入，再调用 `evaluateWithContext()`。这些方法都同步执行，求值时不会切换当前聊天或预设。

```js
const { macros } = SillyTavern.getContext();
const context = macros.captureContext();

macros.evaluateWithContext('{{setvar::topic::调查}}', context);
const text = macros.evaluateWithContext('{{getvar::topic}} / {{char}}', context);
// topic 写入 context.variables.local，当前聊天变量不变。

const other = structuredClone(context); // 从当前结果另开一个独立分支
```

## 捕获与求值

`captureContext()` 保存当时的名称、原始角色模板、聊天数据、设置、时间、local/global 变量原值及模块数据。捕获本身不展开角色模板，也不调用已注册的宏 handler。

`evaluateWithContext(text, context, options?)` 返回展开后的字符串。`text` 必须是字符串；传入空字符串时直接返回，不执行角色卡中的宏。`context` 可以使用捕获结果，也可以由调用者按 [MacroContext 类型](../../src/scripts/macros/engine/MacroEnv.types.js) 提供完整数据。

`options` 与 `substituteParams()` 对应：

- `original`：本段文本的原始内容，可由 `{{original}}` 取用一次。
- `dynamicMacros`：本次调用使用的动态宏。
- `postProcessFn`：宏结果的后处理函数。
- `name1Override`、`name2Override`、`groupOverride`：用户、角色和群组的名称覆盖。
- `replaceCharacterCard`：是否展开角色卡字段。

context 记录捕获时选择的新或旧引擎。两种引擎各自的语法和求值顺序保持不变，之后切换 UI 开关不会改变这份 context 的引擎。

直接使用低层接口的扩展仍可调用 `macros.envBuilder.buildFromRawEnv()`，再交给 `macros.engine.evaluate()`。builder 默认按新引擎准备环境；普通 `substituteParams()` 和 `captureContext()` 使用当前 UI 选择的引擎。

## 上下文与运行环境

MacroContext 是调用者持有、可以保存的数据；MacroEnv 是处理一段文本时传给宏 handler 的运行环境。

对同一个 context 连续求值，会共享变量和禁用词列表 `bannedWords`。每段文本分别记录自己的内容和 hash，`original` 的使用次数也重新计算。角色字段在首次读取时展开，结果只在这一段文本内缓存。需要用同一份输入分别组装多次时，在每次组装开始前复制 context。

handler 内的 `resolve()` 处理嵌套内容，继续使用当前 MacroEnv。处理另一段文本时，使用 `evaluateWithContext()`，或调用 `env.functions.substitute()` 沿用当前输入和变量。

变量操作由 `env.variables.local` 和 `env.variables.global` 提供，包括 `get`、`set`、`add`、`inc`、`dec`、`has`、`del`。普通 live 变量 API 每次操作都读取当前存储，因此扩展整体替换变量对象后，后续宏能读到新对象。

live 环境中的聊天数据和运行状态是只读的，每次读取都取当前值。`captureContext()` 会把这些值复制为普通数据。名称、model 覆盖、`extra` 容器及每段文本的状态仍按调用准备。live 角色字段使用现有的惰性展开函数；显式上下文从保存的模板展开，两种路径使用相同的宏定义和变量规则。

## 扩展宏

需要支持独立求值的宏，应从 handler 收到的 `env` 读取上下文。扩展可以注册 provider，将自身数据放入 `env.extra`；这些数据需要能被 `structuredClone` 复制。下面的 `extensionSettings` 表示扩展自己的配置对象：

```js
const liveData = Object.freeze({
    get label() { return extensionSettings.label; },
});
macros.envBuilder.registerProvider(env => {
    env.extra.myExtension = liveData;
});
macros.register('myLabel', {
    handler: ({ env }) => env.extra.myExtension?.label ?? '',
});
```

provider 在准备 live 环境和捕获 context 时运行。它可以提供普通值，也可以提供可枚举的 getter：live 求值按需调用 getter，捕获时则保存 getter 返回的值。上例的只读对象不保存求值状态，可以在调用间共享；只属于某段文本的状态应放在该次 env 中。

`evaluateWithContext()` 不重新运行 provider。例如，provider 在捕获时设置的角色昵称会随 context 保存，之后切换聊天不会改变它。Session 使用自己的输入构造上下文，不调用当前聊天的 provider。

现有 registry、processor、dynamic 宏和 legacy 注册接口继续可用。`MacrosParser.registerMacro()` 的回调仍以 nonce 为第一个参数，第二个参数为 MacroEnv；在新引擎中调用 legacy 回调时，也会传入这两个参数。

扩展回调仍按普通 JavaScript 执行。回调直接访问全局变量、操作 DOM，或调用普通 `substituteParams()` 时，访问的仍是 live 环境。第一方宏及其下游函数从 `env` 读取数据；第三方回调的全局读写不会自动转移到 context。

宏的错误展示与恢复方式沿用现有行为。显式求值缺少必要上下文时会报错，不从当前聊天补齐。

## Agent 与请求模板

Chat Agent 在 `GENERATE_AFTER_DATA` 的异步处理完成后、dry-run 返回前捕获输入。首次独立预设组装，以及后续子 Agent 和交接，都使用各自的工作副本。`CHAT_COMPLETION_SETTINGS_READY` 可以修改最终请求，在此事件中写入的 live 变量不会进入已保存的输入。组装流程见 [Prompt assembly](../Agent/PromptAssembly.md)。

Additional Parameters 模板原样保存在对应 source 的配置中，准备请求时展开。生成、连接状态检查（status）和自定义图片描述（Custom Caption）使用同一个请求准备函数。status 只展开 headers；Caption 使用 custom 渠道配置和本次图片描述的模型。

同次组装中的提示词、stop、prefill 和 Additional Parameters 共享工作变量。请求重试和后续模型轮次复用已准备的请求，不重复执行模板中的宏。

原生 LLM Connection 的 `adapterHints`、扩展自行构造的 payload，以及扩展在准备完成后覆盖的参数，均按字面值使用。Rust 只做已有的只读文本替换，不执行前端宏引擎。
