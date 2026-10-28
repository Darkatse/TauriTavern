# 性能基线

`pnpm run check` 在一组固定场景里计量前端与 Rust 后端各做了多少工作，并与记录比较。计的是工作量，例如样式重算、布局、宿主流量、内存分配、指令数与 I/O，不是耗时，所以结果不随机器快慢变化。计数只设在生产代码绕不开的边界（渲染引擎、宿主调用、JS 堆、进程），不针对具体函数。代码在 `tests/perf/`，以及 host crate 的 `src/perf.rs` 与 `src/perf/`。

## 失败时

- `pnpm run test:perf`：前端计数偏离 `tests/perf/baseline.json`。
- `pnpm run test:perf:backend`：后端计数偏离 Linux 基线，或在 macOS、Windows 上超过 HEAD。它重放前一步留在 `tests/perf/out/` 的调用，所以要先跑前端层。

任一计数偏离超过 10%（至少 2）即失败；与基线比较时，变少也算。先确认变化来自本次改动。有意的变化用 `-- --update` 重新记录，基线随提交评审。升级 `playwright-core`（换了 Chromium）或 Rust 版本后，也要重新记录。

后端基线只能在装有 valgrind 的 Linux 上记录。在其他系统开发时，CI 失败日志末尾会给出这次的完整计数，可以直接提交为 `tests/perf/backend-baseline.json`。

## 场景与数据

场景是 `tests/perf/*.perf.mjs`，两层共用 `dataset.mjs` 的数据：80 条消息的聊天；200 条世界书，第一条 10 万字；一张重卡，内嵌世界书约 200 万字，聊天末条约 15 万字。数据规模取自真实用户库。改数据或 fixture 时单独提交，并重新记录基线。

新增场景时默认导出 `async (app) => {}`，用 `app.measure(name, ...inputs)` 计量一组输入。每个输入之后页面先静默下来，再进行下一个，所以输入的划分决定哪些工作会合并：回复的每个 token 是一个输入（经 `__perfReply.push()` 与 `end()` 送入）；连续打字放进同一个输入，用帧隔开。点击坐标在步骤之前用 `app.centerOf(locator)` 取得，步骤里用 `page.mouse` 与 `page.keyboard` 发送真实输入。写完后在 6 倍 CPU 限速（`Emulation.setCPUThrottlingRate`）下确认计数仍在余量之内。

## 前端层

在 Playwright 固定版本的 Chromium headless shell 里，经 Web Host 运行构建后的前端，宿主身份为 Windows 桌面。测量页的假后端遵守宿主契约，例如角色列表只给浅条目、`simple` 只返回文件名。首次运行前安装浏览器：`pnpm exec playwright-core install --only-shell chromium`。

每个步骤计：

- 事件派发；样式重算与布局的次数、其中强制的次数和涉及的元素数；有渲染工作的帧数与合成更新。
- 宿主流量：`core.invoke` 参数与结果的字节数。
- JS 堆分配：在第二个会话里测，测量页、Web Host 与 fixture 自身的分配不计。
- 步骤结束后的页面规模：请求、元素、可见滚动容器、存活节点、布局对象与事件监听器。

计数稳定靠以下约定：动画关闭；短于一帧的定时器在下一帧之前执行；步骤进行中，10 秒以内的定时器和网络响应先扣住，等页面静默后再放行；分配会话里 V8 单线程运行。页面报错或请求失败时直接判为失败。

## 后端层

前端套件运行时按顺序记录每次 `core.invoke` 的命令、参数和所属步骤，连同数据写入 `tests/perf/out/`。`tt-perf`（`perf` feature，`perf` profile）在 Tauri 的 mock runtime 上调用真实的命令与服务，在数据副本上重放这些调用。宿主签发的值（会话 id、修订号）从重放时的实际结果里取。插件、网络、通知与回调页面的命令不重放。重放只用一个 async worker 和一个 blocking 线程，按线程划分的缓存每次都以同样的方式预热。

每个步骤都计结果字节，此外：

| 平台 | 计数 | 比较 |
| --- | --- | --- |
| Linux | callgrind 指令数；`/proc/self/io` 的读写字节与次数 | 与 `backend-baseline.json` 比较。基线写明 Rust 版本，测量固定用这个版本构建 |
| macOS | 进程指令数 | 本机与 HEAD 比较，取三次运行的最小值 |
| Windows | I/O 字节与次数 | 同 macOS |

macOS 与 Windows 只在 `src`、`src-tauri`、`tests/perf` 有未提交改动时比较：在 `src-tauri/target/perf-base/` 的工作树里测量 HEAD，结果按提交与 Rust 版本缓存，工作区增长超过余量即失败。有意的增长提交之后就成为新的 HEAD。CI 只比较 Linux。

新步骤用到还没重放过的命令时，重放会报出命令名。把它加入 `src/perf/environment.rs` 的命令表；接收 `Webview` 的命令写成对 `R: Runtime` 泛型。

## 盲区

- JS 分配不含 ArrayBuffer 的外部内存、Blink 创建的字符串与 DOM 对象。
- 后端不计插件命令（如经 `plugin:fs` 读取聊天文件）与网络。
