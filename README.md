# TauriTavern

TauriTavern ports SillyTavern into a native desktop app with Tauri v2 + Rust backend while keeping the upstream frontend experience. The frontend is now synced to SillyTavern 1.15.0 and integrated through a modular Tauri injection layer.

## Highlights

- Native desktop runtime on Windows, macOS, Linux (Tauri v2)
- Rust backend with clean architecture layering
- Frontend compatibility with SillyTavern 1.15.0
- Modular request injection pipeline (`src/tauri/main/*`) replacing the previous monolithic `tauri-main.js`
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
2. `src/lib.js` statically imports `src/dist/lib.bundle.js` and re-exports a stable ESM library surface
3. `src/tauri-main.js` delegates to `bootstrapTauriMain()`
4. `src/tauri/main/bootstrap.js` creates context/router/interceptors, then initializes bridge and runtime helpers

## Frontend Integration Layout

```text
src/
├── tauri-bridge.js              # low-level Tauri bridge (invoke/listen/convertFileSrc)
├── tauri-main.js                # thin bootstrap entry
├── init.js                      # startup orchestrator
├── lib.js                       # library facade (ESM exports)
├── dist/lib.bundle.js           # webpack-built vendor bundle
└── tauri/main/
    ├── bootstrap.js             # composition root
    ├── context.js               # shared state + domain helpers
    ├── http-utils.js            # request/response parsing helpers
    ├── interceptors.js          # fetch/jQuery ajax patching
    ├── router.js                # lightweight route registry
    └── routes/
        ├── system-routes.js
        ├── settings-routes.js
        ├── extensions-routes.js
        ├── resource-routes.js
        ├── character-routes.js
        └── chat-routes.js
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
pnpm run build       # build frontend bundles
pnpm run tauri:dev   # run desktop app in dev mode
pnpm run tauri:build # build release installers
```

## Documentation

- `docs/FrontendGuide.md`: frontend architecture and extension guide
- `docs/BackendStructure.md`: backend architecture details
- `docs/TechStack.md`: stack and integration choices
- `docs/ImplementationPlan.md`: roadmap and milestones

## License

AGPL-3.0 (same license family as SillyTavern).

## Acknowledgements

- [SillyTavern](https://github.com/SillyTavern/SillyTavern)
- [Tauri](https://tauri.app/)
