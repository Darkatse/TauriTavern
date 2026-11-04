# 性能基线

`pnpm run check` 在一组固定场景里计量前端与 Rust 后端各做了多少工作，并与记录比较。计的是工作量，例如样式重算、布局、宿主流量、内存分配、指令数与 I/O，不是耗时，所以结果不随机器快慢变化。计数只设在生产代码绕不开的边界（渲染引擎、宿主调用、JS 堆、进程），不针对具体函数。代码在 `tests/perf/`，以及 host crate 的 `src/perf.rs` 与 `src/perf/`。

## 比较方式

`pnpm run test:perf` 先测量两层（`measure.mjs`）：构建前端，跑前端层，再在后端层重放前端的宿主调用。两层的计数只在 Linux 上精确：前端的帧由测量页发出（macOS 上的无头 Chromium 做不到），后端在 callgrind 下运行。比较方式由此而来：

| 平台 | 比较 |
| --- | --- |
| Linux | 与 `tests/perf/baseline.json` 比较。任一计数偏离超过 10%（至少 2）即失败，变少也算，所以每一次性能变化都是有意记录的。基线写明 Chromium 与 Rust 版本，测量固定用这个 Rust 版本构建 |
| macOS、Windows | 只在 `src`、`src-tauri`、`tests/perf` 有未提交改动时比较：HEAD 在 `src-tauri/target/perf-head/` 的工作树里测量自己，结果按提交、平台与 Rust 版本缓存；工作区的计数增长超过余量即失败 |

CI 只比较 Linux。

## 失败时

先确认变化来自本次改动。有意的变化在 Linux 上用 `pnpm run test:perf -- --update` 重新记录；在其他系统开发时，Linux CI 失败日志末尾会给出这次的完整测量，直接提交为 `tests/perf/baseline.json`。基线随提交评审。在 macOS、Windows 上，有意的增长提交之后就成为新的 HEAD。升级 `playwright-core`（换了 Chromium）或 Rust 版本后，也要重新记录。

## 场景与数据

场景是 `tests/perf/*.perf.mjs`，两层共用 `dataset.mjs` 的数据：80 条消息的聊天；200 条世界书，第一条 10 万字；一张重卡，内嵌世界书约 200 万字，聊天末条约 15 万字。数据规模取自真实用户库。改数据或 fixture 时单独提交，并重新记录基线。

新增场景时默认导出 `async (app) => {}`，用 `app.measure(name, ...inputs)` 计量一组输入。每个输入之后页面先静默下来，再进行下一个，所以输入的划分决定哪些工作会合并：回复的每个 token 是一个输入（经 `__perfReply.push()` 与 `end()` 送入）；连续打字放进同一个输入，用 `app.frame()` 隔开。点击坐标在步骤之前用 `app.centerOf(locator)` 取得，步骤里用 `page.mouse` 与 `page.keyboard` 发送真实输入。写完后在 Linux 上用 6 倍 CPU 限速（`Emulation.setCPUThrottlingRate`）确认计数不变。

## 前端层

在 Playwright 固定版本的 Chromium headless shell 里，经 Web Host 运行构建后的前端，宿主身份为 Windows 桌面。测量页的假后端遵守宿主契约，例如角色列表只给浅条目、`simple` 只返回文件名。首次运行前安装浏览器：`pnpm exec playwright-core install --only-shell chromium`。

每个步骤计：

- 事件派发：不计引擎按自身调度触发的事件，如文档与资源的生命周期事件、内容可见性与滚动结束、指针静止时命中测试引起的边界事件，它们的次数取决于调度而不是页面的工作。
- 样式重算与布局的次数、其中强制的次数和涉及的元素数；有渲染工作的帧数与合成更新。
- 宿主流量：`core.invoke` 参数与结果的字节数。
- JS 堆分配：在第二个会话里测，测量页、Web Host 与 fixture 自身的分配不计。
- 步骤结束后的页面规模：请求、元素、可见滚动容器、存活节点、布局对象与事件监听器。

计数稳定靠以下约定：

- 动画关闭；分配会话里 V8 单线程运行。
- 页面语言固定为 en-US、时区固定为 UTC，不取本机设置；界面按它们翻译文字、格式化日期。
- Linux 上关闭字形 hinting：无头 Chromium 默认对 web font 做完整 hinting，把字宽取整，折行和点击落点都会与 macOS 不同。
- 步骤进行中，页面安排的后续工作只在页面静默时由测量页放行：10 秒以内的定时器和网络响应按到期顺序放行，步骤开启的 interval 每次静默时走一拍；Linux 上测量页还发出每一帧（`--deterministic-mode`），页面静默后出一帧，直到某一帧不再绘制。`performance.now()` 与 `Date.now()` 读一个只随这些放行推进的时钟，按耗时切分的工作（如 CodeMirror 的增量解析）因此不随机器快慢分片。步骤之前已有的定时器等步骤结束再运行；步骤之外，定时器、时钟与帧都按真实时间。
- 短于一帧的定时器在下一帧之前执行。
- 页面请求的帧回调与空闲回调未执行完之前，页面不算静默。

所以一个步骤引起的工作都落在这个步骤里，每次运行的宿主调用归属相同，后端重放的输入因此可以复现；在 Linux 上，计数也不随机器快慢改变。

每个步骤结束时还要验证两件事：再静默一次不能再有任何定时器、回调或宿主调用运行；步骤安排的工作不能留到结束之后，例如超过 10 秒的定时器或仍在运行的 interval。把工作推迟到步骤之外要么被计入步骤，要么让运行失败，不能借此压低计数。页面报错或请求失败时同样直接判为失败。

## 后端层

前端套件运行时按顺序记录每次 `core.invoke` 的命令、参数和所属步骤，连同数据写入 `tests/perf/out/`。`tt-perf`（`perf` feature，`perf` profile）在 Tauri 的 mock runtime 上调用真实的命令与服务，在数据副本上重放这些调用。宿主签发的值（会话 id、修订号）从重放时的实际结果里取。插件、网络、通知与回调页面的命令不重放。重放只用一个 async worker 和一个 blocking 线程，按线程划分的缓存每次都以同样的方式预热。

每个步骤都计结果字节，此外：

| 平台 | 计数 |
| --- | --- |
| Linux | callgrind 指令数；`/proc/self/io` 的读写字节与次数 |
| macOS | 进程指令数 |
| Windows | I/O 字节与次数 |

macOS 与 Windows 的计数还包含系统替进程做的工作，只增不减，所以取三次运行的最小值。每棵源码树构建到自己的 target 目录：Cargo 给同一工作区的产物取的名字与检出位置无关，并按文件时间判断新旧，共用 target 目录时两个检出会互相用到对方的构建。

新步骤用到还没重放过的命令时，重放会报出命令名。把它加入 `src/perf/environment.rs` 的命令表；接收 `Webview` 的命令写成对 `R: Runtime` 泛型。

## 盲区

- macOS 与 Windows 上帧仍由引擎按自己的时钟渲染，帧与任务的先后可能随机器负载变化；同机比较超出余量时，以 Linux 的结果为准。
- JS 分配不含 ArrayBuffer 的外部内存、Blink 创建的字符串与 DOM 对象。
- 测量页不接管 Worker 内的定时器；经它延后很久的工作不会被计入，也不会被步骤结束时的检查发现。
- 后端不计插件命令（如经 `plugin:fs` 读取聊天文件）与网络。
