# Feature map

Each feature lists the files that implement it, as served from the app, and the main functions or objects to search for. Upstream SillyTavern code is in `/script.js` and `/scripts/`; TauriTavern's host code is in `/tauri/main/`.

## Generation

- **Sending and generating**, in `/script.js`:
  - The send button `#send_but` calls `sendTextareaMessage`, which leads to `Generate`.
  - `Generate` builds and sends a generation; `sendMessageAsUser` adds the user's message.
  - Requests go through `sendGenerationRequest` and `sendStreamingRequest`; streaming is handled by `StreamingProcessor`.
  - The reply is processed by `cleanUpMessage` and stored by `saveReply`.
  - Extensions generate with `generateQuietPrompt` and `generateRaw`.
- **Agent Mode.** When it is on (`agentModeEnabled` in the Agent System settings), the request does not go to the model directly.
  - `sendTextareaMessage` gets the run options from `getAgentGenerationOptions` in `/scripts/tauritavern/agent/agent-generation-router.js`.
  - `Generate` builds the prompt as usual, then freezes the inputs and starts an Agent run with `startAgentRunFromGeneratedPrompt`.
  - The Rust Agent runtime calls the model; see `references/API/Agent.md` in the tt-docs skill.
- **Chat Completion requests**, in `/scripts/openai.js`:
  - `prepareOpenAIMessages` assembles the messages, using `populateChatCompletion` and `populateChatHistory`.
  - `sendOpenAIRequest` with `createGenerationParameters` sends them, and `getStreamingReply` reads the stream.
  - The page's `/api/backends/chat-completions/*` requests are answered by `/tauri/main/routes/ai-routes.js`, which calls the Rust backend.
- **Prompt Manager**: `/scripts/PromptManager.js` (`PromptManager`). The order of prompts is in `SillyTavern.getContext().chatCompletionSettings.prompt_order`.
- **Presets**: `/scripts/preset-manager.js` (`getPresetManager`, `PresetManager`).
- **Text Completion formatting** (not used by Chat Completion): instruct mode in `/scripts/instruct-mode.js`, system prompts in `/scripts/sysprompt.js`, and the context template's story string in `/scripts/power-user.js` (`renderStoryString`).

## World info

- **Scanning and placement**, in `/scripts/world-info.js`:
  - `checkWorldInfo` scans for entries, and `getWorldInfoPrompt` produces the text that goes into the prompt;
  - `getSortedEntries` gives the entries in order, `world_info_position` the positions and `world_info_logic` the key logic.
- **Loading and saving lorebooks**: `/scripts/world-info-persistence.js` (`loadWorldInfo`, `saveWorldInfo`), a TauriTavern file.
- **Last generation's activated entries**: `api.worldInfo`, documented in `references/API/WorldInfo.md` in the tt-docs skill.

## Regex

- **Engine**: `/scripts/extensions/regex/engine.js`. `getRegexedString` and `runRegexScript` apply the scripts, and `regex_placement` says where a script applies (user input, AI output, slash commands, world info, reasoning).
- **Editor**: `/scripts/extensions/regex/index.js`.

## Macros

- `substituteParams` in `/script.js` expands macros in text.
- The default engine is the legacy one in `/scripts/macros.js`. The new engine in `/scripts/macros/` (`MacroEngine`, `MacroRegistry`) is used when `powerUserSettings.experimental_macro_engine` is on.
- `SillyTavern.getContext().macros` is documented in `references/API/Macros.md` in the tt-docs skill.

## Slash commands (STscript)

- **Running**: `/scripts/slash-commands.js` (`executeSlashCommands`, `executeSlashCommandsWithOptions`).
- **Parsing and registration**: `/scripts/slash-commands/SlashCommandParser.js` (`addCommandObject`) and `/scripts/slash-commands/SlashCommand.js` (`fromProps`).

## Variables

- `/scripts/variables.js` (`getLocalVariable`, `setLocalVariable`, `getGlobalVariable`, `setGlobalVariable`).
- Local variables are stored in the chat metadata (`chatMetadata.variables`), global ones in `extensionSettings.variables.global`.

## Extensions

- **Loader**: `/scripts/extensions.js` (`activateExtensions`, `loadExtensionSettings`, `extension_settings`, `extensionNames`).
- **Asset loading**: `/scripts/extensions/runtime/asset-loader.js`. `/scripts/extensions/runtime/third-party-runtime.js` picks the flattened stylesheet for WebViews without CSS `@layer`.
- **Install, update and discovery routes**: `/tauri/main/routes/extensions-routes.js`.

## Settings and themes

- **SillyTavern settings**: `saveSettings` and `saveSettingsDebounced` in `/script.js`, written through `/scripts/tauri/setting/settings-persistence.js`.
- **Power-user settings and themes**: `/scripts/power-user.js` (`loadPowerUserSettings`, `applyTheme`).
- **TauriTavern settings**: the panel is `/scripts/tauri/setting/settings-app/SettingsApp.tsx`; values are read and saved through `/tauri-bridge.js`. The day and night theme switching is in `/tauri/main/services/dynamic-theme/`.

## Characters, groups and personas

- **Characters**, in `/script.js`: `getCharacters`, `getOneCharacter`, `selectCharacterById` and `importCharacter`. Host routes are in `/tauri/main/routes/character-routes.js`.
- **Group chats**: `/scripts/group-chats.js` (`generateGroupWrapper`, `getGroups`, `openGroupById`).
- **Personas**: `/scripts/personas.js` (`setUserAvatar`, `createPersona`, `setPersonaDescription`).
- **Tags**: `/scripts/tags.js`.
- **Quick replies**: `/scripts/extensions/quick-reply/index.js`, with the sets and replies in `/scripts/extensions/quick-reply/src/`.

## Chat display

- **Rendering**: `printMessages`, `addOneMessage` and `messageFormatting` in `/script.js`.
- **TauriTavern's ChatSurface**, which owns the chat area, bounds and virtualizes it, and hosts renderer extensions: `/tauri/main/services/chat-surface/` and `/tauri/main/adapters/chat-surface/`. Its page API is `/tauri/main/api/chat-surface.js`.
- **Prompt itemization**, the per-message prompt breakdown: `/scripts/itemized-prompts.js`.

## Shared pieces

- **Popups**: `/scripts/popup.js` (`Popup`, `callGenericPopup`).
- **Top bar drawers**: opening and closing, including closing the other drawers, is `doNavbarIconClick` in `/script.js`. Open state and accessibility attributes are in `/scripts/drawers.js`.
- **Events**: `/scripts/events.js` (`event_types`).
- **Extension context**: `/scripts/st-context.js` (`getContext`).
- **Translations**: `/scripts/i18n.js`, with locale files in `/locales/`.
