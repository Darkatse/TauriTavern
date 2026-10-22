---
name: tt-extensions
description: Third-party extensions in TauriTavern — installing, updating, enabling and inspecting them, what TauriTavern supports, and how JS-Slash-Runner (Tavern Helper, 酒馆助手), LittleWhiteBox (小白X), MagVarUpdate (MVU) and ST-Prompt-Template work here. Read it for any question or problem that involves an extension.
license: AGPL-3.0
---

# Extensions in TauriTavern

## How extensions run here

- **Page only.** An extension is a folder with a `manifest.json` and the scripts and styles it names, loaded into the page. Node server plugins cannot run, because there is no SillyTavern server.
- **Two install locations.** Local, for this user (`data/default-user/extensions/<folder>` in the data directory), and global (`data/extensions/third-party/<folder>`). When both hold the same folder, the local one wins.
- **Served path.** Either way, the extension's files are served at `/scripts/extensions/third-party/<folder>/`, and its internal name is `third-party/<folder>`.
- **Activation.** Third-party extensions activate once the app is ready. When Chat DOM Virtualization is on, the renderers (JS-Slash-Runner, LittleWhiteBox and any extension declaring a `chatSurface` hook) activate earlier, during startup, because they must register before the chat is first drawn.
- **Size limit.** On mobile, a file over 32 MiB is refused when it is loaded.

## Inspecting what is installed

From the page:

- **List**: `await (await fetch('/api/extensions/discover')).json()` returns `[{ name, type }]`, where `type` is `system`, `local` or `global`.
- **Manifest**: `SillyTavern.getContext().getExtensionManifest('<folder>')` gives the display name, `version`, `js`, `css`, `loading_order` and requirements.
- **Enabled or not**: disabled extensions are listed by full name (`third-party/<folder>`) in `SillyTavern.getContext().extensionSettings.disabledExtensions`.
- **Git state**: `fetch('/api/extensions/version', { method: 'POST', headers: SillyTavern.getContext().getRequestHeaders(), body: JSON.stringify({ extensionName: '<folder>', global: <true|false> }) })` returns the branch, commit, whether it is up to date, and the remote URL.
- **Settings**: `extensionSettings[<key>]`, where the extension chooses the key (see below).
- **Its own documentation**: read `README.md`, `docs/` or type definitions from `/scripts/extensions/third-party/<folder>/`. This copy matches the version the user has. A missing file returns 404.

## Install, update, enable, remove

- **Install.** Extensions drawer, then Install extension (`#third_party_extension_button`).
  - The user gives a Git URL and optionally a branch or tag, and can install for all users (global).
  - Only anonymous HTTP(S) Git works: no SSH, no credentials in the URL, no query string, no LFS or submodules, and no ZIP installs.
  - Without virtualization, a new extension activates at once. With it on, a new renderer such as JS-Slash-Runner or LittleWhiteBox works only after a reload.
- **Update, switch branch, move or delete.** These are per extension in Manage extensions (`#extensions_details`).
  - Updates and branch switches take effect after a reload; deleting reloads the page.
  - A folder copied in without `.git` loads but cannot update; reinstall it from Git.
- **Enable or disable.** Changes made in Manage extensions apply when it closes, and that reloads the page. Ask the user first.
- **Errors you may see**:
  - `Only HTTP and HTTPS Git remotes are supported`, `Authenticated Git remote URLs are not supported`, `Git remote URL must not contain a query or fragment`;
  - `Remote Git branch or tag does not exist: …`, `Extension already exists at '…'`;
  - `Extension manifest.json is missing`, `Invalid extension manifest.json: …`;
  - `Extension source metadata is missing. Reinstall this extension to enable updates.`;
  - `Another local mutation is already running`: another install, update or sync is still in progress; wait and retry.

## What TauriTavern provides

- **Compatibility.** lodash as `window._`; polyfills for older mobile WebViews; flattened CSS `@layer` where the WebView lacks it. The tt-troubleshooting skill lists the symptoms.
- **ChatSurface.** TauriTavern owns chat rendering.
  - JS-Slash-Runner and LittleWhiteBox register with ChatSurface as renderers, but only under their usual folder names, `JS-Slash-Runner` and `LittleWhiteBox`.
  - Chat DOM Virtualization (TauriTavern settings, Performance; off by default; saved as `chat_virtualization_enabled` in the TauriTavern settings) needs JS-Slash-Runner 4.9.1 or later and LittleWhiteBox 3.0.4 or later. It will not start while both have code block rendering on.
- **Embedded Runtime.** With virtualization off, it manages message iframes. Modes are Auto, Balanced, Power Saver and Off, in TauriTavern settings, Performance.
- **Code Render.** The built-in Code Render stays off while JS-Slash-Runner or LittleWhiteBox is enabled.
- **Long chats.** An extension that reads the chat from the page only sees the messages currently rendered; the full chat is `SillyTavern.getContext().chat`.
- **References** in the tt-docs skill: `references/CurrentState/ThirdPartyExtensions.md`, `references/FrontendHostContract.md`, `references/API/ChatSurface.md` and `references/CurrentState/EmbeddedRuntime.md`.

## Common extensions

### JS-Slash-Runner (Tavern Helper, 酒馆助手)

- **What it does.** It runs scripts and renders HTML "frontends" inside messages as iframes. It exposes `TavernHelper`, with functions for chat messages, characters, presets, world info, variables and generation.
- **Settings**: `extensionSettings.tavern_helper`.
  - `render.enabled` turns frontends on.
  - `render.depth`: above 0, only the last that many messages render frontends.
  - `render.allow_streaming`.
  - `script.scripts` holds the global scripts.
  - `script.enabled.characters` lists the characters whose own scripts are allowed to run.
- **Storage.** Character scripts are stored in the card (`data.extensions.tavern_helper`), preset scripts in the preset, and chat data in the chat metadata.
- **In the page.** A message frontend is a `div.TH-render` with an iframe whose id is `TH-message--<message id>--<n>`. Script iframes are hidden, with ids starting `TH-script--`.
- **API reference.** The type definitions in its folder, `@types/function/*.d.ts` and `@types/iframe/*.d.ts`, which include `exported.tavernhelper.d.ts`, `exported.mvu.d.ts` and `exported.ejstemplate.d.ts`.
- **With virtualization on**, streaming rendering (`render.allow_streaming`) is not supported and raises an error.

### LittleWhiteBox (小白X)

- **What it does.** It is a suite: iframe rendering, story summary and outline, variables, phone-style apps, image generation, TTS, an assistant, scheduled tasks, immersive mode and more.
- **Settings**: `extensionSettings.LittleWhiteBox`.
- **In the page**: `.xiaobaix-iframe-wrapper` containing `iframe.xiaobaix-iframe`.
- **TauriTavern integration**: in `integrations/tauritavern/` in its folder, with a README there.
  - With virtualization on, it refuses immersive mode, message preview and custom templates. Its error names the features still turned on (error code `TT_UNSUPPORTED_MANAGED_FEATURES`).
  - `TT_CHAT_SURFACE_UNAVAILABLE` means the ChatSurface API is missing or the wrong version.
- **Limit.** Its background image jobs need its Node server plugin, which TauriTavern cannot run.

### MagVarUpdate (MVU)

- **What it does.** It keeps variable state for a character card. The model writes updates in its reply, MVU applies them to message variables, and a status bar shows them.
- **How it loads.** It is not an extension. A JS-Slash-Runner script in the character card imports it from jsDelivr, as `…/gh/MagicalAstrogy/MagVarUpdate@<ref>/artifact/bundle.js`. The `<ref>` tells you the version.
- **What it depends on.**
  - JS-Slash-Runner, with the card's scripts allowed to run, and network access.
  - The status bar is usually a JS-Slash-Runner frontend, so everything that stops frontends also stops it.
- **What a card needs**:
  - a regex that removes `<UpdateVariable>…</UpdateVariable>` from what is shown and sent;
  - a regex that removes `<StatusPlaceHolderImpl/>` from what the model sees, so the placeholder still shows as the status bar;
  - a world info entry that gives the model `{{get_message_variable::stat_data}}`;
  - world info entries marked `[InitVar]` holding the initial values.
- **State.** It lives in message variables: `chat[i].variables[chat[i].swipe_id].stat_data`, with `schema` alongside. Updates come as `_.set(path, old, new)`, `_.add`, `_.assign`, `_.remove` or a `<JSONPatch>` block.
- **Checking it from the page.** `window.Mvu` exists once it has loaded, and `Mvu.getMvuData({ type: 'message', message_id: 'latest' })` returns the latest data.
- **Documentation.** It is in its repository at the same ref, for example `https://cdn.jsdelivr.net/gh/MagicalAstrogy/MagVarUpdate@<ref>/doc/tutorial.md`; also `supplementary-tutorial.md`, `template.md`, `generate.md` and `stat_var.md`. Types: JS-Slash-Runner's `@types/iframe/exported.mvu.d.ts`.
- **When the status bar does not update**, check in this order:
  1. Did the last reply contain an update at all (`<UpdateVariable>` or `_.set(`)? If not, the model did not follow the card's instructions; that is a prompt problem, not an MVU fault.
  2. Is `window.Mvu` present, and are the card's scripts allowed to run?
  3. Does the card have an `[InitVar]` entry, and is the variable world info entry present exactly once?
  4. Does the status bar frontend render at all (JS-Slash-Runner section above)?

### ST-Prompt-Template

- **What it does.** EJS templates (`<% %>`) in prompts, character cards, world info and messages, with `getvar` and `setvar`.
- **Settings**: `extensionSettings.EjsTemplate`. Global object: `EjsTemplate`.
- **Documentation** in its folder: `docs/features.md` and `docs/reference.md`, each with a `_cn` version.
