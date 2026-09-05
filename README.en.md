<div align="center">

<img src="docs/images/tauritavern-readme-hero.webp" alt="TauriTavern" width="720">

# TauriTavern

**SillyTavern, as a native app — desktop & mobile, out of the box**

[简体中文](README.md) · **English** · [日本語](README.ja.md) · [Русский](README.ru.md) · [Português (Brasil)](README.pt-BR.md)

[Downloads](https://tauritavern.github.io/en/downloads/) · [Docs](https://tauritavern.github.io/en/) · [Releases](https://github.com/Darkatse/TauriTavern/releases) · [Issues](https://github.com/Darkatse/TauriTavern/issues)

[![Release](https://img.shields.io/github/v/release/Darkatse/TauriTavern?style=flat-square&color=1f9d96)](https://github.com/Darkatse/TauriTavern/releases/latest)
[![License](https://img.shields.io/github/license/Darkatse/TauriTavern?style=flat-square)](LICENSE)
[![Stars](https://img.shields.io/github/stars/Darkatse/TauriTavern?style=flat-square&labelColor=black&color=ffcb47)](https://github.com/Darkatse/TauriTavern/stargazers)
[![Platforms](https://img.shields.io/badge/platforms-Windows%20%C2%B7%20macOS%20%C2%B7%20Linux%20%C2%B7%20Android%20%C2%B7%20iOS-1f9d96?style=flat-square)](https://tauritavern.github.io/en/downloads/)
<br/>
[![Telegram](https://img.shields.io/badge/Telegram-Group-26A5E4?style=flat-square&logo=telegram&logoColor=white)](https://t.me/TauriTavern)
[![Discord](https://img.shields.io/badge/Discord-Community-5865F2?style=flat-square&logo=discord&logoColor=white)](https://discord.gg/hn57aFGe8h)
[![Issues](https://img.shields.io/github/issues/Darkatse/TauriTavern?style=flat-square&logo=github)](https://github.com/Darkatse/TauriTavern/issues)
[![Canary](https://img.shields.io/github/actions/workflow/status/Darkatse/TauriTavern/canary-release.yml?style=flat-square&logo=githubactions&label=canary)](https://github.com/Darkatse/TauriTavern/actions/workflows/canary-release.yml)

</div>

## Download

<div align="center">

[![⬇ Download TauriTavern](https://img.shields.io/badge/%E2%AC%87_Download-TauriTavern-1f9d96?style=for-the-badge)](https://tauritavern.github.io/en/downloads/)

**Auto-detects your device · one click to the latest stable build**

[![Windows](https://custom-icon-badges.demolab.com/badge/Windows-0078D6?logo=windows11&logoColor=white&style=flat-square)](https://tauritavern.github.io/en/downloads/platforms/)
[![macOS](https://img.shields.io/badge/macOS-000000?style=flat-square&logo=apple&logoColor=white)](https://tauritavern.github.io/en/downloads/platforms/)
[![Linux](https://img.shields.io/badge/Linux-FCC624?style=flat-square&logo=linux&logoColor=black)](https://tauritavern.github.io/en/downloads/platforms/)
[![Android](https://img.shields.io/badge/Android-3DDC84?style=flat-square&logo=android&logoColor=white)](https://tauritavern.github.io/en/downloads/platforms/)
[![iOS TestFlight](https://img.shields.io/badge/iOS-TestFlight-0D96F6?style=flat-square&logo=apple&logoColor=white)](https://testflight.apple.com/join/gpqAdeTm)

[All platforms](https://tauritavern.github.io/en/downloads/platforms/) · [GitHub Releases](https://github.com/Darkatse/TauriTavern/releases)

</div>

<details>
<summary><b>📦 Install with a package manager</b> (Windows · macOS · Linux)</summary>

### Windows · WinGet

Run in PowerShell:

```powershell
winget install --id TauriTavern.TauriTavern
```

### Windows · Scoop

Run in PowerShell:

```powershell
scoop bucket add Darkatse https://github.com/Darkatse/Scoop-Darkatse.git
scoop install Darkatse/TauriTavern
```

### macOS · Homebrew

Run in Terminal:

```sh
brew install --cask tauritavern
```

### Linux

**Arch Linux · AUR**

Install [`tauritavern-bin`](https://aur.archlinux.org/packages/tauritavern-bin) with your preferred AUR helper:

```sh
yay -S tauritavern-bin
```

This package is maintained by [@LX2000WASD](https://github.com/LX2000WASD). Many thanks for the ongoing work in [TauriTavern-aur](https://github.com/LX2000WASD/TauriTavern-aur).

**Debian · Ubuntu · Fedora · openSUSE · NixOS**

The script detects your system and chooses the appropriate installation method. It also supports other Linux systems with Nix installed.

**Stable**

```sh
curl -fsSL https://raw.githubusercontent.com/Darkatse/TauriTavern/main/scripts/install-linux.sh | sh
```

**Nix / NixOS**

If Nix is already installed, add TauriTavern directly to your user profile:

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

Canary receives daily updates with new features and fixes, but may be less reliable than Stable. If you want to try the latest build or are having an issue with Stable, check whether Canary already includes a fix.

Windows, macOS, and mobile builds are available from the [Canary Release](https://github.com/Darkatse/TauriTavern/releases/tag/Canary).

<details>
<summary><b>Install Canary on Linux</b></summary>

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
> **iOS**: install via the [public TestFlight beta](https://testflight.apple.com/join/gpqAdeTm). It requires iOS 15.0 or later. iOS 15.0–16.3 receives limited support; full support starts with iOS 16.4. TestFlight builds are subject to Apple's TestFlight rules and have usage restrictions.
>
> **Windows portable**: requires the WebView2 runtime on the system.

## What is TauriTavern

TauriTavern ports [SillyTavern](https://github.com/SillyTavern/SillyTavern) into a true native app: the frontend keeps the full upstream experience (synced to 1.18.0), while the backend is rebuilt from Node.js into Rust (Tauri v2).

No Node.js to install, no command line — just download and run. Your character cards, chats, presets, world info, and frontend extensions all stay compatible with SillyTavern.

> Note: TauriTavern is an independently maintained open-source project, not an official SillyTavern client. TauriTavern is free and open source under the AGPL-3.0 license. Please review the license terms before use.

## Highlights

- 🖥️ **Five native platforms**: Windows, macOS, Linux, Android, and iOS — one experience everywhere
- 🎭 **Full SillyTavern experience**: frontend synced to upstream 1.18.0, with fully compatible data formats and directory layout
- 🧩 **Frontend extension ecosystem**: built-in native Git — install, update, and switch branches right in the UI (Node-only backend plugins are not supported)
- 🔄 **Built-in multi-device sync**: encrypted LAN pairing, or automatic upload via the remote TT-Sync v2
- 🤖 **Agent framework**: tool calling, Skills, sub-agents, and a run timeline; development is ongoing
- 📦 **One-click migration**: SillyTavern export scripts + in-app import for a smooth move
- ⚡ **Performance engineering**: phased startup and windowed chat loading keep even very long chats smooth
- 🔒 **Your data, yours**: everything stays on your device, with an optional portable mode

## Screenshots

<div align="center">
<img src="docs/images/tauritavern-multidevice-cutout.webp" alt="TauriTavern on desktop and mobile" width="760">
</div>

## Architecture at a Glance

The Rust backend is a Clean Architecture Cargo workspace (`src-tauri/crates/`):

- `tauritavern`: Tauri host, command layer, and composition root
- `tt-application` · `tt-ports` · `tt-domain` · `tt-contracts`: use cases, ports, domain models, and cross-crate contracts
- `tt-adapter-*`: concrete storage, HTTP, media, sync, extension, and tokenization implementations

The frontend is upstream SillyTavern plus a modular Tauri injection layer (`src/tauri/main/`), talking to the Rust backend through the `window.__TAURITAVERN__` platform ABI. See [docs/BackendStructure.md](docs/BackendStructure.md) and [docs/FrontendGuide.md](docs/FrontendGuide.md) for details.

<details>
<summary><b>🛠 Development</b> (prerequisites · common commands · Tauri Pilot · portable builds · FasTools)</summary>

**Prerequisites**: Rust stable (edition 2024) · Node.js 22.13+ · pnpm 11 · Tauri CLI

```bash
git clone https://github.com/Darkatse/TauriTavern.git
cd TauriTavern
pnpm install
```

**Common commands**:

```bash
pnpm run check         # frontend guardrails/types/contracts + Rust dev check
pnpm run web:build     # build frontend bundles (Rspack)
pnpm run tauri:dev     # desktop dev mode
pnpm run tauri:build   # build desktop installers
pnpm run android:dev   # Android dev mode
pnpm run ios:dev       # iOS dev mode
```

**Tauri Pilot (UI development with AI agents)**

The project already includes the development-only [Tauri Pilot](https://github.com/mpiton/tauri-pilot) plugin and capability. It lets an AI agent inspect and operate the desktop WebView through accessibility snapshots. The standard development and release commands do not enable it.

```bash
cargo install tauri-pilot-cli  # only needed once
pnpm run tauri:dev:pilot
```

After the app starts, use another terminal for the basic workflow:

```bash
tauri-pilot ping
tauri-pilot snapshot -i
tauri-pilot click @e3          # use a ref from the current snapshot
tauri-pilot diff -i
tauri-pilot logs --level error
```

Take a snapshot before interacting and perform one action at a time. Use `wait` after asynchronous updates and prefer `assert` for verification. An MCP-capable agent can register `tauri-pilot mcp` as a stdio server.

**Portable builds**: `pnpm run tauri:build:portable` (outputs to `release/`); force portable runtime with `TAURITAVERN_RUNTIME_MODE=portable` or a `portable.flag` file.

**FasTools**: the recommended toolkit for development and deployment debugging. Build with `pnpm run fastools:build`, run with `pnpm run fastools:run`.

Platform details: [docs/AndroidDevelopment.md](docs/AndroidDevelopment.md) and [docs/iOSDevelopment.md](docs/iOSDevelopment.md).

</details>

## Documentation

- 📖 [Online docs](https://tauritavern.github.io/en/): bilingual guides covering usage, Agent, architecture, API, and downloads
- [docs/FrontendGuide.md](docs/FrontendGuide.md): frontend architecture and extension guide
- [docs/FrontendHostContract.md](docs/FrontendHostContract.md): public host-layer contract
- [docs/BackendStructure.md](docs/BackendStructure.md): backend Clean Architecture and crate boundaries
- [docs/CurrentState/](docs/CurrentState/README.md): implementation status of shipped modules

## Contributing

Issues and PRs are welcome! Please read [CONTRIBUTING.md](CONTRIBUTING.md) first, target the `dev` branch unless it is an urgent fix.

## Acknowledgements & License

Built on [SillyTavern](https://github.com/SillyTavern/SillyTavern) and [Tauri](https://tauri.app/), with thanks to [Cocktail](https://github.com/Lianues/cocktail), [Tavern-Helper](https://github.com/N0VI028/JS-Slash-Runner), [LittleWhiteBox](https://github.com/RT15548/LittleWhiteBox), [MikTik](https://github.com/Darkatse/MikTik), and [@LX2000WASD](https://github.com/LX2000WASD) for maintaining the [TauriTavern AUR package](https://github.com/LX2000WASD/TauriTavern-aur).

Released under [AGPL-3.0](LICENSE) (same license family as SillyTavern).

[![Contributors](https://contrib.rocks/image?repo=Darkatse/TauriTavern)](https://github.com/Darkatse/TauriTavern/graphs/contributors)

<p align="center"><sub><em>Made with love and care ❤️ — The TauriTavern Team</em></sub></p>
