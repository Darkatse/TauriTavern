/**
 * Shared typedefs for the structured macro environment object (MacroEnv)
 * used by the macro engine, registry, env builder, and macro definition
 * modules. This file intentionally only contains JSDoc typedefs so that
 * it can be imported purely for type information from multiple modules
 * without creating runtime dependencies.
 */

/** @typedef {import('./MacroRegistry.js').MacroHandler} MacroHandler */
/** @typedef {import('./MacroRegistry.js').MacroDefinitionOptions} MacroDefinitionOptions */

/**
 * A dynamic macro value can be:
 * - A string (direct value)
 * - A MacroHandler function (resolved at runtime)
 * - A MacroDefinitionOptions object (full macro definition with handler, args, etc.)
 * @typedef {string | MacroHandler | MacroDefinitionOptions} DynamicMacroValue
 */

/**
 * @typedef {Object} MacroEnvNames
 * @property {string} user
 * @property {string} char
 * @property {string} group
 * @property {string} groupNotMuted
 * @property {string} notChar
 */

/**
 * @typedef {Object} MacroEnvCharacter
 * @property {string} [description]
 * @property {string} [personality]
 * @property {string} [scenario]
 * @property {string} [persona]
 * @property {string} [charPrompt]
 * @property {string} [charInstruction]
 * @property {string} [mesExamplesRaw]
 * @property {string} [charDepthPrompt]
 * @property {string} [creatorNotes]
 * @property {string} [version]
 * @property {string} [firstMessage]
 * @property {string[]} [alternateGreetings]
 */

/**
 * @typedef {Object} MacroEnvSystem
 * @property {string} model
 * @property {string} api
 * @property {number} maxPrompt
 * @property {number} maxContext
 * @property {number} maxResponse
 */

/**
 * @typedef {Object} MacroChat
 * @property {string} lastMessage
 * @property {string} lastUserMessage
 * @property {string} lastCharMessage
 * @property {number|string|null} lastMessageId
 * @property {number|string|null} lastSwipeId
 * @property {number|string|null} currentSwipeId
 * @property {number|string|null} firstIncludedMessageId
 * @property {number|string|null} firstDisplayedMessageId
 * @property {string} allChatRange
 * @property {number} idHash
 * @property {string|number|null} pickRerollSeed
 * @property {string} idleDuration
 */

/**
 * @typedef {Pick<typeof import('../../power-user.js').power_user,
 * 'instruct' | 'sysprompt' | 'context' | 'reasoning' | 'prefer_character_prompt' |
 * 'collapse_newlines' | 'pin_examples' | 'custom_stopping_strings' | 'custom_stopping_strings_macro'> &
 * { isGroup: boolean, groupNames: string[], groupNamesNotMuted: string[] }} MacroSettings
 */

/**
 * Plain captured inputs. Callers own this data; evaluation writes only its variable maps
 * and bannedWords. Clone once when starting an independent assembly.
 * @typedef {Object} MacroContext
 * @property {'new'|'legacy'} engine
 * @property {MacroEnvNames} names
 * @property {MacroEnvCharacter & {
 *   groupCards: ReturnType<typeof import('../../../script.js').getCharacterCardFieldsSource>['groupCards'],
 *   personaPosition: number
 * }} character Raw templates, without executing their macros.
 * @property {MacroEnvSystem} system
 * @property {MacroChat} chat
 * @property {MacroSettings} settings
 * @property {{input: string, isMobile: boolean, lastGenerationType: string,
 *   extensions: {name: string, enabled: boolean}[], ephemeralStoppingStrings: string[]}} state
 * @property {number} now Captured Unix time in milliseconds.
 * @property {{local: Record<string, any>, global: Record<string, any>}} variables Raw stored values.
 * @property {Record<string, {value: string}>} extensionPrompts
 * @property {string[]} bannedWords
 * @property {Record<string, any>} extra Module-owned, structured-cloneable data.
 */

/** @typedef {Omit<import('./MacroEnvBuilder.js').MacroEnvRawContext, 'content'>} MacroEvaluationOptions */

/**
 * @typedef {Object} MacroEnvFunctions
 * @property {() => string} [original]
 * @property {(text: string) => string} postProcess
 * @property {(text: string, options?: MacroEvaluationOptions) => string} substitute Start a fresh top-level text using the same inputs and variables.
 */

/**
 * One evaluation frame. Character fields and original are fresh for each top-level text;
 * recursive handler resolve() calls retain this frame. Providers run on live preparation, including capture.
 * @typedef {Omit<MacroContext, 'character'|'variables'|'chat'|'state'> & {
 *   context: MacroContext,
 *   content: string,
 *   contentHash: number,
 *   character: MacroEnvCharacter,
 *   chat: Readonly<MacroChat>,
 *   state: Readonly<MacroContext['state']>,
 *   variables: ReturnType<typeof import('../../variables/scopes.js').createMacroVariables>,
 *   functions: MacroEnvFunctions,
 *   dynamicMacros: Record<string, DynamicMacroValue>
 * }} MacroEnv
 */

export {};
