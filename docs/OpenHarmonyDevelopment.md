# TauriTavern OpenHarmony 端开发说明

本文档记录 OpenHarmony / HarmonyOS NEXT 端的实验性支持：产物与构建方式、宿主契约、踩过的坑，以及当前限制与验收清单。鸿蒙端复用现有的 Rust 后端与前端，原生宿主来自实验性的 Tauri OpenHarmony 分支及其 fork。

## 1. 定位与产物

- 实验性支持，不进入 Stable 发布。
- Canary Release 提供未签名的 ARM64 HAP，需要用自己的证书和 profile 签名后才能安装。命名与发布规则见 [更新渠道](CurrentState/UpdateChannels.md)。
- x86_64 HAP 用于 x86 主机上的 DevEco 模拟器：手动触发 [`ohos.yml`](../.github/workflows/ohos.yml) 获得，参数见该文件；产物作为 workflow artifact，不进入 Release。
- 工程声明的设备类型为 phone、tablet、2in1。目前只在 x86_64 模拟器（phone）上完成验证；ARM64 真机、平板和 PC 形态尚未验收。
- 鸿蒙 PC 运行的是同一个 HAP，不能使用 Linux 桌面版。

## 2. 框架栈与版本

鸿蒙宿主依赖一组尚未进入上游的实验组件。仓库与提交以钉住文件为准：

| 组件 | 钉住位置 |
| --- | --- |
| Tauri 核心（含原生插件调用桥，基于 tauri-apps/tauri `feat/open-harmony`）、Wry、Tao、openharmony-ability | [`scripts/ohos/tauri-pins.json`](../scripts/ohos/tauri-pins.json) |
| clipboard-manager、dialog、fs、notification、opener、barcode-scanner 插件 | [`scripts/ohos/plugins-pin.json`](../scripts/ohos/plugins-pin.json) |
| ohos-native-bindings（LeenHawk/ohos-native-bindings） | 由 Wry、Ability 的 `Cargo.toml` 间接钉住 |
| SDK 与 `cargo-tauri` CLI | 工具链镜像 [tauri-harmony](https://github.com/LeenHawk/tauri-harmony)，digest 见 [`ohos.yml`](../.github/workflows/ohos.yml) |

openharmony-ability 的 Rust crate 与 ArkTS HAR 出自同一份源码。

**版本约束。** 当前 fork 的 tauri 已对齐 TT 声明的 2.11.6。准备脚本保留应用的版本约束、features 和 workspace 继承；pin 不满足声明时由 Cargo 报错。插件仍可能解析到声明允许的较新 minor 版本，实际结果以 pin 和生成的 lockfile 为准。

## 3. 构建

在工具链容器里使用一次性 checkout（Docker 用法见镜像仓库）。准备阶段会有意修改 Cargo manifest、capabilities 和 lockfile，这些改动不能提交。

```sh
pnpm install --frozen-lockfile
python3 scripts/ohos/prepare.py
export TARGET_TRIPLE=aarch64-unknown-linux-ohos # x86 主机上的模拟器用 x86_64-unknown-linux-ohos
source scripts/ohos/env.sh
export PATH="$HARMONY_TOOLS_DIR/command-line-tools/bin:$PATH"
pnpm ohos build --ci --target "${TARGET_TRIPLE%%-*}" \
  --config src-tauri/crates/tauritavern/tauri.ohos.conf.json -- --lib
python3 scripts/ohos/validate-hap.py --target "$TARGET_TRIPLE" \
  src-tauri/crates/tauritavern/gen/ohos/entry/build/default/outputs/default/entry-default-unsigned.hap
```

各步骤的职责：

- [`prepare.py`](../scripts/ohos/prepare.py)：
  - 按 pin 获取插件 fork 与运行时源码，用路径依赖和 `[patch.crates-io]` 把 Tauri、Wry、Tao、Ability 和插件替换为 fork；
  - 追加 `napi-ohos`、`napi-derive-ohos`，因为 fork 的 `tauri::mobile_entry_point` 和 Ability 的 `#[ability]` 宏会在应用 crate 中展开 `napi_ohos::` 路径；
  - 给 `mobile-barcode-scanner` capability 的 `platforms` 加上 `openHarmony`，因为稳定版 schema 不认识这个平台；
  - 运行插件 fork 的 `install.py --sources-only`：从同一份 Ability 源码打包 HAR（`gen/ohos/vendor/ability.har`），并复制插件的 ArkTS 源码与类型声明。
- [`env.sh`](../scripts/ohos/env.sh)：按 `TARGET_TRIPLE` 设置交叉编译器、linker 和 bindgen 参数。
- `pnpm ohos`：先构建前端，再调用镜像里的 `cargo tauri`。npm 版 CLI 没有 ohos 子命令。
- pnpm 和前端构建使用镜像里的 Node；不要把鸿蒙工具链自带的旧版 Node 前置到 `PATH`。
- `--target`：由 `TARGET_TRIPLE` 派生，一次构建只有这一个目标来源。
- `--config tauri.ohos.conf.json`：必须显式传入。fork CLI 在 `ohos build` 中按 Android 目标解析配置，不会自动合并这个文件。它把 `bundle.resources` 置空：鸿蒙读取嵌入资源，不需要再把资源复制进 HAP。
- [`validate-hap.py`](../scripts/ohos/validate-hap.py)：确认应用库存在，所有原生库都属于目标 ABI 且 ELF 架构正确，ArkTS 字节码与模块清单存在，ZIP 完整。

Release 构建沿用 `src-tauri/Cargo.toml` 的 release profile。`module.json5` 启用了 `compressNativeLibs`，HAP 中的原生库经 ZIP 压缩，ELF 内容不变。

CI：Canary 调用 `ohos.yml` 构建 aarch64，手动触发用于构建模拟器包；两者都执行上面的校验和 §4 的工程检查。鸿蒙构建失败时 Canary 的处理见 [更新渠道](CurrentState/UpdateChannels.md)。

## 4. 已跟踪工程与构建事实

[`gen/ohos`](../src-tauri/crates/tauritavern/gen/ohos) 是 TT 自有的 DevEco 工程，与 `gen/android`、`gen/apple` 一样纳入版本控制，不要用 `ohos init` 重新生成。它持有：

- 包名 `com.tauritavern.client`；
- 图标、权限、设备类型、未签名配置；
- Ability 入口：插件桥初始化与返回键桥。
- ArkUI 页面入口 `pages/Main`：挂载 HAR 的 `DefaultXComponent`，模块名由 `RustAbility` 写入的 `AppStorage` 提供。

**页面入口。** 不使用 HAR 的默认页面：它通过命名路由加载 HAR 内的页面，模块没有页面声明时会加载失败，旧 HAR 吞掉了这个错误，结果是白屏；当前 pin 已改为传播加载错误。模块本来就需要一个页面声明，所以由 TT 的 `pages/Main` 承担：`EntryAbility` 把 `defaultPage` 设为 `false`，用 `loadContent('pages/Main')` 加载，失败会直接抛出。

插件源码、Ability HAR、原生库、CLI 写入的配置和构建输出都是派生内容，由 `gen/ohos/.gitignore` 忽略。

每次构建的事实只从构建调用进入，不写回已跟踪的文件：

| 事实 | 来源 | 读取方 |
| --- | --- | --- |
| 目标 | `TARGET_TRIPLE` | `env.sh` 与 CLI 的 `--target` |
| 版本 | CLI 在调用 hvigor 前写入的合并后 Tauri 配置 `assets/tauri.conf.json`，包含 `--config` 覆盖 | 根 `hvigorfile.ts`，设置 `versionName` 和 `versionCode`。`versionCode` 沿用 Tauri 为 Android 计算的公式 `major*1_000_000 + minor*1_000 + patch`，要求 minor、patch 小于 1000 |
| 构建模式 | CLI 的 `--debug`（默认 release） | CLI 自己传给 `ohrs` 和 hvigor |

**Rust 只由 CLI 构建。** `pnpm ohos build` 先执行 `ohrs build --arch <arch> --dist entry/libs`，把应用库放进 `entry/libs/<abi>`，再调用 hvigor 打包。CLI 在本进程中设置合并后的 Tauri 配置，这次编译已经包含 `--config` 覆盖。hvigor 只打包 `entry/libs` 中的预编译库，不编译 Rust。因此 TT 只支持 `ohos build`：在 DevEco 中直接构建不会更新应用库；`ohos dev` 只在启动时编译一次 Rust，之后的 Rust 改动不会生效。

**不变式。** 准备阶段只产生约定的依赖适配差异（Cargo manifest、capabilities、lockfile），已跟踪的 `gen/ohos` 保持不变。之后无论切换目标、版本还是构建模式，构建都不新增已跟踪文件的差异。CI 在构建后检查 `git status --porcelain -- src-tauri/crates/tauritavern/gen/ohos` 为空。是否写入不取决于上一次构建的状态，所以一次构建就能暴露问题；切换目标留下的旧库只会出现在被忽略的 `entry/libs` 中，由 `validate-hap.py` 拒绝，删除 `entry/libs` 后重新构建即可。

### 4.1 x86_64 构建打出 ARM64 库

**现象。** x86_64 的 HAP 中打包的是 ARM64 应用库。

**根因。** CLI 先执行 `ohrs build --arch <arch>`，把正确目标的库放进 `entry/libs`；随后 hvigor 回调按模板的 `properties.target || "aarch64"` 又构建了一次。CLI 调用 hvigor 时只传 `-p buildMode`，没有传 target；Android 的 CLI 会给 Gradle 传 `-PtargetList`，鸿蒙这边缺了这一项。

**方案。** 删除 hvigor 中的 Rust 构建。CLI 的第一次构建已经针对正确的目标，hvigor 回调只是用默认目标再编译一次。CLI 成为唯一的构建方之后，hvigor 不再需要知道目标。

## 5. 宿主身份、资源与 IPC

平台判断的通用规则见 [FrontendGuide](FrontendGuide.md) §2.1 和 [FrontendHostContract](FrontendHostContract.md) §2.2；内嵌资源见 [Android](AndroidDevelopment.md) §2。这里只记录鸿蒙特有的事实：

- **宿主身份**为 `{ platform: 'ohos', kind: 'mobile' }`。稳定版 tauri-build 把鸿蒙归为 desktop，fork 把它归为 mobile；`platform/identity.rs` 用 `compile_error!` 守住这个前提。
- **IPC** 使用原始字节 body，帧预算由 `platform/ipc.rs` 按平台给出，鸿蒙与桌面同档。它依赖 fork 的 custom protocol body reader 一直读到 EOF。

## 6. WebView 宿主契约

### 6.1 DOM Storage 与 IndexedDB

**现象。** 早期 HAR 没有开启 `domStorageAccess`，运行时 `localStorage` 为 `null`，`i18n.js` 在模块初始化时出错，前端白屏。

**方案。** Wry 显式设置可选的 `domStorageAccess(true)`，HAR 保留 `databaseAccess`，数据在进程重启后保留。

**维护。** 升级 Ability 时确认两者仍然开启。上游已经把 DOM Storage 做成可选配置，ArkWeb 默认是关闭的。

### 6.2 初始化脚本顺序

ArkWeb 的 `javaScriptOnDocumentStart` 按字典序执行多个脚本条目，不按数组顺序。如果拆成多个条目，Tauri 的引导脚本可能在 `__TAURI_INTERNALS__` 建立之前就运行（报错 `Object.defineProperty called on non-object`）。

当前 fork 改用 `runJavaScriptOnDocumentStart`（API 15+），按数组顺序独立注入每段脚本。前一段抛错不会阻止后续的 TT 宿主身份脚本执行；主 frame 专用脚本仍包在 `window === window.top` 判断里。

### 6.3 frame 授权

- postMessage IPC 以 ArkWeb 的 `getLastJavascriptProxyCallingFrameUrl` 作为来源；custom protocol 使用请求所在 frame 的 URL。
- 主 frame 专用脚本不会进入子 frame。
- 沙箱（opaque origin）iframe 无法通过任何一条 IPC 路径调用原生接口。

### 6.4 弹窗

- ArkWeb 原生的 `alert`/`confirm`/`prompt` 由 HAR 的 WebHost 处理，按钮资源包含英文、简体中文和繁体中文。
- OHOS 不再注入 dialog 插件的 `init-iife.js`，保留原生 `alert` 和同步返回布尔值的 `confirm`，包括扩展直接调用的场景。
- 桌面端和 iOS 的 dialog 插件注入行为由各自平台处理。

### 6.5 返回键

`Main.ets` 的 `onBackPress` 交给 HAR 的 WebHost 处理：

1. 处于网页全屏时，先退出全屏；
2. 否则执行 `window.__TAURITAVERN_HANDLE_BACK__()`；
3. 返回 `false` 时，Ability 退到后台（见 `EntryAbility.ets`）。

逐层关闭的逻辑仍在 `back-navigation.js` 中，原生侧只负责转发和退到后台。

### 6.6 其他浏览器能力

- `<input type="file">`：由 HAR 的文件选择回调处理；
- `window.open`：按移动宿主的规则处理，见 [FrontendHostContract](FrontendHostContract.md) §5.4；
- 网页全屏可以进入和退出；
- 软键盘弹出时 visual viewport 缩小，输入框保持可见；
- 安全区：`EntryAbility` 设置 `setWindowLayoutFullScreen(false)`，由系统避让，`--tt-inset-*` 回落到 `env()`。

## 7. 文件导入与导出

鸿蒙是[文件传输](CurrentState/FileTransfer.md)的一个平台实现，与 Android 共用 URI 复制代码。鸿蒙特有的约束：dialog fork 只为面板返回的 URI 授权，授权保存在 Ability 的内存中，进程结束即失效。

## 8. 局域网同步

鸿蒙与 Android、Windows、Linux 共用 Rust mDNS 与本机地址实现，地址规则见[同步](CurrentState/Sync.md#lan-发现与信任)。鸿蒙特有的约束：普通应用不能绑定 `NETLINK_ROUTE` 套接字；本机地址经 libc `getifaddrs` 获取，鸿蒙 musl 的实现不绑定该套接字，接口查询受限时用 ioctl 补齐接口名和标志。鸿蒙没有公开的组播锁接口。

## 9. 权限

| 权限 | 用途 |
| --- | --- |
| `INTERNET` | 模型请求、扩展下载、局域网同步等网络访问 |
| `CAMERA` | 条码插件扫码（ScanKit，需要支持的 HarmonyOS 设备） |
| `READ_PASTEBOARD` | 网页通过 `navigator.clipboard.readText` 主动读取剪贴板：`/clipboard-get`，以及 Quick Reply 的"从剪贴板添加"。复制和普通粘贴不依赖它。它属于受限权限（需要 ACL），ArkWeb 是否要求这个权限尚未实测 |

## 10. 未覆盖

- **后台生成：** 鸿蒙没有对应的后台执行保护（`generation_background` 返回空），应用退到后台后，生成可能被系统挂起。
- **局域网同步：** 已在 x86_64 模拟器验证；真机上的 Wi-Fi、热点、VPN 与跨设备互通尚未验证。
- **系统通知、扫码、语音合成：** 尚未验证。

## 11. 设备验收清单

- ARM64 真机和 x86_64 模拟器都能进入主界面，`window.__TAURITAVERN_HOST__` 为 `{ platform: 'ohos', kind: 'mobile' }`。
- localStorage 和 IndexedDB 在强制结束进程后仍保留。
- 沙箱 iframe 无法通过任何一条 IPC 路径调用原生接口。
- 新建聊天、发送、保存、强制结束后重新打开，内容完整；超过单帧预算的聊天提交与上传成功。
- 原生选择器导入、保存面板导出可用，取消不报错；`<input type="file">` 可用。
- 同步面板（含 TT-Sync）可打开，接收服务能启动；与其他设备互相发现、配对并双向同步。
- Wi-Fi 与移动数据同时在线时，配对链接使用 Wi-Fi 地址；组网地址出现在可用地址中，可通过手动地址配对与同步。
- TT 设置页能打开；返回键逐层关闭弹窗和抽屉，最后退到后台。
- 复制、粘贴与 `/clipboard-get` 正常；外链、全屏、媒体 Range、键盘和安全区表现正常。
