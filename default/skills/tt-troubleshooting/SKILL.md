---
name: tt-troubleshooting
description: Diagnosing problems in TauriTavern and reporting them — error messages, failed or cut-off replies, world info that does not trigger, extensions that fail to load, custom endpoints, sync, the data directory, iOS and mobile limits, and exporting a debug bundle for the developers. Read it when a user reports something broken or unexpected, before guessing at a cause.
license: AGPL-3.0
---

# Troubleshooting TauriTavern

## Evidence that disappears

Collect these first, because they are overwritten.

- **What went to the model** (`api.dev.llmApiLogs`).
  - By default only the last 5 requests are kept, and this assistant's own requests are logged too, so each step you take can push the user's request out. When the problem is a reply, read the log before anything else.
  - `index()` lists recent requests, oldest first, with `ok`, `model`, `endpoint`, `source`, `durationMs` and `stream`. Pick the entry by time, model and endpoint.
  - `getPreview(id)` returns a readable request and response plus `errorMessage`.
  - `getRaw(id)` returns the payload as sent, with inline media replaced by placeholders.
  - If the entry is gone, ask the user to reproduce. You can also offer to keep more entries with `setKeep(n)`, which is a saved setting.
- **Frontend logs** (`api.dev.frontendLogs`).
  - Window errors and unhandled promise rejections are always recorded.
  - Console output is recorded only while console capture is on, which is off by default. Turning it on is a saved setting: ask first, then have the user reproduce.
  - Entries are kept in memory, and warnings and errors share the last 100.

Reproducing a failed reply means generating in the user's chat. If the user has already described the failure, use the evidence above instead, and ask before sending anything into their chat.

## Other evidence

- **The chat itself.** A failed normal generation is saved as an assistant message holding the error text and the endpoint. Read it with `SillyTavern.getContext().chat.at(-1).mes`.
- **Backend logs.** `api.dev.backendLogs.tail({ limit })` returns up to the last 800 entries from the Rust side. Frontend entries are not included.
- **The state involved.** The relevant settings (tt-app-guide skill) and the installed extensions (tt-extensions skill).
- **References.** `references/API/Dev.md` and `references/CurrentState/LoggingObservability.md` in the tt-docs skill.

## Isolate

- **Extensions** cause many problems. Have the user retest with third-party extensions disabled, then re-enable them half at a time. Every round reloads the page, so agree on it with the user first.
- **Platform.** Older Android WebViews and iOS builds behave differently (below).

## Known problems

**Failed replies.** Match the `errorMessage` of the log entry:

- `Authentication error: …`: the key is wrong, missing or not allowed for this model.
- `… endpoint failed with status 429 …`: rate limit or quota.
- `Invalid data: model.context_overflow: …`: the prompt is too long for the model's context.
- Other `Invalid data: …` messages: the provider rejected the request, often because of a parameter.
- Timeout, DNS, TLS, proxy or connection errors: check the network, VPN, the request proxy (TauriTavern settings, System) and the endpoint address.
- Background generations: requests from extensions answer with an error instead of writing a message. When the app is not in front, the user may also get a system notification.

**Cut-off replies.** Compare the reply with the preset's maximum response length (`chatCompletionSettings.openai_max_tokens`, control `#openai_max_tokens`), and look for the provider's finish reason in the raw response.

**Custom endpoints**
- The first request to a new endpoint opens a native approval dialog. Cancelling gives `Endpoint authorization cancelled by user`, a normal cancel, not an error. Trying again shows the dialog again.
- An approval covers one exact scheme, host, port and path, so changing any of them asks again.
- `User-configured endpoint requires approval: <endpoint>` means a request reached the network without going through the dialog, for example an Agent run using an endpoint the user never approved in a chat.
- Reference: `references/CurrentState/UserEndpointAccess.md` in the tt-docs skill.

**World info that does not trigger**
- `api.worldInfo.getLastActivation()` returns the entries activated by the last real generation, or `null` if no generation has run since the page loaded. Dry runs such as token counting do not count.
- The world info scanner writes a debug-level line for each decision, starting with `[WI]`. With console capture on, the debug entries of the frontend logs show why each entry was or was not activated.
- Reference: `references/API/WorldInfo.md` in the tt-docs skill.

**Extensions that fail to load**
- The Manage extensions button (`#extensions_details`) changes to a warning color.
- Its list says which extension failed to load and why: a script that could not load, missing or disabled dependencies, or a required client version. The extension is named by its display name, and the text is in the user's language.
- Install and update errors, and which Git sources work: tt-extensions skill.

**Older mobile WebViews**
- `… .at is not a function` and similar: TauriTavern adds `at`, `findLast`, `toSorted`, `Object.hasOwn` and a few others. If `window.__TAURITAVERN_MOBILE_RUNTIME_COMPAT__ === true`, the polyfills are in place and the extension uses something newer.
- Missing extension styles: when the WebView lacks CSS `@layer`, TauriTavern serves a flattened copy, and the stylesheet URL carries `ttCompat=layer`.
- More notes: the extension and compatibility sections of `references/FrontendGuide.md` in the tt-docs skill.

**Chat rendering**
- **Virtualization stopped.** When Chat DOM Virtualization fails at startup or later, a popup says it stopped and shows the error. Common errors:
  - a renderer extension too old for it (JS-Slash-Runner needs 4.9.1, LittleWhiteBox 3.0.4);
  - both renderers with code block rendering on;
  - a LittleWhiteBox feature it cannot run (tt-extensions skill).
  - The popup offers to turn virtualization off and reload.
- **Iframe placeholders.** With virtualization off, placeholders or reload hints in message iframes come from the Embedded Runtime, whose mode is in TauriTavern settings, Performance. With virtualization on, the renderers draw same-height placeholders for messages outside the visible range.
- Reference: `references/CurrentState/EmbeddedRuntime.md` in the tt-docs skill.

**iOS TestFlight builds**
- The public TestFlight build turns off, among others:
  - third-party extensions;
  - custom endpoints and reverse proxies;
  - sources other than OpenAI, Claude and Google AI Studio;
  - image generation;
  - LAN sync.
- Errors read `iOS policy disabled capability: <key>` or `iOS policy disabled chat completion source: <source>`. This is by design, not a fault.
- `window.__TAURITAVERN__.iosPolicy?.profile` gives the policy of the device you are running on.
- Reference: `references/CurrentState/iOSPolicy.md` in the tt-docs skill.

**Sync**
- `Another local mutation is already running`: another sync or local operation is still in progress. Wait and retry.
- "Newer wins" depends on the devices' clocks agreeing.
- After a partial failure, the transferred files stay, and a retry rescans.
- Reference: `references/CurrentState/Sync.md` in the tt-docs skill.

**Data directory (desktop)**
- A new data directory must be an existing, empty, absolute path, and it takes effect after a restart.
- If moving fails, the app keeps using the old directory and says so.
- Reference: `references/CurrentState/DataDirectorySelection.md` in the tt-docs skill.

## Report

- **To the user.** Give the cause, the evidence (quote the log line or value), and the fix or next step. Say what you changed and how to undo it.
- **When the developers need to look.**
  - Start the debug bundle export with `api.dev.exportBundle()`. The user chooses where the file goes, and the call returns `false` if they cancel.
  - The bundle holds the settings files, the frontend and backend logs, and the kept LLM requests. Exported after a long conversation with you, those requests are mostly yours, so export it right after the problem happens.
  - The version panel in the Extensions drawer links to the project's GitHub and Discord, where the user can post it. Tell them to look through it before sharing.
- **Privacy.** Logs and requests contain the user's prompts and characters. Quote only what bears on the problem, and never repeat an API key or password.
