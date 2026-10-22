---
name: tt-docs
description: TauriTavern's own documentation for this version — the page API (window.__TAURITAVERN__.api), frontend structure, extension support, chat rendering, logs, custom endpoints, sync, data directory, updates, iOS and mobile limits, and the in-app assistant. Open its index when another TauriTavern skill points here, or when you need the exact behavior of a TauriTavern API or feature.
license: AGPL-3.0
---

# TauriTavern documentation

These are TauriTavern's own documents, from the same source revision as this app. They are written in Chinese. The layout under `references/` mirrors the project's `docs/` folder, so links between the documents listed here resolve; links to any other document point to files that are not included.

## Page API (`window.__TAURITAVERN__.api`)

- `references/API/README.md` — Start here for the page API: how to wait for the host to be ready, and which module covers what.
- `references/API/Chat.md` — `api.chat`: a handle on the current or another chat that reads its saved messages from disk (paging back from the end, finding the last matching message, searching text) and keeps per-chat data. It does not switch the visible chat; the open chat is in memory as `SillyTavern.getContext().chat`.
- `references/API/WorldInfo.md` — `api.worldInfo`: which world info entries the last generation activated, subscribing to activations, and opening an entry in the editor.
- `references/API/Dev.md` — `api.dev`: frontend logs, backend logs, LLM API request and response logs, and the debug bundle export.
- `references/API/Macros.md` — Macros through `SillyTavern.getContext().macros`: registering macros and evaluating text against the live context or a captured one.
- `references/API/ChatSurface.md` — ChatSurface: how chat messages are rendered and virtualized, the renderer participants that JS-Slash-Runner and LittleWhiteBox register, and its errors and events.
- `references/API/Layout.md` — `api.layout`: the content viewport, system bar areas, and screen capture.
- `references/API/Extension.md` — `api.extension.store`: the key-value and blob storage extensions use for their data.
- `references/API/Migration.md` — How an extension can use TauriTavern's optional chat APIs (finding the last message, search, storage, history) instead of scanning `getContext().chat`.
- `references/API/LlmConnections.md` — `api.llmConnections`: saved model connections that Agent profiles and Model Targets refer to.
- `references/API/Agent.md` — The Agent API: runs, Sessions, run history, profiles, tool catalogs, and registering extension tools.
- `references/API/Skill.md` — `api.skill`: listing, reading, importing and installing Skills, and Skill scopes.
- `references/API/MCP.md` — `api.mcp`: MCP server registrations, discovery, test calls, and how Agent runs use MCP tools.
- `references/API/Database.md` — `api.db`: the embedded database extensions use (nodes, graphs, text and vector search, TQL).

## How the frontend works

- `references/FrontendGuide.md` — How the frontend is put together: boot sequence, directory layout, how `/api/*` requests reach the Rust backend, chat payload and DOM boundaries, ChatSurface ownership, theme scope, Chat Completion parameters, and how third-party extensions are adapted.
- `references/FrontendHostContract.md` — What TauriTavern guarantees to pages and extensions: readiness signals, global objects such as `window.__TAURITAVERN__` and lodash `_`, request routing, compatibility patches, and the smoke tests that pin them down.

## Features and platforms

- `references/CurrentState/ThirdPartyExtensions.md` — Third-party extensions: install and update rules, local and global folders and which wins, supported compatibility, what is not supported (such as server plugins), and regression checks.
- `references/CurrentState/EmbeddedRuntime.md` — The embedded runtime that manages message iframes for JS-Slash-Runner and LittleWhiteBox: its modes, how it runs, and how to debug it.
- `references/CurrentState/LoggingObservability.md` — Where logs go, how errors reach the user as notifications, and the developer observability tools.
- `references/CurrentState/UserEndpointAccess.md` — Custom API endpoints (custom URL, reverse proxy): the one-time native approval dialog, where approvals are stored, and what cancelling does.
- `references/CurrentState/Sync.md` — LAN Sync and TT-Sync: what data syncs, which settings sections sync independently, discovery and pairing, and automatic sync.
- `references/CurrentState/DataDirectorySelection.md` — Choosing or moving the data directory: what is supported and what is not.
- `references/CurrentState/UpdateChannels.md` — Stable and Canary update channels, how updates are detected, and public TestFlight builds.
- `references/CurrentState/iOSPolicy.md` — Features restricted in iOS builds: policy profiles and capabilities, how they are applied, and their known limits.
- `references/CurrentState/MobileStyleAdaptation.md` — The mobile content viewport and window background on Android, iOS and HarmonyOS, and where styles differ from upstream.
- `references/CurrentState/TTS.md` — System text-to-speech on Android and the built-in System TTS provider.
- `references/CurrentState/CharacterIdentityContract.md` — How characters keep their identity across renames and imports, how a character's chats are found, and how a card's embedded world info is adopted.
- `references/CurrentState/InAppAssistant.md` — The in-app assistant itself: its profile and lifecycle, Sessions and history, interface, and the boundaries of its tools.
