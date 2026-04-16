# Scripts

这个目录存放仓库内的开发、构建、迁移导出与 CI 辅助脚本。

## SillyTavern 迁移导出

这两个脚本会交互式生成一个可直接导入 TauriTavern `data-migration` 扩展的 zip：

- 自动检测当前目录是否为 SillyTavern 根目录
- 可选是否导出 `data/default-user/backups`
- 自动将 `public/scripts/extensions/third-party` 映射到 `data/extensions/third-party`
- 提供明显的压缩进度提示

### 一键执行

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

- `export-sillytavern-migration.sh`
  面向 Unix/macOS/Linux/Termux 的 SillyTavern 迁移导出脚本。
- `export-sillytavern-migration.ps1`
  面向 Windows PowerShell 的 SillyTavern 迁移导出脚本。
- `build-portable.mjs`
  构建 Tauri portable 二进制，并将产物复制到指定输出目录。对应 `pnpm run tauri:build:portable`。
- `check-frontend-guardrails.mjs`
  校验前端宿主层文件规模和依赖边界，避免 Host Kernel 持续膨胀。对应 `pnpm run check:frontend`。
- `audit-panel-runtime-deps.mjs`
  用 `rg` 快速审计若干运行时依赖锚点，属于开发期排查辅助脚本。
- `tauri-ios-xcode-script.sh`
  包装 `tauri ios xcode-script`，补齐 Xcode GUI 构建环境中的 PATH / Node / pnpm，并在构建后处理 iOS 图标。
- `ios-policy.mjs`
  iOS Dev/Build 包装脚本：为构建过程注入 `TAURITAVERN_IOS_POLICY_PROFILE`，并在 `ios_internal_full` / `ios_external_beta` 构建时自动使用 `--export-method app-store-connect`。
- `ios-opaque-app-icons.swift`
  将 iOS App Icon PNG 展平为不透明背景，供 `tauri-ios-xcode-script.sh` 调用。
- `ci/setup-macos-signing.sh`
  GitHub Actions / CI 中的 macOS 签名初始化脚本，用于导入证书、创建 keychain 与写入 Apple API Key 路径。
- `guardrails/frontend-lines-baseline.json`
  `check-frontend-guardrails.mjs` 使用的基线数据文件，不是独立执行脚本。

## 维护约定

- 面向最终用户的一次性脚本，优先保持交互友好、依赖少、失败直出。
- 面向仓库内部的脚本，优先通过 `pnpm` script 或 CI 调用，不额外扩散入口。
- 如果修改迁移导出脚本涉及归档结构，请同步确认它仍符合当前 `data-migration` 导入器的 `data/...` 契约。
