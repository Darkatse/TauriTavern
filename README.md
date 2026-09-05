<div align="center">

<img src="docs/images/tauritavern-readme-hero.webp" alt="TauriTavern" width="720">

# TauriTavern

**SillyTavern 的原生应用 —— 桌面与移动，开箱即用**

**简体中文** · [English](README.en.md) · [日本語](README.ja.md) · [Русский](README.ru.md) · [Português (Brasil)](README.pt-BR.md)

[下载](https://tauritavern.github.io/downloads/) · [文档](https://tauritavern.github.io/) · [Releases](https://github.com/Darkatse/TauriTavern/releases) · [Issues](https://github.com/Darkatse/TauriTavern/issues)

[![Release](https://img.shields.io/github/v/release/Darkatse/TauriTavern?style=flat-square&color=1f9d96)](https://github.com/Darkatse/TauriTavern/releases/latest)
[![License](https://img.shields.io/github/license/Darkatse/TauriTavern?style=flat-square)](LICENSE)
[![Stars](https://img.shields.io/github/stars/Darkatse/TauriTavern?style=flat-square&labelColor=black&color=ffcb47)](https://github.com/Darkatse/TauriTavern/stargazers)
[![Platforms](https://img.shields.io/badge/platforms-Windows%20%C2%B7%20macOS%20%C2%B7%20Linux%20%C2%B7%20Android%20%C2%B7%20iOS-1f9d96?style=flat-square)](https://tauritavern.github.io/downloads/)
<br/>
[![Telegram](https://img.shields.io/badge/Telegram-%E7%BE%A4%E7%BB%84-26A5E4?style=flat-square&logo=telegram&logoColor=white)](https://t.me/TauriTavern)
[![Discord](https://img.shields.io/badge/Discord-%E7%A4%BE%E5%8C%BA-5865F2?style=flat-square&logo=discord&logoColor=white)](https://discord.gg/hn57aFGe8h)
[![Issues](https://img.shields.io/github/issues/Darkatse/TauriTavern?style=flat-square&logo=github)](https://github.com/Darkatse/TauriTavern/issues)
[![Canary](https://img.shields.io/github/actions/workflow/status/Darkatse/TauriTavern/canary-release.yml?style=flat-square&logo=githubactions&label=canary)](https://github.com/Darkatse/TauriTavern/actions/workflows/canary-release.yml)

</div>

## 下载

<div align="center">

[![⬇ 下载 TauriTavern](https://img.shields.io/badge/%E2%AC%87_%E4%B8%8B%E8%BD%BD-TauriTavern-1f9d96?style=for-the-badge)](https://tauritavern.github.io/downloads/)

**自动识别你的设备 · 一键获取最新稳定版**

[![Windows](https://custom-icon-badges.demolab.com/badge/Windows-0078D6?logo=windows11&logoColor=white&style=flat-square)](https://tauritavern.github.io/downloads/platforms/)
[![macOS](https://img.shields.io/badge/macOS-000000?style=flat-square&logo=apple&logoColor=white)](https://tauritavern.github.io/downloads/platforms/)
[![Linux](https://img.shields.io/badge/Linux-FCC624?style=flat-square&logo=linux&logoColor=black)](https://tauritavern.github.io/downloads/platforms/)
[![Android](https://img.shields.io/badge/Android-3DDC84?style=flat-square&logo=android&logoColor=white)](https://tauritavern.github.io/downloads/platforms/)
[![iOS TestFlight](https://img.shields.io/badge/iOS-TestFlight-0D96F6?style=flat-square&logo=apple&logoColor=white)](https://testflight.apple.com/join/gpqAdeTm)

[全部平台下载](https://tauritavern.github.io/downloads/platforms/) · [GitHub Releases](https://github.com/Darkatse/TauriTavern/releases)

</div>

<details>
<summary><b>📦 使用包管理器安装</b>（Windows · macOS · Linux）</summary>

### Windows · WinGet

在 PowerShell 中运行：

```powershell
winget install --id TauriTavern.TauriTavern
```

### Windows · Scoop

在 PowerShell 中运行：

```powershell
scoop bucket add Darkatse https://github.com/Darkatse/Scoop-Darkatse.git
scoop install Darkatse/TauriTavern
```

### macOS · Homebrew

在终端中运行：

```sh
brew install --cask tauritavern
```

### Linux

**Arch Linux · AUR**

使用你喜欢的 AUR 助手安装 [`tauritavern-bin`](https://aur.archlinux.org/packages/tauritavern-bin)：

```sh
yay -S tauritavern-bin
```

该软件包由 [@LX2000WASD](https://github.com/LX2000WASD) 维护，感谢他在 [TauriTavern-aur](https://github.com/LX2000WASD/TauriTavern-aur) 中的持续维护贡献！

**Debian · Ubuntu · Fedora · openSUSE · NixOS**

脚本会自动识别系统并选择合适的安装方式，也支持其他已经安装 Nix 的 Linux。

**稳定版**

```sh
curl -fsSL https://raw.githubusercontent.com/Darkatse/TauriTavern/main/scripts/install-linux.sh | sh
```

**Nix / NixOS**

已经安装 Nix 的用户可以直接加入当前用户 profile：

```sh
nix profile add github:Darkatse/TauriTavern#tauritavern
```

**Flatpak**

```sh
flatpak remote-add --user --if-not-exists \
  tauritavern \
  https://flatpak.tauritavern.com/tauritavern.flatpakrepo
flatpak install --user tauritavern com.tauritavern.client
```

</details>

### Canary

Canary 版本每天更新，包含最新功能和修复，但稳定性可能不如稳定版。如果你希望体验最新版本，或在稳定版中遇到问题，可以先尝试 Canary，确认问题是否已经修复。

Windows、macOS 和移动平台可从 [Canary Release](https://github.com/Darkatse/TauriTavern/releases/tag/Canary) 下载。

<details>
<summary><b>Linux Canary 安装</b></summary>

**Linux**

```sh
curl -fsSL https://raw.githubusercontent.com/Darkatse/TauriTavern/main/scripts/install-linux.sh \
  | sh -s -- --channel canary
```

**Nix / NixOS**

```sh
nix profile add github:Darkatse/TauriTavern/Canary#canary
```

</details>

> [!TIP]
> **iOS 用户**：通过 [TestFlight 公开外测](https://testflight.apple.com/join/gpqAdeTm) 安装，需要 iOS 15.0 或更高版本。iOS 15.0–16.3 仅提供有限支持，完整支持从 iOS 16.4 开始。请注意 TestFlight 版本需要遵守苹果的 TestFlight 规则，存在使用限制。
>
> **Windows 便携版**（Portable）：需系统已安装 WebView2 运行时。

## 这是什么

TauriTavern 把 [SillyTavern](https://github.com/SillyTavern/SillyTavern) 移植为真正的原生应用：前端完整保留上游体验（已同步 1.18.0），后端从 Node.js 重构为 Rust（Tauri v2）。

不需要安装 Node.js，不需要命令行，安装即用。你的角色卡、聊天记录、预设、世界书与前端扩展，全部兼容。

> 注：TauriTavern 是独立维护的开源项目，并非 SillyTavern 官方客户端。TauriTavern 完全免费且开源，遵循 AGPL-3.0 许可协议。请在使用前仔细阅读许可条款。

## 特性亮点

- 🖥️ **全平台原生**：Windows、macOS、Linux、Android、iOS，桌面与移动同一份体验
- 🎭 **完整 SillyTavern 体验**：前端同步上游 1.18.0，数据格式与目录布局完全兼容
- 🧩 **前端扩展生态**：内置原生 Git，安装、更新、切换分支都在界面内完成（不支持上游 Node-only 后端插件）
- 🔄 **内置多设备同步**：局域网加密配对同步，或经远端 TT-Sync v2 自动上传
- 🤖 **Agent 框架**：工具调用、Skills、子代理与运行时间线，持续演进中
- 📦 **一键迁移**：SillyTavern 数据导出脚本 + 应用内导入，平滑搬家
- ⚡ **性能工程**：分阶段启动、聊天虚拟DOM加载，超长聊天记录依然流畅
- 🔒 **数据自主**：数据完全保存在本地，支持便携模式

## 截图

<div align="center">
<img src="docs/images/tauritavern-multidevice-cutout.webp" alt="TauriTavern 桌面与移动端界面" width="760">
</div>

## 架构速览

本项目 Rust 后端是遵循 Clean Architecture 的 Cargo workspace（`src-tauri/crates/`）：

- `tauritavern`：Tauri host、命令层与组合根
- `tt-application` · `tt-ports` · `tt-domain` · `tt-contracts`：用例、端口、领域模型与跨 crate 契约
- `tt-adapter-*`：存储、HTTP、媒体、同步、扩展、分词等具体实现

前端为上游 SillyTavern + 模块化 Tauri 注入层（`src/tauri/main/`），经 `window.__TAURITAVERN__` 平台 ABI 与 Rust 后端通信。详情请见 [docs/BackendStructure.md](docs/BackendStructure.md) 与 [docs/FrontendGuide.md](docs/FrontendGuide.md)。

<details>
<summary><b>🛠 开发与构建</b>（前置要求 · 常用命令 · Tauri Pilot · 便携构建 · FasTools）</summary>

**前置要求**：Rust stable（支持 edition 2024）· Node.js 22.13+ · pnpm 11 · Tauri CLI

```bash
git clone https://github.com/Darkatse/TauriTavern.git
cd TauriTavern
pnpm install
```

**常用命令**：

```bash
pnpm run check         # 前端 guardrails/类型/契约 + Rust dev check
pnpm run web:build     # 构建前端资源包（Rspack）
pnpm run tauri:dev     # 桌面开发模式
pnpm run tauri:build   # 构建桌面发行包
pnpm run android:dev   # Android 开发模式
pnpm run ios:dev       # iOS 开发模式
```

**Tauri Pilot（AI Agent 界面调试）**

项目已接入 [Tauri Pilot](https://github.com/mpiton/tauri-pilot) 的开发专用插件与权限。它让 AI Agent 通过可访问性快照检查和操作桌面端 WebView；普通开发与发行命令不会启用这项能力。

```bash
cargo install tauri-pilot-cli  # 仅首次需要
pnpm run tauri:dev:pilot
```

应用启动后，在另一终端按以下顺序操作：

```bash
tauri-pilot ping
tauri-pilot snapshot -i
tauri-pilot click @e3          # 使用当前 snapshot 返回的 ref
tauri-pilot diff -i
tauri-pilot logs --level error
```

交互前先取得 snapshot，每次只执行一个操作；异步更新后使用 `wait`，并优先用 `assert` 验证结果。支持 MCP 的 Agent 可将 `tauri-pilot mcp` 注册为 stdio server。

**便携版构建**：`pnpm run tauri:build:portable`（默认输出至 `release/`）；运行时可通过 `TAURITAVERN_RUNTIME_MODE=portable` 或 `portable.flag` 强制启用便携策略。

**FasTools**：超级好用的开发与部署调试工具箱，强烈推荐。`pnpm run fastools:build` 进行构建，`pnpm run fastools:run` 运行。

平台细节见 [docs/AndroidDevelopment.md](docs/AndroidDevelopment.md) 与 [docs/iOSDevelopment.md](docs/iOSDevelopment.md)。


</details>

## 文档

- 📖 [在线文档站](https://tauritavern.github.io/)：中英双语，含指南、Agent、架构、API 与下载
- [docs/FrontendGuide.md](docs/FrontendGuide.md)：前端架构与扩展指南
- [docs/FrontendHostContract.md](docs/FrontendHostContract.md)：宿主层对外契约
- [docs/BackendStructure.md](docs/BackendStructure.md)：后端 Clean Architecture 与 crate 边界
- [docs/CurrentState/](docs/CurrentState/README.md)：已落地模块的实现状态

## 贡献

欢迎 Issue 与 PR。提交前请阅读 [CONTRIBUTING.md](CONTRIBUTING.md)。除紧急修复外，PR 请以 `dev` 为目标分支。

## 致谢与许可

基于 [SillyTavern](https://github.com/SillyTavern/SillyTavern) 与 [Tauri](https://tauri.app/) 构建，并感谢 [Cocktail](https://github.com/Lianues/cocktail)、[Tavern-Helper](https://github.com/N0VI028/JS-Slash-Runner)、[LittleWhiteBox](https://github.com/RT15548/LittleWhiteBox)、[MikTik](https://github.com/Darkatse/MikTik)，以及维护 [TauriTavern AUR 软件包](https://github.com/LX2000WASD/TauriTavern-aur) 的 [@LX2000WASD](https://github.com/LX2000WASD)。

以 [AGPL-3.0](LICENSE) 许可发布（与 SillyTavern 同系列许可协议）。

[![Contributors](https://contrib.rocks/image?repo=Darkatse/TauriTavern)](https://github.com/Darkatse/TauriTavern/graphs/contributors)

<p align="center"><sub><em>我们尽力用爱打造 ❤️ —— TauriTavern 团队</em></sub></p>
