# 性能基线

`pnpm run check` 在 Linux 上计量一组固定场景里前端与 Rust 后端各做了多少工作，并与记录比较。计的是工作量（样式重算、布局、宿主流量、内存分配、指令数与 I/O），不是耗时；计数设在生产代码绕不开的边界上：渲染引擎、宿主调用、JS 堆、分配器与进程。代码在 `tests/perf/` 与 host crate 的 `src/perf.rs`、`src/perf/`。

## 比较方式

`pnpm run test:perf` 先跑前端层，再在后端层重放前端的宿主调用。两层的计数在 Linux 上精确，不随机器快慢变化：前端的帧由测量页发出，后端的读写取自内核计数，指令数取自 callgrind。

| 平台 | 比较 |
| --- | --- |
| Linux | 与 `tests/perf/baseline.json` 比较，任一计数偏离超过 10%（至少 2）即失败，变少也算。基线写明 Chromium 与 Rust 版本，测量用这个 Rust 版本构建 |
| macOS、Windows | 计数只是近似，`pnpm run check` 不比较。需要时运行 `pnpm run test:perf -- --head`：`src`、`src-tauri`、`tests/perf` 有未提交改动时，与 HEAD 在本机的测量比较，计数增长超过余量即失败。HEAD 在 `src-tauri/target/perf-head/` 的工作树里测量，结果按提交、平台与 Rust 版本缓存 |

CI 只比较 Linux。

## 失败时

先确认变化来自本次改动。有意的变化在 Linux 上用 `pnpm run test:perf -- --update` 重新记录；不在 Linux 上开发时，Linux CI 失败日志末尾给出完整测量，可直接提交为 `tests/perf/baseline.json`。基线随提交评审。升级 `playwright`（即更换 Chromium）或 Rust 版本后也要重新记录。

记录机器的字体须与 CI 相同：CI 没有 Noto Sans，应用字体与 `sans-serif` 都落到 DejaVu Sans。装有 Noto Sans 的 Linux 用排除该字族的 `FONTCONFIG_FILE`，并设 `LANG=C.UTF-8`，否则布局计数不同。

## 场景与数据

场景是 `tests/perf/*.perf.mjs`，两层共用 `dataset.mjs` 的数据，规模取自真实用户库：

- 聊天：80 条消息；一条短回复，和一条约 4 千字、带思考过程的长回复。
- 世界书：200 条，第一条 10 万字。
- 重卡：内嵌世界书约 200 万字，聊天末条约 15 万字。
- 前端卡：按生态卡的常见结构合成，回复含思考、正文、变量更新与状态栏。最近两层的状态栏约 80 KB，由 Code Render 渲染成 iframe；iframe 与页面同在一个渲染线程，其工作一并计入。
- 应用内助手：一个会话，440 条消息、约 2 MB 历史。

启动（`app/startup`）是同一用户的第二次启动：页面先加载一次，第二次启动看到第一次留下的宿主状态与浏览器存储；后端同样先重放第一次启动，再计量第二次。

改数据或 fixture 时单独提交，并重新记录基线。

新增场景时导出 `async (app) => {}`，用 `app.measure(name, ...inputs)` 计量一组输入。每个输入之后页面先静默下来，所以输入的划分决定哪些工作合并：流式回复每次推送的文本是一个输入（经 `__perfReply` 的 `push()`、`think()`、`end()` 送入），连续打字放进同一个输入，用 `app.frame()` 隔开。点击坐标在步骤之前用 `app.centerOf(locator)` 取得，步骤里用 `page.mouse`、`page.keyboard` 发送真实输入。依赖配置才会发生的工作（如卡的正则、Code Render），在步骤之后核对产物。写完后在 Linux 上用 6 倍 CPU 限速确认计数不变。

## 前端层

在 Playwright 固定版本的 Chromium headless shell 里，经 Web Host 运行构建后的前端，宿主身份为 Windows 桌面。测量页的假后端遵守宿主契约，例如角色列表只给浅条目；Web Host 没有的原生命令（分词、助手会话、更新检查）按原生的结果应答。首次运行前安装浏览器：`pnpm exec playwright install --only-shell chromium`。

每个步骤计：

- 事件派发，不含次数由引擎调度决定的事件（生命周期、内容可见性、滚动结束、静止指针的边界事件）。
- 样式重算与布局的次数、其中强制的次数与涉及的元素数；有渲染工作的帧数与合成更新。
- 宿主流量：`core.invoke` 参数与结果的字节数。
- 控制台消息条数。
- JS 堆分配：在另一个会话里测，不含测量页、Web Host 与 fixture 自身的分配。
- 步骤结束后的页面规模：请求、元素、可见滚动容器、存活节点、布局对象与事件监听器。

启动的工作取决于资源何时加载完，所以启动只记页面规模。

一个步骤引起的工作都落在这个步骤里，靠以下约定：

- 动画关闭；页面语言与时区固定为 en-US、UTC；Linux 上关闭字形 hinting，字宽与 macOS 一致；分配会话里 V8 单线程运行。
- 步骤进行中，10 秒以内的定时器、网络响应与步骤开启的 interval 只在页面静默时按到期顺序放行，`performance.now()` 与 `Date.now()` 只随放行推进。Linux 上帧也由测量页在静默后逐帧发出。步骤之外一切按真实时间。
- 页面请求的帧回调与空闲回调执行完之前，页面不算静默。
- 步骤结束后再静默一次，不能再有定时器、回调或宿主调用运行，也不能留下超过 10 秒的定时器或仍在运行的 interval。页面报错、请求失败或宿主拒绝调用同样判为失败。

## 后端层

前端层按顺序记录每次 `core.invoke` 的命令、参数与所属步骤，与数据一起写入 `tests/perf/out/`。`tt-perf`（`perf` feature，`perf` profile）在 Tauri 的 mock runtime 上用真实的命令与服务重放这些调用：

- 每次运行从一份新复制的数据开始：`tt-perf prepare` 在后端启动后把数据文件的修改时间设为同一时刻，再重放第一次启动；`tt-perf measure` 在同一目录里计量其余部分。缓存记录了文件的变更时间，所以中途不再复制。
- `app/startup` 从后端启动之前开始计，包括启动记录、启动配置、应用状态与默认内容的初始化。
- 宿主签发的值（会话 id、修订号、读取器）取自重放时的实际结果。插件、网络、通知与回调页面的命令不重放。

每个步骤都计结果字节与分配。tt-perf 运行在自己的全局分配器上（`src/perf/allocator.rs`），系统分配器包在几个固定名字的函数后，由它们记下分配与重新分配的次数和请求的字节数。此外：

| 平台 | 计数 |
| --- | --- |
| Linux | `/proc/self/io` 的读写字节与次数；callgrind 指令数，不含分配器内部 |
| macOS | 进程指令数 |
| Windows | I/O 字节与次数 |

分配与系统计数取自原生重放：Linux 的读写精确，重放一次；macOS 与 Windows 的计数还含系统替进程做的工作，取三次运行的最小值。Linux 的指令数另由 callgrind 重放一次。

分配器内部的工作取决于此前整个进程留下的堆，而不是当前步骤：线程交错与哈希种子每次重放都不同，前面步骤的改动也会把整理空闲内存的开销移到后面的步骤。所以指令数不含这些函数内部，步骤向分配器要了什么由分配次数与字节数计。

新步骤用到还没重放过的命令时，重放会报出命令名。把它加入 `src/perf/environment.rs` 的命令表；接收 `Webview` 的命令写成对 `R: Runtime` 泛型。

## 盲区

- macOS 与 Windows 上帧由引擎按自己的时钟渲染；同机比较超出余量时，以 Linux 的结果为准。
- JS 分配不含 ArrayBuffer 的外部内存、Blink 创建的字符串与 DOM 对象。
- 测量页不接管 Worker 与 iframe 内的定时器和动画，经它们延后的工作不计入。
- JS-Slash-Runner 不在测量内。
- 后端不计插件命令（如经 `plugin:fs` 读取聊天文件）与网络。
- 系统库自己的分配（如打开目录时的缓冲）不经 tt-perf 的分配器，不计入分配，其分配器开销仍在 Linux 的指令数里。
- 启动时第一次目录扫描会查询 CPU 配额，Linux 上要读 cgroup 的 `cpu.max`，`app/startup` 的读次数因此随机器的 cgroup 层级差几次。
