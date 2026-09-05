<div align="center">

<img src="docs/images/tauritavern-readme-hero.webp" alt="TauriTavern" width="720">

# TauriTavern

**SillyTavernをネイティブアプリに — デスクトップでもモバイルでも、すぐに使えます**

[简体中文](README.md) · [English](README.en.md) · **日本語** · [Русский](README.ru.md) · [Português (Brasil)](README.pt-BR.md)

[ダウンロード](https://tauritavern.github.io/en/downloads/) · [ドキュメント](https://tauritavern.github.io/en/) · [リリース](https://github.com/Darkatse/TauriTavern/releases) · [Issue](https://github.com/Darkatse/TauriTavern/issues)

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

## ダウンロード

<div align="center">

[![⬇ Download TauriTavern](https://img.shields.io/badge/%E2%AC%87_Download-TauriTavern-1f9d96?style=for-the-badge)](https://tauritavern.github.io/en/downloads/)

**端末を自動判別し、最新の安定版をワンクリックでダウンロード**

[![Windows](https://custom-icon-badges.demolab.com/badge/Windows-0078D6?logo=windows11&logoColor=white&style=flat-square)](https://tauritavern.github.io/en/downloads/platforms/)
[![macOS](https://img.shields.io/badge/macOS-000000?style=flat-square&logo=apple&logoColor=white)](https://tauritavern.github.io/en/downloads/platforms/)
[![Linux](https://img.shields.io/badge/Linux-FCC624?logo=linux&logoColor=black&style=flat-square)](https://tauritavern.github.io/en/downloads/platforms/)
[![Android](https://img.shields.io/badge/Android-3DDC84?logo=android&logoColor=white&style=flat-square)](https://tauritavern.github.io/en/downloads/platforms/)
[![iOS TestFlight](https://img.shields.io/badge/iOS-TestFlight-0D96F6?logo=apple&logoColor=white&style=flat-square)](https://testflight.apple.com/join/gpqAdeTm)

[すべてのプラットフォーム](https://tauritavern.github.io/en/downloads/platforms/) · [GitHub Releases](https://github.com/Darkatse/TauriTavern/releases)

</div>

<details>
<summary><b>📦 パッケージマネージャーでインストール</b>（Windows · macOS · Linux）</summary>

### Windows · WinGet

PowerShellで次を実行します。

```powershell
winget install --id TauriTavern.TauriTavern
```

### Windows · Scoop

PowerShellで次を実行します。

```powershell
scoop bucket add Darkatse https://github.com/Darkatse/Scoop-Darkatse.git
scoop install Darkatse/TauriTavern
```

### macOS · Homebrew

ターミナルで次を実行します。

```sh
brew install --cask tauritavern
```

### Linux

**Arch Linux · AUR**

お使いのAURヘルパーで[`tauritavern-bin`](https://aur.archlinux.org/packages/tauritavern-bin)をインストールします。

```sh
yay -S tauritavern-bin
```

このパッケージは[@LX2000WASD](https://github.com/LX2000WASD)さんが管理しています。[TauriTavern-aur](https://github.com/LX2000WASD/TauriTavern-aur)での継続的なメンテナンスに感謝します。

**Debian · Ubuntu · Fedora · openSUSE · NixOS**

スクリプトがシステムを判別し、適切な方法でインストールします。Nixが導入済みであれば、ほかのLinux環境でも利用できます。

**安定版**

```sh
curl -fsSL https://raw.githubusercontent.com/Darkatse/TauriTavern/main/scripts/install-linux.sh | sh
```

**Nix / NixOS**

Nixが導入済みの場合は、TauriTavernをユーザープロファイルへ直接追加できます。

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

Canary版は毎日更新され、最新の機能や修正が含まれますが、安定性は安定版に及ばない場合があります。新しいバージョンを試したい場合や、安定版で問題が発生している場合は、Canary版でその問題が修正されているか確認してみてください。

Windows、macOS、モバイル向けビルドは[Canary Release](https://github.com/Darkatse/TauriTavern/releases/tag/Canary)から入手できます。

<details>
<summary><b>Linux向けCanaryのインストール</b></summary>

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
> **iOS**： [公開TestFlightベータ](https://testflight.apple.com/join/gpqAdeTm)からインストールできます。iOS 15.0以降が必要です。iOS 15.0〜16.3は限定サポートで、完全サポートはiOS 16.4以降です。TestFlight版にはAppleのTestFlight規約と利用上の制限が適用されます。
>
> **Windowsポータブル版**：システムにWebView2ランタイムが必要です。

## TauriTavernとは

TauriTavernは、[SillyTavern](https://github.com/SillyTavern/SillyTavern)をネイティブアプリとして移植したプロジェクトです。フロントエンドは上流の使用感を保ったまま1.18.0へ同期し、バックエンドはNode.jsからRust（Tauri v2）で作り直しています。

Node.jsのインストールやコマンドライン操作は必要ありません。ダウンロードして、そのまま起動できます。キャラクターカード、チャット、プリセット、世界情報、フロントエンド拡張はSillyTavernと互換性があります。

> 注：TauriTavernは独立して管理されているオープンソースプロジェクトであり、SillyTavernの公式クライアントではありません。AGPL-3.0ライセンスのもと、無償で公開されています。利用前にライセンス条項をご確認ください。

## 主な機能

- 🖥️ **5つのネイティブプラットフォーム**：Windows、macOS、Linux、Android、iOSで同じように利用できます
- 🎭 **SillyTavernとの互換性**：フロントエンドは上流1.18.0と同期し、データ形式とディレクトリ構成にも対応
- 🧩 **フロントエンド拡張**：ネイティブGitを内蔵し、画面上でインストール、更新、ブランチ切り替えを行えます（Node.js専用のバックエンドプラグインは非対応）
- 🔄 **端末間同期**：暗号化されたLANペアリング、またはTT-Sync v2を使ったリモート自動アップロードに対応
- 🤖 **Agentフレームワーク**：ツール呼び出し、Skills、サブエージェント、実行タイムラインを備えています。開発は継続中です
- 📦 **簡単な移行**：SillyTavern用エクスポートスクリプトとアプリ内インポートを用意
- ⚡ **動作の最適化**：段階的な起動処理とチャットのウィンドウ読み込みにより、長いチャットでも操作性を保ちます
- 🔒 **ユーザーデータを手元で管理**：データは端末内に保存され、必要に応じてポータブルモードも選べます

## スクリーンショット

<div align="center">
<img src="docs/images/tauritavern-multidevice-cutout.webp" alt="デスクトップとモバイルで動作するTauriTavern" width="760">
</div>

## アーキテクチャ概要

Rustバックエンドは、Clean Architectureに基づくCargoワークスペース（`src-tauri/crates/`）として構成されています。

- `tauritavern`：Tauriホスト、コマンド層、コンポジションルート
- `tt-application` · `tt-ports` · `tt-domain` · `tt-contracts`：ユースケース、ポート、ドメインモデル、crate間の契約
- `tt-adapter-*`：ストレージ、HTTP、メディア、同期、拡張、トークナイザーの実装

フロントエンドは上流SillyTavernにモジュール化されたTauri注入レイヤー（`src/tauri/main/`）を組み合わせ、`window.__TAURITAVERN__`プラットフォームABIを通じてRustバックエンドと通信します。詳しくは[docs/BackendStructure.md](docs/BackendStructure.md)と[docs/FrontendGuide.md](docs/FrontendGuide.md)をご覧ください。

<details>
<summary><b>🛠 開発</b>（前提環境 · 主なコマンド · Tauri Pilot · ポータブルビルド · FasTools）</summary>

**前提環境**：Rust stable（edition 2024）· Node.js 22.13以降 · pnpm 11 · Tauri CLI

```bash
git clone https://github.com/Darkatse/TauriTavern.git
cd TauriTavern
pnpm install
```

**主なコマンド**：

```bash
pnpm run check         # フロントエンドのガードレール、型、契約とRust開発チェック
pnpm run web:build     # フロントエンドバンドルをビルド（Rspack）
pnpm run tauri:dev     # デスクトップ開発モード
pnpm run tauri:build   # デスクトップ用インストーラーをビルド
pnpm run android:dev   # Android開発モード
pnpm run ios:dev       # iOS開発モード
```

**Tauri Pilot（AIエージェントを使ったUI開発）**

このプロジェクトには、開発時にのみ有効になる[Tauri Pilot](https://github.com/mpiton/tauri-pilot)プラグインと権限設定が含まれています。アクセシビリティのスナップショットを通じて、AIエージェントがデスクトップWebViewを確認・操作できます。通常の開発コマンドとリリースビルドでは有効になりません。

```bash
cargo install tauri-pilot-cli  # 初回のみ必要
pnpm run tauri:dev:pilot
```

アプリを起動したら、別のターミナルで次の基本手順を実行します。

```bash
tauri-pilot ping
tauri-pilot snapshot -i
tauri-pilot click @e3          # 現在のスナップショットにあるrefを使用
tauri-pilot diff -i
tauri-pilot logs --level error
```

操作前にスナップショットを取得し、一度に1つずつ操作してください。非同期の更新後は`wait`を使い、確認には`assert`を優先します。MCPに対応したエージェントでは、`tauri-pilot mcp`をstdioサーバーとして登録できます。

**ポータブルビルド**：`pnpm run tauri:build:portable`（`release/`へ出力）。`TAURITAVERN_RUNTIME_MODE=portable`または`portable.flag`ファイルを使うと、ポータブルランタイムを明示的に有効化できます。

**FasTools**：開発と配布時の問題調査に推奨しているツールセットです。`pnpm run fastools:build`でビルドし、`pnpm run fastools:run`で実行します。

各プラットフォームについては、[docs/AndroidDevelopment.md](docs/AndroidDevelopment.md)と[docs/iOSDevelopment.md](docs/iOSDevelopment.md)をご覧ください。

</details>

## ドキュメント

- 📖 [オンラインドキュメント](https://tauritavern.github.io/en/)：使い方、Agent、アーキテクチャ、API、ダウンロードを扱う中英2言語のガイド
- [docs/FrontendGuide.md](docs/FrontendGuide.md)：フロントエンドのアーキテクチャと拡張ガイド
- [docs/FrontendHostContract.md](docs/FrontendHostContract.md)：公開ホストレイヤーの契約
- [docs/BackendStructure.md](docs/BackendStructure.md)：バックエンドのClean Architectureとcrate境界
- [docs/CurrentState/](docs/CurrentState/README.md)：実装済みモジュールの現状

## コントリビューション

IssueとPull Requestを受け付けています。最初に[CONTRIBUTING.md](CONTRIBUTING.md)を読み、緊急の修正を除いて`dev`ブランチを対象にしてください。

## 謝辞とライセンス

[SillyTavern](https://github.com/SillyTavern/SillyTavern)と[Tauri](https://tauri.app/)を基盤として開発しています。[Cocktail](https://github.com/Lianues/cocktail)、[Tavern-Helper](https://github.com/N0VI028/JS-Slash-Runner)、[LittleWhiteBox](https://github.com/RT15548/LittleWhiteBox)、[MikTik](https://github.com/Darkatse/MikTik)、そして[TauriTavern AURパッケージ](https://github.com/LX2000WASD/TauriTavern-aur)を管理している[@LX2000WASD](https://github.com/LX2000WASD)さんに感謝します。

[AGPL-3.0](LICENSE)（SillyTavernと同系統のライセンス）のもとで公開しています。

[![Contributors](https://contrib.rocks/image?repo=Darkatse/TauriTavern)](https://github.com/Darkatse/TauriTavern/graphs/contributors)

<p align="center"><sub><em>TauriTavernチームが心を込めて開発しています ❤️</em></sub></p>
