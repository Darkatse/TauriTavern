---
name: tt-frontend-map
description: Where TauriTavern's frontend code lives and how to read it from inside the app — the layers (upstream SillyTavern, TauriTavern host, first-party and third-party extensions), which file implements which feature, the event and context entry points, and how to fetch and search the code the user is running. Read it to find how something works in code or where a behavior comes from.
license: AGPL-3.0
---

# TauriTavern frontend map

The code you read from the page is the code the user is running.

## Reading the running code

- **Fetching.** The app serves its frontend files as they are in the source tree. From the page, `await (await fetch('/scripts/world-info.js')).text()` returns that file as the user runs it.
- **Missing paths.** In the installed app, a path that does not exist returns the app's HTML page with status 200, not a 404. Check that you got the file you asked for; for example, a `.js` file should not start with `<!DOCTYPE`.
- **`/api/` paths** are requests to the backend, not files.
- **Large files.** Search in the page and return only the matching lines with their numbers, then read the range around a hit:

  ```js
  const text = await (await fetch('/scripts/world-info.js')).text();
  return text.split('\n').flatMap((line, i) => /checkWorldInfo/.test(line) ? [`${i + 1}: ${line}`] : []);
  ```

- **Bundled code is minified.** Read the source each bundle is built from:
  - `/dist/lib.*` is built from `/lib-bundle-core.js`, `/lib-bundle-optional.js` and `/lib-bundle-editor.js`.
  - The `dist/` of `/scripts/extensions/agent-system/`, `/scripts/extensions/mcp-manager/` and `/scripts/extensions/in-app-agent/` is built from `src/index.tsx` or `src/index.ts` in the same folder, for example `/scripts/extensions/in-app-agent/src/index.ts`.
  - `/scripts/tauri/setting/dist/` is built from `/scripts/tauri/setting/settings-app/SettingsApp.tsx`, `/scripts/tauri/setting/dev-logs-app/DevLogsApp.tsx` and `/scripts/tauri/setting/sync-app/index.ts`.
- **Types.** `/types.d.ts` declares `window.__TAURITAVERN__` and the page API.
- **Third-party extensions** are served from `/scripts/extensions/third-party/<folder>/`. There a missing file is a real 404.
- **What actually reached the model.** The Rust backend is not readable from the page. For a request's final payload, read the LLM API log (tt-troubleshooting skill).

## Layers

- **Upstream SillyTavern.** `/script.js` holds the core, generation and chat; `/scripts/` holds the features; `/scripts/extensions/<name>/` holds the built-in extensions. TauriTavern changes some of these files in place.
- **TauriTavern host**, in `/tauri/main/`:
  - `bootstrap.js` starts the host and builds `window.__TAURITAVERN__`;
  - `routes/` answers the page's `/api/*` requests by calling the Rust backend, one `<domain>-routes.js` file per area;
  - `api/` holds the page API modules;
  - `services/`, `adapters/` and `kernel/` hold chat rendering (ChatSurface), the embedded runtime, logging and the rest;
  - `compat/` holds the mobile compatibility code.
- **TauriTavern frontend.**
  - `/scripts/tauri/`: TauriTavern settings, log viewers, sync, and chat and regex helpers.
  - `/scripts/tauritavern/`: the Agent integration, iOS policy and custom endpoints.
  - TauriTavern files among the upstream ones, such as `/scripts/world-info-persistence.js`.
- **First-party extensions**:
  - `/scripts/extensions/in-app-agent/` (this assistant);
  - `/scripts/extensions/agent-system/` (Agent profiles, Skills, runs);
  - `/scripts/extensions/mcp-manager/`;
  - `/scripts/extensions/tauritavern-version/`, `/scripts/extensions/data-migration/` and `/scripts/extensions/code-render/`.
- **Extension loading helpers**, used for every extension: `/scripts/extensions/runtime/`.
- **What the backend does**: the tt-docs skill. How the pieces fit together: `references/FrontendGuide.md` in that skill.

## Features

`references/features.md` maps each feature to its files and main functions. It covers generation, prompts and presets, world info, regex, macros, slash commands, variables, extensions, settings and themes, characters, groups, personas, quick replies and chat display. Read it before searching the code.

## Entry points

- **Events.** `event_types` is defined in `/scripts/events.js`. Subscribe with `SillyTavern.getContext().eventSource.on(SillyTavern.getContext().eventTypes.<NAME>, handler)`. Useful names:
  - `GENERATION_STARTED`, `GENERATION_ENDED`, `GENERATION_STOPPED`;
  - `MESSAGE_SENT`, `MESSAGE_RECEIVED`, `CHARACTER_MESSAGE_RENDERED`;
  - `CHAT_CHANGED`, `SETTINGS_UPDATED`, `WORLDINFO_SCAN_DONE`, `CHAT_COMPLETION_PROMPT_READY`, `APP_READY`.
- **Context.** `SillyTavern.getContext()`, defined in `/scripts/st-context.js`, is how extensions reach state and helpers.
- **Modules.** A module can be imported from the page, and you get the same instance the app uses. For example, `await import('/scripts/extensions.js')` gives the extension loader's exports.
