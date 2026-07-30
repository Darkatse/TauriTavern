<div align="center">

<img src="docs/images/tauritavern-readme-hero.webp" alt="TauriTavern" width="720">

# TauriTavern

**SillyTavern como aplicativo nativo para computadores e dispositivos móveis**

[简体中文](README.md) · [English](README.en.md) · [日本語](README.ja.md) · [Русский](README.ru.md) · **Português (Brasil)**

[Downloads](https://tauritavern.github.io/en/downloads/) · [Documentação](https://tauritavern.github.io/en/) · [Releases](https://github.com/Darkatse/TauriTavern/releases) · [Issues](https://github.com/Darkatse/TauriTavern/issues)

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

## Baixar

<div align="center">

[![⬇ Download TauriTavern](https://img.shields.io/badge/%E2%AC%87_Download-TauriTavern-1f9d96?style=for-the-badge)](https://tauritavern.github.io/en/downloads/)

**Detecta seu dispositivo automaticamente · baixe a versão estável mais recente com um clique**

[![Windows](https://custom-icon-badges.demolab.com/badge/Windows-0078D6?logo=windows11&logoColor=white&style=flat-square)](https://tauritavern.github.io/en/downloads/platforms/)
[![macOS](https://img.shields.io/badge/macOS-000000?style=flat-square&logo=apple&logoColor=white)](https://tauritavern.github.io/en/downloads/platforms/)
[![Linux](https://img.shields.io/badge/Linux-FCC624?logo=linux&logoColor=black&style=flat-square)](https://tauritavern.github.io/en/downloads/platforms/)
[![Android](https://img.shields.io/badge/Android-3DDC84?logo=android&logoColor=white&style=flat-square)](https://tauritavern.github.io/en/downloads/platforms/)
[![iOS TestFlight](https://img.shields.io/badge/iOS-TestFlight-0D96F6?logo=apple&logoColor=white&style=flat-square)](https://testflight.apple.com/join/gpqAdeTm)

[Todas as plataformas](https://tauritavern.github.io/en/downloads/platforms/) · [Versões no GitHub](https://github.com/Darkatse/TauriTavern/releases)

</div>

<details>
<summary><b>📦 Instalação com um gerenciador de pacotes</b> (Windows · macOS · Linux · Canary)</summary>

### Windows · Scoop

Execute no PowerShell:

```powershell
scoop bucket add Darkatse https://github.com/Darkatse/Scoop-Darkatse.git
scoop install Darkatse/TauriTavern
```

### macOS · Homebrew

O cask do TauriTavern para Homebrew está aguardando análise. Até que seja aceito, use o botão acima para baixar o instalador do macOS.

[Acompanhar a análise no Homebrew](https://github.com/Homebrew/homebrew-cask/pull/275888)

### Linux

**Arch Linux · AUR**

Instale o [`tauritavern-bin`](https://aur.archlinux.org/packages/tauritavern-bin) com o AUR helper de sua preferência:

```sh
yay -S tauritavern-bin
```

Esse pacote é mantido por [@LX2000WASD](https://github.com/LX2000WASD). Agradecemos pelo trabalho contínuo no [TauriTavern-aur](https://github.com/LX2000WASD/TauriTavern-aur).

**Debian · Ubuntu · Fedora · openSUSE · NixOS**

O script identifica seu sistema e escolhe o método de instalação adequado. Ele também funciona em outros sistemas Linux que já tenham o Nix instalado.

**Versão estável**

```sh
curl -fsSL https://raw.githubusercontent.com/Darkatse/TauriTavern/main/scripts/install-linux.sh | sh
```

**Canary**

```sh
curl -fsSL https://raw.githubusercontent.com/Darkatse/TauriTavern/main/scripts/install-linux.sh \
  | sh -s -- --channel canary
```

**Nix / NixOS**

Se o Nix já estiver instalado, adicione o TauriTavern diretamente ao seu perfil de usuário:

```sh
# Versão estável
nix profile add github:Darkatse/TauriTavern#tauritavern

# Canary
nix profile add github:Darkatse/TauriTavern/Canary#canary
```

**Flatpak**

```sh
flatpak remote-add --user --if-not-exists \
  tauritavern \
  https://flatpak.tauritavern.com/tauritavern.flatpakrepo
flatpak install --user tauritavern com.tauritavern.client
```

A versão Canary contém as mudanças mais recentes, mas pode ser menos estável. As compilações para Windows, macOS e dispositivos móveis estão disponíveis na página da [Canary Release](https://github.com/Darkatse/TauriTavern/releases/tag/Canary).

</details>

> [!TIP]
> **iOS**: instale pelo [teste público no TestFlight](https://testflight.apple.com/join/gpqAdeTm). É necessário ter o iOS 16 ou mais recente. As compilações estão sujeitas às regras e limitações do TestFlight.
>
> **Versão portátil para Windows**: requer o WebView2 Runtime instalado no sistema.

## O que é o TauriTavern

O TauriTavern transforma o [SillyTavern](https://github.com/SillyTavern/SillyTavern) em um aplicativo nativo. O frontend mantém a experiência do projeto original e está sincronizado com a versão 1.18.0, enquanto o backend foi reescrito de Node.js para Rust com Tauri v2.

Não é preciso instalar o Node.js nem usar a linha de comando: basta baixar e executar. Seus cartões de personagem, chats, predefinições, Informações do Mundo e extensões de frontend continuam compatíveis com o SillyTavern.

> Observação: o TauriTavern é um projeto independente de código aberto, não um cliente oficial do SillyTavern. Ele é gratuito e distribuído sob a licença AGPL-3.0. Consulte os termos da licença antes de usar.

## Principais recursos

- 🖥️ **Cinco plataformas nativas**: Windows, macOS, Linux, Android e iOS com a mesma experiência de uso
- 🎭 **Compatibilidade com o SillyTavern**: o frontend está sincronizado com a versão 1.18.0, incluindo formatos de dados e estrutura de diretórios compatíveis
- 🧩 **Ecossistema de extensões do frontend**: o Git nativo integrado permite instalar, atualizar e trocar de branch pela própria interface (plugins de backend exclusivos para Node.js não são compatíveis)
- 🔄 **Sincronização entre dispositivos**: pareamento criptografado pela rede local ou envio automático pelo TT-Sync v2 remoto
- 🤖 **Agent Framework**: chamadas de ferramentas, Skills, subagentes e linha do tempo de execução; o desenvolvimento continua
- 📦 **Migração direta**: scripts de exportação do SillyTavern e importação dentro do aplicativo
- ⚡ **Otimizações de desempenho**: inicialização em etapas e carregamento em janelas mantêm a interface responsiva mesmo em conversas longas
- 🔒 **Seus dados permanecem com você**: tudo fica armazenado no dispositivo, com um modo portátil opcional

## Capturas de tela

<div align="center">
<img src="docs/images/tauritavern-multidevice-cutout.webp" alt="TauriTavern em computadores e dispositivos móveis" width="760">
</div>

## Visão geral da arquitetura

O backend em Rust é organizado como um workspace Cargo (`src-tauri/crates/`) baseado em Clean Architecture:

- `tauritavern`: host Tauri, camada de comandos e raiz de composição
- `tt-application` · `tt-ports` · `tt-domain` · `tt-contracts`: casos de uso, portas, modelos de domínio e contratos entre crates
- `tt-adapter-*`: implementações de armazenamento, HTTP, mídia, sincronização, extensões e tokenização

O frontend combina o SillyTavern original com uma camada modular de integração com o Tauri (`src/tauri/main/`). Ele se comunica com o backend em Rust pela ABI de plataforma `window.__TAURITAVERN__`. Consulte [docs/BackendStructure.md](docs/BackendStructure.md) e [docs/FrontendGuide.md](docs/FrontendGuide.md) para saber mais.

<details>
<summary><b>🛠 Desenvolvimento</b> (pré-requisitos · comandos comuns · Tauri Pilot · builds portáteis · FasTools)</summary>

**Pré-requisitos**: Rust stable (edition 2024) · Node.js 20.19.x ou 22.12+ · pnpm · Tauri CLI

```bash
git clone https://github.com/Darkatse/TauriTavern.git
cd TauriTavern
pnpm install
```

**Comandos comuns**:

```bash
pnpm run check         # verificações de frontend, tipos, contratos e código Rust
pnpm run web:build     # gera os bundles do frontend com Rspack
pnpm run tauri:dev     # modo de desenvolvimento para desktop
pnpm run tauri:build   # compila os instaladores para desktop
pnpm run android:dev   # modo de desenvolvimento para Android
pnpm run ios:dev       # modo de desenvolvimento para iOS
```

**Tauri Pilot (desenvolvimento de frontend com agentes de IA)**

O projeto inclui o plugin [Tauri Pilot](https://github.com/mpiton/tauri-pilot) e suas permissões somente para desenvolvimento. Com ele, um agente de IA pode inspecionar e operar o WebView para desktop por meio de snapshots de acessibilidade. Os comandos normais de desenvolvimento e as compilações de lançamento não ativam esse recurso.

```bash
cargo install tauri-pilot-cli  # necessário apenas uma vez
pnpm run tauri:dev:pilot
```

Depois de iniciar o aplicativo, use outro terminal para executar o fluxo básico:

```bash
tauri-pilot ping
tauri-pilot snapshot -i
tauri-pilot click @e3          # use um ref do snapshot atual
tauri-pilot diff -i
tauri-pilot logs --level error
```

Tire um snapshot antes de interagir e execute uma ação por vez. Use `wait` após atualizações assíncronas e prefira `assert` para verificar o resultado. Um agente compatível com MCP pode registrar `tauri-pilot mcp` como servidor stdio.

**Builds portáteis**: `pnpm run tauri:build:portable` (gera os arquivos em `release/`). Para forçar o modo portátil, use `TAURITAVERN_RUNTIME_MODE=portable` ou crie um arquivo `portable.flag`.

**FasTools**: conjunto recomendado de ferramentas para desenvolvimento e diagnóstico de distribuição. Compile com `pnpm run fastools:build` e execute com `pnpm run fastools:run`.

Os detalhes de cada plataforma estão em [docs/AndroidDevelopment.md](docs/AndroidDevelopment.md) e [docs/iOSDevelopment.md](docs/iOSDevelopment.md).

</details>

## Documentação

- 📖 [Documentação on-line](https://tauritavern.github.io/en/): guias em chinês e inglês sobre uso, Agent, arquitetura, API e downloads
- [docs/FrontendGuide.md](docs/FrontendGuide.md): arquitetura do frontend e guia de extensões
- [docs/FrontendHostContract.md](docs/FrontendHostContract.md): contrato público da camada de host
- [docs/BackendStructure.md](docs/BackendStructure.md): Clean Architecture do backend e limites entre crates
- [docs/CurrentState/](docs/CurrentState/README.md): estado atual dos módulos implementados

## Como contribuir

Issues e Pull Requests são bem-vindos. Leia primeiro o [CONTRIBUTING.md](CONTRIBUTING.md) e, exceto em correções urgentes, use a branch `dev` como destino.

## Agradecimentos e licença

O projeto foi criado a partir do [SillyTavern](https://github.com/SillyTavern/SillyTavern) e do [Tauri](https://tauri.app/). Agradecemos aos responsáveis por [Cocktail](https://github.com/Lianues/cocktail), [Tavern-Helper](https://github.com/N0VI028/JS-Slash-Runner), [LittleWhiteBox](https://github.com/RT15548/LittleWhiteBox), [MikTik](https://github.com/Darkatse/MikTik) e a [@LX2000WASD](https://github.com/LX2000WASD), que mantém o [pacote do TauriTavern para AUR](https://github.com/LX2000WASD/TauriTavern-aur).

Distribuído sob a licença [AGPL-3.0](LICENSE), da mesma família de licenças usada pelo SillyTavern.

[![Contributors](https://contrib.rocks/image?repo=Darkatse/TauriTavern)](https://github.com/Darkatse/TauriTavern/graphs/contributors)

<p align="center"><sub><em>Feito com cuidado pela equipe do TauriTavern ❤️</em></sub></p>
