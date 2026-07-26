# Scripts

这个目录存放仓库内的开发、构建、迁移导出与 CI 辅助脚本。

## Linux 一键安装

`install-linux.sh` 为 Debian 12+、Ubuntu 22.04 LTS+、Fedora 与 openSUSE Leap 16.0 配置签名软件源，并安装或更新 TauriTavern：

```sh
curl -fsSL https://raw.githubusercontent.com/Darkatse/TauriTavern/main/scripts/install-linux.sh | sh
```

使用 Canary 渠道：

```sh
curl -fsSL https://raw.githubusercontent.com/Darkatse/TauriTavern/main/scripts/install-linux.sh \
  | sh -s -- --channel canary
```

NixOS 会自动使用项目 flake；其他已经安装 Nix 的 Linux 可显式选择 Nix：

```sh
curl -fsSL https://raw.githubusercontent.com/Darkatse/TauriTavern/main/scripts/install-linux.sh \
  | sh -s -- --method nix
```

Nix 安装进入当前用户 profile，不使用 sudo，也不修改 `/etc/nix/nix.conf`。脚本使用 POSIX `sh` 语法，可由 sh、Dash、Bash 或 Zsh 执行。运行前可先下载并检查内容，或使用 `--dry-run` 查看识别结果和执行计划：

```sh
curl -fsSL \
  https://raw.githubusercontent.com/Darkatse/TauriTavern/main/scripts/install-linux.sh \
  -o install-tauritavern.sh
sh install-tauritavern.sh --dry-run
```

原生软件源安装会核对完整的 OpenPGP 主密钥和签名子密钥指纹。Nix 安装接受项目 flake 声明的 binary cache；multi-user Nix 仍要求 daemon 预先信任该 cache，否则可能回退到本地构建。

## SillyTavern 迁移导出

这两个脚本会交互式生成一个可直接导入 TauriTavern `data-migration` 扩展的 zip：

- 自动检测当前目录是否为 SillyTavern 根目录
- 可选是否导出 `data/default-user/backups`
- 自动将 `public/scripts/extensions/third-party` 映射到 `data/extensions/third-party`
- 提供明显的压缩进度提示

### 一键执行

一键执行需要在可交互终端中运行；脚本通过终端读取选项，避免与 `curl | sh` 的脚本输入流冲突。

Unix / macOS / Linux / Termux:

```sh
curl -fsSL https://raw.githubusercontent.com/Darkatse/TauriTavern/main/scripts/export-sillytavern-migration.sh | sh
```

Windows PowerShell:

```powershell
iex (iwr 'https://raw.githubusercontent.com/Darkatse/TauriTavern/main/scripts/export-sillytavern-migration.ps1').Content
```

### 本地执行

Unix / macOS / Linux / Termux:

```sh
sh scripts/export-sillytavern-migration.sh
```

Windows PowerShell:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\export-sillytavern-migration.ps1
```

## 目录说明

Flatpak 构建配方位于 `packaging/flatpak/`，软件源发布工具位于
`distribution/`。

- `install-linux.sh`
  通过受支持的 APT/RPM 软件源或 Nix flake 安装 TauriTavern。
- `export-sillytavern-migration.sh`
  面向 Unix/macOS/Linux/Termux 的 SillyTavern 迁移导出脚本。
- `export-sillytavern-migration.ps1`
  面向 Windows PowerShell 的 SillyTavern 迁移导出脚本。
- `build-portable.mjs`
  构建 Tauri portable 二进制，并将产物复制到指定输出目录。对应 `pnpm run tauri:build:portable`。
- `tauri-before-build.mjs`
  统一生成 Tauri 打包所需的前端 bundle；pnpm 启动入口也复用它以覆盖移动端 IDE 构建。
- `tauri-dev-server.mjs`
  为 `tauri dev` 提供轻量静态前端服务器、页面刷新通道与开发态 Service Worker 会话 bootstrap，避免普通前端文件变化污染 Rust 编译指纹。
- `check-frontend-guardrails.mjs`
  校验前端宿主层文件规模和依赖边界，避免 Host Kernel 持续膨胀。对应 `pnpm run check:frontend`。
- `tauri-ios-xcode-script.sh`
  包装 `tauri ios xcode-script`，补齐 Xcode GUI 构建环境中的 PATH / Node / pnpm，并在构建后处理 iOS 图标。
- `generate-ios-app-icon-variants.swift`
  从 `src-tauri/crates/tauritavern/icons/icon.png` 生成 iOS `Any` / `Dark` / `Tinted` 三个 1024px App Icon 源图。
- `ios-policy.mjs`
  iOS Dev/Build 包装脚本：为构建过程注入 `TAURITAVERN_IOS_POLICY_PROFILE`，并在 `ios_internal_full` / `ios_external_beta` 构建时自动使用 `--export-method app-store-connect`。
- `ios-opaque-app-icons.swift`
  校验 iOS App Icon appearance 变体，并只将基础 `Any` 图标展平为不透明背景，供 `tauri-ios-xcode-script.sh` 调用。
- `ci/setup-macos-signing.sh`
  GitHub Actions / CI 中的 macOS 签名初始化脚本，用于导入证书、创建 keychain 与写入 Apple API Key 路径。
- `ci/verify-release-version.mjs`
  校验 Stable Release tag 与前端、Cargo、Tauri、Cargo lock 和 Nix 包版本一致。
- `ci/collect-release-assets.mjs`
  校验完整的跨平台构建产物，并按 Stable 与 Canary 共用的用户可见命名契约收集 Release 资产。
- `ci/distribute-testflight.mjs`
  等待 App Store Connect 处理指定 iOS 构建，写入 `What to Test`，关联公开外测组，并按当前状态提交 Beta App Review。
- `guardrails/frontend-lines-baseline.json`
  `check-frontend-guardrails.mjs` 使用的基线数据文件，文件行数硬性限制指标。

## 维护约定

- 面向最终用户的一次性脚本，优先保持交互友好、依赖少、失败直出。
- 面向仓库内部的脚本，优先通过 `pnpm` script 或 CI 调用，不额外扩散入口。
- 如果修改迁移导出脚本涉及归档结构，请同步确认它仍符合当前 `data-migration` 导入器的 `data/...` 契约。
