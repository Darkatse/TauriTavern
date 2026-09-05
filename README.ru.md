<div align="center">

<img src="docs/images/tauritavern-readme-hero.webp" alt="TauriTavern" width="720">

# TauriTavern

**SillyTavern в виде нативного приложения для компьютеров и мобильных устройств**

[简体中文](README.md) · [English](README.en.md) · [日本語](README.ja.md) · **Русский** · [Português (Brasil)](README.pt-BR.md)

[Скачать](https://tauritavern.github.io/en/downloads/) · [Документация](https://tauritavern.github.io/en/) · [Релизы](https://github.com/Darkatse/TauriTavern/releases) · [Задачи](https://github.com/Darkatse/TauriTavern/issues)

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

## Скачать

<div align="center">

[![⬇ Download TauriTavern](https://img.shields.io/badge/%E2%AC%87_Download-TauriTavern-1f9d96?style=for-the-badge)](https://tauritavern.github.io/en/downloads/)

**Определяет ваше устройство автоматически · последняя стабильная сборка в один клик**

[![Windows](https://custom-icon-badges.demolab.com/badge/Windows-0078D6?logo=windows11&logoColor=white&style=flat-square)](https://tauritavern.github.io/en/downloads/platforms/)
[![macOS](https://img.shields.io/badge/macOS-000000?style=flat-square&logo=apple&logoColor=white)](https://tauritavern.github.io/en/downloads/platforms/)
[![Linux](https://img.shields.io/badge/Linux-FCC624?logo=linux&logoColor=black&style=flat-square)](https://tauritavern.github.io/en/downloads/platforms/)
[![Android](https://img.shields.io/badge/Android-3DDC84?logo=android&logoColor=white&style=flat-square)](https://tauritavern.github.io/en/downloads/platforms/)
[![iOS TestFlight](https://img.shields.io/badge/iOS-TestFlight-0D96F6?logo=apple&logoColor=white&style=flat-square)](https://testflight.apple.com/join/gpqAdeTm)

[Все платформы](https://tauritavern.github.io/en/downloads/platforms/) · [Релизы на GitHub](https://github.com/Darkatse/TauriTavern/releases)

</div>

<details>
<summary><b>📦 Установка через менеджер пакетов</b> (Windows · macOS · Linux)</summary>

### Windows · WinGet

Выполните в PowerShell:

```powershell
winget install --id TauriTavern.TauriTavern
```

### Windows · Scoop

Выполните в PowerShell:

```powershell
scoop bucket add Darkatse https://github.com/Darkatse/Scoop-Darkatse.git
scoop install Darkatse/TauriTavern
```

### macOS · Homebrew

Выполните в терминале:

```sh
brew install --cask tauritavern
```

### Linux

**Arch Linux · AUR**

Установите [`tauritavern-bin`](https://aur.archlinux.org/packages/tauritavern-bin) с помощью предпочитаемого AUR-помощника:

```sh
yay -S tauritavern-bin
```

Этот пакет поддерживает [@LX2000WASD](https://github.com/LX2000WASD). Благодарим его за постоянную работу над [TauriTavern-aur](https://github.com/LX2000WASD/TauriTavern-aur).

**Debian · Ubuntu · Fedora · openSUSE · NixOS**

Скрипт определит вашу систему и выберет подходящий способ установки. Он также работает в других дистрибутивах Linux, если в них установлен Nix.

**Стабильная версия**

```sh
curl -fsSL https://raw.githubusercontent.com/Darkatse/TauriTavern/main/scripts/install-linux.sh | sh
```

**Nix / NixOS**

Если Nix уже установлен, добавьте TauriTavern прямо в профиль пользователя:

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

Canary обновляется ежедневно и включает новые функции и исправления, но может быть менее стабильной, чем стабильная версия. Если вы хотите попробовать последнюю сборку или столкнулись с проблемой в стабильной версии, проверьте, исправлена ли эта проблема в Canary.

Сборки для Windows, macOS и мобильных платформ доступны в разделе [Canary Release](https://github.com/Darkatse/TauriTavern/releases/tag/Canary).

<details>
<summary><b>Установка Canary в Linux</b></summary>

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
> **iOS**: установите приложение через [открытое тестирование в TestFlight](https://testflight.apple.com/join/gpqAdeTm). Требуется iOS 15.0 или новее. Для iOS 15.0–16.3 предоставляется ограниченная поддержка; полная поддержка начинается с iOS 16.4. На тестовые сборки распространяются правила и ограничения TestFlight.
>
> **Портативная версия для Windows**: в системе должен быть установлен WebView2 Runtime.

## Что такое TauriTavern

TauriTavern переносит [SillyTavern](https://github.com/SillyTavern/SillyTavern) в нативное приложение. Фронтенд сохраняет возможности исходного проекта и синхронизирован с версией 1.18.0, а бэкенд переписан с Node.js на Rust с использованием Tauri v2.

Устанавливать Node.js и работать с командной строкой не нужно: достаточно скачать и запустить приложение. Карточки персонажей, чаты, пресеты, информация о мире и расширения фронтенда совместимы с SillyTavern.

> Примечание: TauriTavern — независимо поддерживаемый проект с открытым исходным кодом, а не официальный клиент SillyTavern. Он распространяется бесплатно по лицензии AGPL-3.0. Перед использованием ознакомьтесь с условиями лицензии.

## Основные возможности

- 🖥️ **Пять нативных платформ**: Windows, macOS, Linux, Android и iOS с единым пользовательским интерфейсом
- 🎭 **Совместимость с SillyTavern**: фронтенд синхронизирован с версией 1.18.0, форматы данных и структура каталогов совместимы с исходным проектом
- 🧩 **Расширения фронтенда**: встроенный нативный Git позволяет устанавливать и обновлять расширения, а также переключать ветки прямо в интерфейсе (плагины бэкенда, рассчитанные только на Node.js, не поддерживаются)
- 🔄 **Синхронизация между устройствами**: зашифрованное сопряжение по локальной сети или автоматическая отправка через удалённый TT-Sync v2
- 🤖 **Agent Framework**: вызов инструментов, Skills, субагенты и временная шкала выполнения; разработка продолжается
- 📦 **Перенос данных**: скрипты экспорта из SillyTavern и импорт внутри приложения
- ⚡ **Работа с длинными чатами**: поэтапный запуск и загрузка только видимой части чата помогают интерфейсу сохранять отзывчивость
- 🔒 **Данные остаются у вас**: всё хранится на устройстве; при необходимости доступен портативный режим

## Снимки экрана

<div align="center">
<img src="docs/images/tauritavern-multidevice-cutout.webp" alt="TauriTavern на компьютере и мобильном устройстве" width="760">
</div>

## Кратко об архитектуре

Бэкенд на Rust организован как рабочее пространство Cargo (`src-tauri/crates/`) по принципам Clean Architecture:

- `tauritavern`: оболочка Tauri, слой команд и корень композиции
- `tt-application` · `tt-ports` · `tt-domain` · `tt-contracts`: сценарии использования, порты, доменные модели и контракты между crate
- `tt-adapter-*`: реализации хранилища, HTTP, мультимедиа, синхронизации, расширений и токенизации

Фронтенд состоит из исходного SillyTavern и модульного слоя интеграции с Tauri (`src/tauri/main/`). Он взаимодействует с бэкендом на Rust через платформенный ABI `window.__TAURITAVERN__`. Подробнее см. в [docs/BackendStructure.md](docs/BackendStructure.md) и [docs/FrontendGuide.md](docs/FrontendGuide.md).

<details>
<summary><b>🛠 Разработка</b> (требования · основные команды · Tauri Pilot · портативные сборки · FasTools)</summary>

**Требования**: Rust stable (edition 2024) · Node.js 22.13+ · pnpm 11 · Tauri CLI

```bash
git clone https://github.com/Darkatse/TauriTavern.git
cd TauriTavern
pnpm install
```

**Основные команды**:

```bash
pnpm run check         # проверки фронтенда, типов, контрактов и Rust-кода
pnpm run web:build     # сборка фронтенда с помощью Rspack
pnpm run tauri:dev     # режим разработки для компьютера
pnpm run tauri:build   # сборка установщиков для компьютера
pnpm run android:dev   # режим разработки Android
pnpm run ios:dev       # режим разработки iOS
```

**Tauri Pilot (разработка интерфейса с помощью ИИ-агентов)**

В проект входит плагин [Tauri Pilot](https://github.com/mpiton/tauri-pilot) с разрешениями, доступными только при разработке. Он позволяет ИИ-агенту проверять и управлять содержимым WebView для настольной версии через снимки доступности. Обычные команды разработки и сборки релизов его не включают.

```bash
cargo install tauri-pilot-cli  # требуется только один раз
pnpm run tauri:dev:pilot
```

После запуска приложения откройте другой терминал и выполните основные шаги:

```bash
tauri-pilot ping
tauri-pilot snapshot -i
tauri-pilot click @e3          # используйте ref из текущего снимка
tauri-pilot diff -i
tauri-pilot logs --level error
```

Перед взаимодействием сделайте снимок и выполняйте по одному действию за раз. После асинхронных обновлений используйте `wait`, а для проверки отдавайте предпочтение `assert`. Агент с поддержкой MCP может зарегистрировать `tauri-pilot mcp` как stdio-сервер.

**Портативные сборки**: `pnpm run tauri:build:portable` (результат сохраняется в `release/`). Чтобы принудительно включить портативный режим, задайте `TAURITAVERN_RUNTIME_MODE=portable` или создайте файл `portable.flag`.

**FasTools**: рекомендуемый набор инструментов для разработки и диагностики развёртывания. Сборка выполняется командой `pnpm run fastools:build`, запуск — `pnpm run fastools:run`.

Подробности для отдельных платформ приведены в [docs/AndroidDevelopment.md](docs/AndroidDevelopment.md) и [docs/iOSDevelopment.md](docs/iOSDevelopment.md).

</details>

## Документация

- 📖 [Документация на сайте](https://tauritavern.github.io/en/): руководства на китайском и английском языках по использованию, Agent, архитектуре, API и загрузке
- [docs/FrontendGuide.md](docs/FrontendGuide.md): архитектура фронтенда и руководство по расширениям
- [docs/FrontendHostContract.md](docs/FrontendHostContract.md): публичный контракт слоя хоста
- [docs/BackendStructure.md](docs/BackendStructure.md): Clean Architecture бэкенда и границы crate
- [docs/CurrentState/](docs/CurrentState/README.md): текущее состояние реализованных модулей

## Участие в разработке

Мы принимаем сообщения о проблемах и Pull Request. Перед началом прочитайте [CONTRIBUTING.md](CONTRIBUTING.md). Если исправление не срочное, выбирайте целевой веткой `dev`.

## Благодарности и лицензия

Проект основан на [SillyTavern](https://github.com/SillyTavern/SillyTavern) и [Tauri](https://tauri.app/). Благодарим авторов [Cocktail](https://github.com/Lianues/cocktail), [Tavern-Helper](https://github.com/N0VI028/JS-Slash-Runner), [LittleWhiteBox](https://github.com/RT15548/LittleWhiteBox), [MikTik](https://github.com/Darkatse/MikTik), а также [@LX2000WASD](https://github.com/LX2000WASD) за поддержку [пакета TauriTavern для AUR](https://github.com/LX2000WASD/TauriTavern-aur).

Проект распространяется по лицензии [AGPL-3.0](LICENSE), которая относится к тому же семейству лицензий, что и лицензия SillyTavern.

[![Contributors](https://contrib.rocks/image?repo=Darkatse/TauriTavern)](https://github.com/Darkatse/TauriTavern/graphs/contributors)

<p align="center"><sub><em>Сделано командой TauriTavern с вниманием к деталям ❤️</em></sub></p>
