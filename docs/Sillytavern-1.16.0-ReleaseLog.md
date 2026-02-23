# SillyTavern 1.16.0
Note: The first-time startup on low-end devices may take longer due to the image metadata caching process.

## Backends
- NanoGPT: Enabled tool calling and reasoning effort support.
- OpenAI (and compatible): Added audio inlining support.
- Added Adaptive-P sampler settings for supported Text Completion backends.
- Gemini: Thought signatures can be disabled with a config.yaml setting.
- Pollinations: Updated to a new API; now requires an API key to use.
- Moonshot: Mapped thinking type to "Request reasoning" setting in the UI.
- Synchronized model lists for Claude and Z.AI.

## Features
- Improved naming pattern of branched chat files.
- Enhanced world duplication to use the current world name as a base.
- Improved performance of message rendering in large chats.
- Improved performance of chat file management dialog.
- Groups: Added tag filters to group members list.
- Background images can now save additional metadata like aspect ratio, dominant color, etc.
- Welcome Screen: Added the ability to pin recent chats to the top of the list.
- Docker: Improved build process with support for non-root container users.
- Server: Added CORS module configuration options to config.yaml.

## Macros
> Note: New features require "Experimental Macro Engine" to be enabled in user settings.

- Added autocomplete support for macros in most text inputs (hint: press Ctrl+Space to trigger autocomplete).
- Added a hint to enable the experimental macro engine if attempting to use new features with the legacy engine.
- Added scoped macros syntax.
- Added conditional if macro and preserve whitespace (#) flag.
- Added variable shorthands, comparison and assignment operators.
- Added {{hasExtension}} to check for active extensions.

## STscript
- Added /reroll-pick command to reroll {{pick}} macros in the current chat.
- Added /beep command to play a message notification sound.


## Extensions
- Added the ability to quickly toggle all third-party extensions on or off in the Extensions Manager.
- Image Generation:
  - Added image generation indicator toast and improved abort handling.
  - Added stable-diffusion.cpp backend support.
  - Added video generation for Z.AI backend.
  - Added reduced image prompt processing toggle.
  - Added the ability to rename styles and ComfyUI workflows.
- Vector Storage:
  - Added slash commands for interacting with vector storage settings.
  - Added NanoGPT as an embeddings provider option.
- TTS:
  - Added regex processing to remove unwanted parts from the input text.
  - Added Volcengine and GPT-SoVITS-adapter providers.
- Image Captioning: Added a model name input for Custom (OpenAI-compatible) backend.

## Bug Fixes
- Fixed path traversal vulnerability in several server endpoints.
- Fixed server CORS forwarding being available without authentication when CORS proxy is enabled.
- Fixed asset downloading feature to require a host whitelist match to prevent SSRF vulnerabilities.
- Fixed basic authentication password containing a colon character not working correctly.
- Fixed experimental macro engine being case-sensitive when checking for macro names.
- Fixed compatibility of the experimental macro engine with the STscript parser.
- Fixed tool calling sending user input while processing the tool response.
- Fixed logit bias calculation not using the "Best match" tokenizer.
- Fixed app attribution for OpenRouter image generation requests.
- Fixed itemized prompts not being updated when a message is deleted or moved.
- Fixed error message when the application tab is unloaded in Firefox.
- Fixed Google Translate bypassing the request proxy settings.
- Fixed swipe synchronization overwriting unresolved macros in greetings.

## Community updates
- Macros 2.0 (v0.4) - Add scoped macros (last arg can be scoped), {{if}} macro and macro flags (baseline implementation) by @Wolfsblvt in #4913
- Zai moonshot reverse proxy by @subzero5544 in #4923
- add new Tts adapter provider by @guoql666 in #4915
- updated claude prompt caching url by @underscorex86 in #4931
- Macros 2.0 (v0.5) - Add variable shorthand macros and variable support to {{if}} macro by @Wolfsblvt in #4933
- Added regex filter option to TTS extension by @ZhenyaPav in #4924
- Macros 2.0 [Fix] - Make macro name matching case-insensitive throughout the macro system by @Wolfsblvt in #4942
- Macros 2.0 (v0.5.1) - Delayed Macro Argument Resolution for {{if}} - Macro by @Wolfsblvt in #4934
- Add Adaptive-P settings by @Cohee1207 in #4945
- Wrap reloadCurrentChat into SimpleMutex by @Cohee1207 in #4944
- Macros 2.0 [➕Macros] - Add {{hasExtension}} macro, and refactor - extension lookup logic by @Wolfsblvt in #4948
- Macros 2.0 [Fix] - Macro display override uses alias name if relevant + preserve whitespaces for autocomplete closing tag by @Wolfsblvt in #4953
- Macros 2.0 (v0.5.2) - Onboarding for new Macro Engine by @Wolfsblvt in #4955
- Caption: Add custom model input field by @Cohee1207 in #4956
- Macros 2.0 (v0.6.6) - STscript compatibility by @Cohee1207 in #4957
- Audio inlining for OpenAI and Custom-compatible by @Cohee1207 in #4964
- Removed 87 redundant chat[chat.length - 1] lookups. by @DeclineThyself in #4963
- Fix user handle naming logic by @allen9441 in #4969
- "gradually replacing property access with a dot operator" by @DeclineThyself in #4965
- Optimize getGroupPastChats by @Cohee1207 in #4976
- Replace $.ajax with fetch by @Cohee1207 in #4978
- Macros 2.0 [Fix] - Fix macro evaluation to allow nested scoped macros in arguments by @Wolfsblvt in #4977
- Macros 2.0 [Fix] - Fix {{pick}} macro seeding inside delayed-resolution macros like {{if}} by @Wolfsblvt in #4986
- Fix: init macros before extensions by @Cohee1207 in #4988
- Enhance world duplication to use current world name as base by @Wolfsblvt in #4990
- Fix: don't call append media twice on swipe by @Cohee1207 in #4991
- Improve performance of printMessages by @Cohee1207 in #4979
- Feature: Enhanced Branch and Checkpoint Naming by @Wolfsblvt in #4993
- refactor/perf-printMessages #2: Removed getMessageFromTemplate by @DeclineThyself in #4983
- Show a page reload prompt on EME toggle by @Cohee1207 in #4994
- refactor/perf-printMessages #3: Extracted updateMessageItemizedPromptButton and getMessageHTML from addOneMessage to improve readability. by @DeclineThyself in #4984
- Adjust itemized prompts on message move/delete by @Cohee1207 in #5000
- Macros 2.0 (v0.7.0) -Variable Shorthand: New Operators & Lazy Evaluation by @Wolfsblvt in #4997
- refactor/perf-printMessages #4: Renamed newMessage to messageElement and newMessageId to messageId. by @DeclineThyself in #4985
- Add to list of Showdown block tags by @Cohee1207 in #4998
- Update Dockerfile by @Cohee1207 in #4954
- Suppress error messages when Firefox unloads the tab by @Cohee1207 in #5013
- feat(sd): Add Z.AI GLM-Image model support by @mschienbein in #5012
- Improved printMessages performance on large chats by reducing DOM updates. by @DeclineThyself in #4947
- Adaptive-P for llama.cpp llama-server by @Beinsezii in #4959
- Add taxon filter controls to Group Chat member list by @paradox460 in #5006
- Adaptive P Hotfix by @Beinsezii in #5022
- Adding Slash Commands for Vector Storage Extension by @adventchilde in #5008
- Gemini: Add config.yaml setting for thought signatures by @Cohee1207 in #5025
- Docker: Build Optimization and Enhanced Non-Root/Volumeless Support by @Pavdig in #5024
- feat(sd): Add generation status indicator and improve abort handling by @mschienbein in #5015
- Recent Chats: Add pin functionality by @Cohee1207 in #5030
- feat(docker): add robust healthcheck script by @Cohee1207 in #5028
- Fixes #4950:try to modify config.yaml at start instead of modify it by @san-tian in #5043
- adaptive_P for tabby by @Ph0rk0z in #5044
- Add APP_INIT event before hideLoader in initialization sequence, before APP_READY fires by @Wolfsblvt in #5051
- Add /reroll-pick command to reset {{pick}} macro by @Wolfsblvt in #5049
- Added 'dot-notation': ['error'] to .eslint.cjs by @DeclineThyself in #5042
- Clarified contribution guidelines for large PRs. by @DeclineThyself in #5032
- Allow reasoning edit to substitute macros on saving the reasoning by @Wolfsblvt in #5052
- Macros 2.0 (v0.7.3) - Variable Shorthand: Comparison Operators & Autocomplete Improvements by @Wolfsblvt in #5050
- Macros 2.0 - [Chore] Allow registration of aliases for existing macros by @Wolfsblvt in #5053
- Macros 2.0 (v0.7.1) - Macro Autocomplete everywhere by @Wolfsblvt in #5019
- Update Pollinations API by @Cohee1207 in #5060
- Expose character update APIs for extensions by @rdeforest in #5062
- Volcengine tts by @Crush0 in #5003
- Stable diffusion.cpp server support by @Jay4242 in #5074
- fix: welcome depth by @StageDog in #5077
- feat(openrouter): add model quantizations setting by @Brioch in #5080
- Refactor /search to use per-line async parsing by @Cohee1207 in #5085
- /image-metadata by @Vibecoder9000 in #4788
- Fix/Do not spam saveSettingsDebounced in AccountStorage by @leandrojofre in #5090
- Macros 2.0 - Improve Autocomplete edge cases on completing macros by @Wolfsblvt in #5093
- (chore) World Info slash commands do some console context warn loggings by @Wolfsblvt in #5096
- Add Minimal Prompt Processing option by @KrsityKu in #5095
- add option for claude-opus-4-6 by @LumiWasTaken in #5103
- Feat/Allow to bulk toggle all third-party extensions from Manage Extensions by @leandrojofre in #5094
- Backgrounds metadata population and frontend colors by @Vibecoder9000 in #5092
- Add clearData option to clearChat function by @DeclineThyself in #5091
- Sync OpenRouter providers list by @cloak1505 in #5110
- Background sort feature by @Vibecoder9000 in #5107
- Macros 2.0 - Optional scoped content + improved closing-tag autocomplete by @Wolfsblvt in #5117
- fix: sync swipes only when chat is not pristine to ensure macro resolution by @Cohee1207 in #5106
- fix: correct typo 'seperated' to 'separated' by @thecaptain789 in #5121
- Add rename buttons for ComfyUI workflows and style presets by @Copilot in #5124
- feat(server): make CORS middleware configurable by @awaae001 in #5123
- Preserve user input on tool call recursion by @Cohee1207 in #5134
- Set HTML lang attribute from app locale to enable CSS hyphens: auto by @Copilot in #5136
- Add "✨ Vibe Coded" label to PRs created by @Copilot by @Copilot in #5137
- Add GLM-5 to Z.AI model list by @Copilot in #5138
- Macros 2.0 - list-supported Macros Autocomplete Improvements by @Wolfsblvt in #5135
- Update zh-CN translations by @Tosd0 in #5145
- Add NanoGPT embeddings support for Vector Storage by @Copilot in #5150
- Fix: HTTP Basic Auth fails when password contains colons by @Hime-Hina in #5153
- Staging by @Cohee1207 in #5154