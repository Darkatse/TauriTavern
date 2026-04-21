# TauriTavern

[One-stop download link](https://tauritavern.github.io/downloads/)

TauriTavern ports SillyTavern into a native desktop app with Tauri v2 + Rust backend while keeping the upstream frontend experience. The frontend is now synced to SillyTavern 1.16.0 and integrated through a modular Tauri injection layer.

![TauriTavern hero](docs/images/tauritavern-readme-hero.png)

## Highlights

- Native desktop runtime on Windows, macOS, Linux (Tauri v2)
- Rust backend with clean architecture layering
- Frontend compatibility with SillyTavern 1.16.0
- Chat Completion providers: OpenAI, Claude, Gemini(MakerSuite), and Custom OpenAI-compatible endpoint
- Modular request injection pipeline (`src/tauri/main/*`), organized as a maintainable Host Kernel layering (`context/kernel/services/adapters/routes`)
- Stable platform ABI: `window.__TAURITAVERN__` + request tracing header: `x-tauritavern-trace-id`
- Engineering guardrails: strict type checks (`tsc -p tsconfig.host.json`) + dependency/line-budget rules (routes must not reference `window`)
- Unified frontend bootstrap pipeline without runtime loader indirection

## Architecture

### Backend (`src-tauri`)

- `presentation`: Tauri commands and API boundary
- `application`: use cases/services and DTO orchestration
- `domain`: core models, contracts, errors
- `infrastructure`: file persistence, repositories, logging

### Frontend (`src`)

- Upstream SillyTavern frontend code (HTML/CSS/JS)
- Tauri bridge and interception layer for replacing HTTP endpoints with local Tauri command calls

Frontend startup flow:

1. `src/init.js` loads `lib.js` -> `tauri-main.js` -> `script.js`
2. `src/lib.js` statically imports `src/dist/lib.core.bundle.js` and re-exports a stable ESM library surface (heavy/optional libs are loaded on demand via `getHljs()/getReadability()` from `src/dist/lib.optional.bundle.js`)
3. `src/tauri-main.js` delegates to `bootstrapTauriMain()`
4. `src/tauri/main/bootstrap.js` creates context/router/interceptors, installs the `window.__TAURITAVERN__` platform ABI, and injects a trace header for host-handled routes

## Frontend Integration Layout

```text
src/
├── tauri-bridge.js              # low-level Tauri bridge (invoke/listen/convertFileSrc)
├── tauri-main.js                # thin bootstrap entry
├── init.js                      # startup orchestrator
├── lib.js                       # library facade (ESM exports)
├── dist/lib.core.bundle.js      # webpack-built core vendor bundle (startup-critical)
├── dist/lib.optional.bundle.js  # webpack-built optional vendor bundle (on-demand)
└── tauri/main/
    ├── bootstrap.js             # composition root
    ├── context.js               # compatibility shim (re-export `context/index`)
    ├── context/                 # host kernel facade + types (stable contract)
    ├── kernel/                  # pure logic (policies/tracing/hash/...)
    ├── services/                # stateful capabilities (assets/thumbnails/characters/android…)
    ├── adapters/                # adapters touching window/DOM/upstream ST
    ├── http-utils.js            # request/response parsing helpers
    ├── interceptors.js          # fetch/jQuery ajax patching
    ├── router.js                # lightweight route registry
    └── routes/
        ├── system-routes.js
        ├── settings-routes.js
        ├── extensions-routes.js
        ├── resource-routes.js
        ├── character-routes.js
        ├── chat-routes.js
        └── ai-routes.js
```

## Development

Prerequisites:

- Rust stable
- Node.js 18+
- pnpm
- Tauri CLI

Setup:

```bash
git clone https://github.com/Darkatse/tauritavern.git
cd tauritavern
pnpm install
```

Common commands:

```bash
pnpm run check             # guardrails + host-kernel type checks (recommended)
pnpm run web:build         # build frontend bundles (webpack)
pnpm run dev           # desktop dev mode (alias of tauri:dev)
pnpm run tauri:dev     # desktop dev mode
pnpm run tauri:build   # build desktop installers
pnpm run android:dev   # Android dev mode
pnpm run ios:dev       # iOS dev mode
```

Portable build notes:

- `pnpm run tauri:build:portable` outputs to `release/` by default
- You can force portable runtime mode via `TAURITAVERN_RUNTIME_MODE=portable` or `portable.flag`
- On Windows, portable users must ensure WebView2 runtime is available

## FasTools (Debug Utility)

`fastools` is an useful toolkit that facilitates debugging during development and desktop deployment.

Build:

```bash
pnpm run fastools:build
```

Run:

- `pnpm run fastools:run`

If you prefer cargo directly, run from repository root:

```bash
cargo build --release --manifest-path fastools/Cargo.toml
cargo run --manifest-path fastools/Cargo.toml
```

## Documentation

- `docs/FrontendGuide.md`: frontend architecture and extension guide
- `docs/FrontendHostContract.md`: public host-kernel contract (keep stable during refactors)
- `docs/BackendStructure.md`: backend architecture details
- `docs/TechStack.md`: stack and integration choices
- `docs/ImplementationPlan.md`: roadmap and milestones

## License

AGPL-3.0 (same license family as SillyTavern).

## Acknowledgements

- [SillyTavern](https://github.com/SillyTavern/SillyTavern)
- [Tauri](https://tauri.app/)
- [Cocktail](https://github.com/Lianues/cocktail)
- [Tavern-Helper](https://github.com/N0VI028/JS-Slash-Runner)
- [LittleWhiteBox](https://github.com/RT15548/LittleWhiteBox)
- [MikTik](https://github.com/Darkatse/MikTik)
