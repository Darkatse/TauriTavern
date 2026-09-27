import { Handlebars, moment, seedrandom, droll } from '../lib.js';
import { chat, chat_metadata, getCurrentChatId, substituteParams } from '../script.js';
import { timestampToMoment, isDigitsOnly, getStringHash, escapeRegex, uuidv4 } from './utils.js';
import { getInstructMacros } from './instruct-mode.js';
import { getVariableMacros } from './variables/scopes.js';
import { inject_ids } from './constants.js';
import { initRegisterMacros, macros as macroSystem } from './macros/macro-system.js';
import { findLastMessageId, getLastSwipeId as getChatLastSwipeId, getCurrentSwipeId as getChatCurrentSwipeId } from './macros/chat-state.js';
import { power_user } from './power-user.js';
import { MacroEnvBuilder } from './macros/engine/MacroEnvBuilder.js';
import { getMacroMessageExamples } from './macros/definitions/env-macros.js';

/**
 * @typedef Macro
 * @property {RegExp} regex - Regular expression to match the macro
 * @property {(substring: string, ...args: any[]) => string} replace - Function to replace the macro
 */

// Register any macro that you want to leave in the compiled story string
Handlebars.registerHelper('trim', () => '{{trim}}');
// Catch-all helper for any macro that is not defined for story strings
Handlebars.registerHelper('helperMissing', function () {
    const options = arguments[arguments.length - 1];
    const macroName = options.name;
    return substituteParams(`{{${macroName}}}`);
});

/**
 * @typedef {Object<string, *>} EnvObject
 * @typedef {(nonce: string, env?: import('./macros/engine/MacroEnv.types.js').MacroEnv) => string} MacroFunction
 */

/**
 * @typedef {Object} CustomMacro
 * @property {string} key - Macro name (key)
 * @property {string} description - Optional description of the macro
 */

/**
 * @deprecated Use macros.registry.registerMacro (from scripts/macros/macro-system.js)
 * or substituteParams({ dynamicMacros }) with the new macro engine.
 */
export class MacrosParser {
    /**
     * A map of registered macros.
     * @type {Map<string, string|MacroFunction>}
     */
    static #macros = new Map();

    /**
     * A map of macro descriptions.
     * @type {Map<string, string>}
     */
    static #descriptions = new Map();

    /**
     * Logs a deprecation warning for MacrosParser APIs, pointing callers to
     * the new macro engine registration surface.
     *
     * @param {string} method
     * @param {string} replacement
     * @param {IArguments} [methodArgs=null]
     * @returns {void}
     */
    static #logDeprecated(method, replacement, methodArgs = null) {
        console.warn(`[DEPRECATED] MacrosParser.${method} is deprecated and will be removed in a future version. Use ${replacement} instead. Arguments:`, (methodArgs ?? 'none'));
    }

    /**
     * Bridges a legacy MacrosParser macro registration into the new macro
     * engine when the experimental macro engine flag is enabled.
     *
     * This mirrors the simple "{{key}}" replacement behavior by registering
     * a 0-arg macro in MacroRegistry that does not take arguments and returns
     * the sanitized value from the legacy registry.
     *
     * @param {string} key
     * @param {string|MacroFunction} value
     * @param {string} description
     * @returns {void}
     */
    static #registerMacroInNewEngine(key, value, description) {
        if (!power_user.experimental_macro_engine) {
            return;
        }

        // Like the old MacrosParser, we explicitly allow overriding macros, and only warn
        if (macroSystem.registry.hasMacro(key)) {
            console.warn(`Macro ${key} is already registered`);
        }

        const legacyValue = value;

        macroSystem.registry.registerMacro(key, {
            // Legacy MacrosParser macros never took arguments; keep the
            // contract that only {{key}} without arguments is valid.
            category: 'legacy',
            description: typeof description === 'string' ? description : 'Automatically registered macro from MacrosParser',
            handler: ({ env }) => {
                /** @type {string|MacroFunction|undefined} */
                let stored = legacyValue;

                if (typeof stored === 'function') {
                    try {
                        const nonce = uuidv4();
                        stored = stored(nonce, env);
                    } catch (e) {
                        console.warn(`Macro "${key}" function threw an error.`, e);
                        stored = '';
                    }
                }

                // Let the new macro engine's normalizeMacroResult handle type
                // normalization for the returned value.
                return stored;
            },
        });
    }

    /**
     * Bridges a legacy MacrosParser macro unregistration into the new macro
     * engine when the experimental macro engine flag is enabled.
     *
     * @param {string} key
     * @returns {void}
     */
    static #unregisterMacroInNewEngine(key) {
        if (!power_user.experimental_macro_engine) {
            return;
        }

        macroSystem.registry.unregisterMacro(key);
    }

    /**
     * Returns an iterator over all registered macros.
     * @returns {IterableIterator<CustomMacro>}
     */
    static [Symbol.iterator] = function* () {
        // When experimental macro engine is active, yield from the new registry
        if (power_user.experimental_macro_engine) {
            // Exclude hidden aliases for consistency with autocomplete behavior
            for (const def of macroSystem.registry.getAllMacros({ excludeHiddenAliases: true })) {
                yield { key: def.name, description: def.description || '' };
            }
            return;
        }

        for (const macro of MacrosParser.#macros.keys()) {
            yield { key: macro, description: MacrosParser.#descriptions.get(macro) };
        }
    };

    /**
     * Access a macro by its name.
     * @param {string} key Macro name (key)
     * @returns {string|MacroFunction|undefined} The macro value
     */
    static get(key) {
        MacrosParser.#logDeprecated('get', 'macros.registry.getMacro (from scripts/macros/macro-system.js)', arguments);
        return MacrosParser.#macros.get(key);
    }

    /**
     * Checks if a macro is registered.
     * @param {string} key Macro name (key)
     * @returns {boolean} True if the macro is registered, false otherwise
     */
    static has(key) {
        MacrosParser.#logDeprecated('has', 'macros.registry.hasMacro (from scripts/macros/macro-system.js)', arguments);
        if (power_user.experimental_macro_engine) {
            return macroSystem.registry.hasMacro(key);
        }

        return MacrosParser.#macros.has(key);
    }

    /**
     * Registers a global macro that can be used anywhere where substitution is allowed.
     * @param {string} key Macro name (key)
     * @param {string|MacroFunction} value A string or a function that returns a string
     * @param {string} [description] Optional description of the macro
     */
    static registerMacro(key, value, description = '') {
        MacrosParser.#logDeprecated('registerMacro', 'macros.registry.registerMacro (from scripts/macros/macro-system.js) or substituteParams({ dynamicMacros })', arguments);
        if (typeof key !== 'string') {
            throw new Error('Macro key must be a string');
        }

        // Allowing surrounding whitespace would just create more confusion...
        key = key.trim();

        if (!key) {
            throw new Error('Macro key must not be empty or whitespace only');
        }

        if (key.startsWith('{{') || key.endsWith('}}')) {
            throw new Error('Macro key must not include the surrounding braces');
        }

        if (typeof value !== 'string' && typeof value !== 'function') {
            console.warn(`Macro value for "${key}" will be converted to a string`);
            value = this.sanitizeMacroValue(value);
        }

        MacrosParser.#registerMacroInNewEngine(key, value, description);
        if (power_user.experimental_macro_engine) {
            return;
        }

        if (this.#macros.has(key)) {
            console.warn(`Macro ${key} is already registered`);
        }

        this.#macros.set(key, value);

        if (typeof description === 'string' && description) {
            this.#descriptions.set(key, description);
        }
    }

    /**
     * Unregisters a global macro with the given key
     *
     * @param {string} key Macro name (key)
     */
    static unregisterMacro(key) {
        MacrosParser.#logDeprecated('unregisterMacro', 'macros.registry.unregisterMacro (from scripts/macros/macro-system.js)', arguments);
        if (typeof key !== 'string') {
            throw new Error('Macro key must be a string');
        }

        // Allowing surrounding whitespace would just create more confusion...
        key = key.trim();

        if (!key) {
            throw new Error('Macro key must not be empty or whitespace only');
        }

        if (power_user.experimental_macro_engine) {
            MacrosParser.#unregisterMacroInNewEngine(key);
            return;
        }

        const deleted = this.#macros.delete(key);

        if (!deleted) {
            console.warn(`Macro ${key} was not registered`);
        }

        this.#descriptions.delete(key);
    }

    /**
     * Populate the env object with macro values from the current context.
     * @param {EnvObject} env Env object for the current evaluation context
     * @returns {void}
     */
    static populateEnv(env) {
        if (!env || typeof env !== 'object') {
            console.warn('Env object is not provided');
            return;
        }

        // No macros are registered
        if (this.#macros.size === 0) {
            return;
        }

        for (const [key, value] of this.#macros) {
            env[key] = value;
        }
    }

    /**
     * Performs a type-check on the macro value and returns a sanitized version of it.
     * @param {any} value Value returned by a macro
     * @returns {string} Sanitized value
     */
    static sanitizeMacroValue(value) {
        if (typeof value === 'string') {
            return value;
        }

        if (value === null || value === undefined) {
            return '';
        }

        if (value instanceof Promise) {
            console.warn('Promises are not supported as macro values');
            return '';
        }

        if (typeof value === 'function') {
            console.warn('Functions are not supported as macro values');
            return '';
        }

        if (value instanceof Date) {
            return value.toISOString();
        }

        if (typeof value === 'object') {
            return JSON.stringify(value);
        }

        return String(value);
    }
}

/**
 * Gets a hashed id of the current chat from the metadata.
 * If no metadata exists, creates a new hash and saves it.
 * @returns {number} The hashed chat id
 */
export function getChatIdHash() {
    const cachedIdHash = chat_metadata.chat_id_hash;

    // If chat_id_hash is not already set, calculate it
    if (typeof cachedIdHash !== 'number') {
        // Use the main_chat if it's available, otherwise get the current chat ID
        const chatId = chat_metadata.main_chat ?? getCurrentChatId();
        const chatIdHash = getStringHash(chatId);
        chat_metadata.chat_id_hash = chatIdHash;
        return chatIdHash;
    }

    return cachedIdHash;
}

/**
 * Returns the ID of the last message in the chat
 *
 * Optionally can only choose specific messages, if a filter is provided.
 *
 * @param {object} param0 - Optional arguments
 * @param {boolean} [param0.exclude_swipe_in_propress=true] - Whether a message that is currently being swiped should be ignored
 * @param {function(object):boolean} [param0.filter] - A filter applied to the search, ignoring all messages that don't match the criteria. For example to only find user messages, etc.
 * @returns {number|null} The message id, or null if none was found
 */
export function getLastMessageId({ exclude_swipe_in_propress = true, filter = null } = {}) {
    return findLastMessageId(chat, { excludePendingSwipe: exclude_swipe_in_propress, filter });
}

/**
 * Returns the ID of the first message included in the context
 *
 * @returns {number|null} The ID of the first message in the context
 */
export function getFirstIncludedMessageId() {
    return chat_metadata.lastInContextMessageId;
}

/**
 * Returns the ID of the first displayed message in the chat.
 *
 * @returns {number|null} The ID of the first displayed message
 */
export function getFirstDisplayedMessageId() {
    const mesId = Number(document.querySelector('#chat .mes')?.getAttribute('mesid'));

    if (!isNaN(mesId) && mesId >= 0) {
        return mesId;
    }

    return null;
}

/**
 * Returns the last message in the chat
 *
 * @returns {string} The last message in the chat
 */
export function getLastMessage() {
    const mid = getLastMessageId();
    return chat[mid]?.mes ?? '';
}

/**
 * Returns the last message from the user
 *
 * @returns {string} The last message from the user
 */
export function getLastUserMessage() {
    const mid = getLastMessageId({ filter: m => m.is_user && !m.is_system });
    return chat[mid]?.mes ?? '';
}

/**
 * Returns the last message from the bot
 *
 * @returns {string} The last message from the bot
 */
export function getLastCharMessage() {
    const mid = getLastMessageId({ filter: m => !m.is_user && !m.is_system });
    return chat[mid]?.mes ?? '';
}

/**
 * Returns the number of existing swipes on the last message, including a pending swipe's message.
 *
 * @returns {number|null} The swipe count, or null when unavailable
 */
export function getLastSwipeId() {
    return getChatLastSwipeId(chat);
}

/**
 * Returns the 1-based ID (number) of the current swipe
 *
 * @returns {number|null} The 1-based ID of the current swipe
 */
export function getCurrentSwipeId() {
    return getChatCurrentSwipeId(chat);
}

/**
 * Replaces banned words in macros with an empty string.
 * Adds them to textgenerationwebui ban list.
 * @returns {Macro}
 */
function getBannedWordsMacro(macroEnv) {
    const banPattern = /{{banned "(.*)"}}/gi;
    const banReplace = (match, bannedWord) => {
        if (macroEnv.system.api === 'textgenerationwebui') {
            console.log('Found banned word in macros: ' + bannedWord);
            macroEnv.bannedWords.push(bannedWord);
        }
        return '';
    };

    return { regex: banPattern, replace: banReplace };
}

export function getTimeSinceLastMessage(now = moment()) {
    if (Array.isArray(chat) && chat.length > 0) {
        let lastMessage;
        let takeNext = false;

        for (let i = chat.length - 1; i >= 0; i--) {
            const message = chat[i];

            if (message.is_system) {
                continue;
            }

            if (message.is_user && takeNext) {
                lastMessage = message;
                break;
            }

            takeNext = true;
        }

        if (lastMessage?.send_date) {
            const lastMessageDate = timestampToMoment(lastMessage.send_date);
            const duration = moment.duration(now.diff(lastMessageDate));
            return duration.humanize();
        }
    }

    return 'just now';
}

/**
 * Returns a macro that picks a random item from a list.
 * @returns {Macro} The random replace macro
 */
function getRandomReplaceMacro() {
    const randomPattern = /{{random\s?::?([^}]+)}}/gi;
    const randomReplace = (match, listString) => {
        // Split on either double colons or comma. If comma is the separator, we are also trimming all items.
        const list = listString.includes('::')
            ? listString.split('::')
            // Replaced escaped commas with a placeholder to avoid splitting on them
            : listString.replace(/\\,/g, '##�COMMA�##').split(',').map(item => item.trim().replace(/##�COMMA�##/g, ','));

        if (list.length === 0) {
            return '';
        }
        const rng = seedrandom('added entropy.', { entropy: true });
        const randomIndex = Math.floor(rng() * list.length);
        return list[randomIndex];
    };

    return { regex: randomPattern, replace: randomReplace };
}

/**
 * Returns a macro that picks a random item from a list with a consistent seed.
 * @param {string} rawContent The raw content of the string
 * @returns {Macro} The pick replace macro
 */
function getPickReplaceMacro(rawContent, macroEnv) {
    // We need to have a consistent chat hash, otherwise we'll lose rolls on chat file rename or branch switches
    // No need to save metadata here - branching and renaming will implicitly do the save for us, and until then loading it like this is consistent
    const chatIdHash = macroEnv.chat.idHash;
    const rawContentHash = getStringHash(rawContent);

    const pickPattern = /{{pick\s?::?([^}]+)}}/gi;
    const pickReplace = (match, listString, offset) => {
        // Split on either double colons or comma. If comma is the separator, we are also trimming all items.
        const list = listString.includes('::')
            ? listString.split('::')
            // Replaced escaped commas with a placeholder to avoid splitting on them
            : listString.replace(/\\,/g, '##�COMMA�##').split(',').map(item => item.trim().replace(/##�COMMA�##/g, ','));

        if (list.length === 0) {
            return '';
        }

        // We build a hash seed based on: unique chat file, raw content, and the placement inside this content
        // This allows us to get unique but repeatable picks in nearly all cases
        const combinedSeedString = `${chatIdHash}-${rawContentHash}-${offset}`;
        const finalSeed = getStringHash(combinedSeedString);
        // @ts-ignore - have to use numbers for legacy picks
        const rng = seedrandom(finalSeed);
        const randomIndex = Math.floor(rng() * list.length);
        return list[randomIndex];
    };

    return { regex: pickPattern, replace: pickReplace };
}

/**
 * @returns {Macro} The dire roll macro
 */
function getDiceRollMacro() {
    const rollPattern = /{{roll[ : ]([^}]+)}}/gi;
    const rollReplace = (match, matchValue) => {
        let formula = matchValue.trim();

        if (isDigitsOnly(formula)) {
            formula = `1d${formula}`;
        }

        const isValid = droll.validate(formula);

        if (!isValid) {
            console.debug(`Invalid roll formula: ${formula}`);
            return '';
        }

        const result = droll.roll(formula);
        if (result === false) return '';
        return String(result.total);
    };

    return { regex: rollPattern, replace: rollReplace };
}

/**
 * Returns the difference between two times. Works with any time format acceptable by moment().
 * Can work with {{date}} {{time}} macros
 * @returns {Macro} The time difference macro
 */
function getTimeDiffMacro() {
    const timeDiffPattern = /{{timeDiff::(.*?)::(.*?)}}/gi;
    const timeDiffReplace = (_match, matchPart1, matchPart2) => {
        const time1 = moment(matchPart1);
        const time2 = moment(matchPart2);

        const timeDifference = moment.duration(time1.diff(time2));
        return timeDifference.humanize(true);
    };

    return { regex: timeDiffPattern, replace: timeDiffReplace };
}

/**
 * Returns the outlet prompt for a given outlet key.
 * @param {string} key - The outlet key
 * @returns {string} The outlet prompt
 */
function getOutletPrompt(key, macroEnv) {
    const value = macroEnv.extensionPrompts[inject_ids.CUSTOM_WI_OUTLET(key)]?.value;
    return value || '';
}

/**
 * Evaluate the legacy grammar with an already prepared macro environment.
 * @param {string} content
 * @param {import('./macros/engine/MacroEnv.types.js').MacroEnv} macroEnv
 * @returns {string}
 */
export function evaluateLegacyWithEnv(content, macroEnv) {
    const env = {};
    if (macroEnv.functions.original) env.original = macroEnv.functions.original;
    // Legacy card extraction is eager and ordered: fields may write variables used by later fields.
    const character = {};
    for (const field of ['charPrompt', 'mesExamplesRaw', 'description', 'personality', 'persona', 'scenario',
        'charInstruction', 'version', 'charDepthPrompt', 'creatorNotes', 'firstMessage', 'alternateGreetings']) {
        if (Object.hasOwn(macroEnv.character, field)) character[field] = macroEnv.character[field];
    }
    const fields = {
        charPrompt: 'charPrompt',
        charInstruction: 'charInstruction',
        charJailbreak: 'charInstruction',
        description: 'description',
        personality: 'personality',
        scenario: 'scenario',
        persona: 'persona',
        mesExamplesRaw: 'mesExamplesRaw',
        charVersion: 'version',
        char_version: 'version',
        charDepthPrompt: 'charDepthPrompt',
        creatorNotes: 'creatorNotes',
    };
    for (const [name, field] of Object.entries(fields)) {
        if (Object.hasOwn(character, field)) env[name] = character[field] ?? '';
    }
    if (Object.hasOwn(character, 'mesExamplesRaw')) {
        env.mesExamples = () => getMacroMessageExamples({ ...macroEnv, character });
    }
    // Names follow card fields so legacy substitution still expands names in those fields.
    env.user = macroEnv.names.user;
    env.char = macroEnv.names.char;
    env.group = env.charIfNotGroup = macroEnv.names.group;
    env.groupNotMuted = macroEnv.names.groupNotMuted;
    env.notChar = macroEnv.names.notChar;
    env.model = macroEnv.system.model;
    for (const [name, value] of Object.entries(macroEnv.dynamicMacros)) {
        const existing = Object.keys(env).find(key => key.toLowerCase() === name.toLowerCase());
        env[existing ?? name] = value;
    }
    return evaluateMacros(content, env, macroEnv.functions.postProcess, macroEnv);
}

/**
 * Substitutes {{macro}} parameters in a string.
 * @param {string} content - The string to substitute parameters in.
 * @param {EnvObject} env - Map of macro names to the values they'll be substituted with. If the param
 * values are functions, those functions will be called and their return values are used.
 * @param {function(string): string} postProcessFn - Function to run on the macro value before replacing it.
 * @param {import('./macros/engine/MacroEnv.types.js').MacroEnv|null} [macroEnv] - Explicit source for built-ins; omitted for ordinary live calls.
 * @returns {string} The string with substituted parameters.
 */
export function evaluateMacros(content, env, postProcessFn, macroEnv = null) {
    if (!content) {
        return '';
    }

    macroEnv ??= MacroEnvBuilder.buildFromRawEnv({ content, replaceCharacterCard: false }, 'legacy');
    postProcessFn = typeof postProcessFn === 'function' ? postProcessFn : (x => x);
    const rawContent = content;

    /**
     * Built-ins running before the env variables
     * @type {Macro[]}
     * */
    const preEnvMacros = [
        // Legacy non-curly macros
        { regex: /<USER>/gi, replace: () => typeof env.user === 'function' ? env.user() : env.user },
        { regex: /<BOT>/gi, replace: () => typeof env.char === 'function' ? env.char() : env.char },
        { regex: /<CHAR>/gi, replace: () => typeof env.char === 'function' ? env.char() : env.char },
        { regex: /<CHARIFNOTGROUP>/gi, replace: () => typeof env.group === 'function' ? env.group() : env.group },
        { regex: /<GROUP>/gi, replace: () => typeof env.group === 'function' ? env.group() : env.group },
        getDiceRollMacro(),
        ...getInstructMacros(env, macroEnv.settings),
        ...getVariableMacros(macroEnv.variables),
        { regex: /{{newline}}/gi, replace: () => '\n' },
        { regex: /(?:\r?\n)*{{trim}}(?:\r?\n)*/gi, replace: () => '' },
        { regex: /{{noop}}/gi, replace: () => '' },
        { regex: /{{input}}/gi, replace: () => macroEnv.state.input },
    ];

    /**
     * Built-ins running after the env variables
     * @type {Macro[]}
    */
    const postEnvMacros = [
        { regex: /{{maxPrompt}}/gi, replace: () => String(macroEnv.system.maxPrompt) },
        { regex: /{{maxPromptTokens}}/gi, replace: () => String(macroEnv.system.maxPrompt) },
        { regex: /{{maxContext}}/gi, replace: () => String(macroEnv.system.maxContext) },
        { regex: /{{maxContextTokens}}/gi, replace: () => String(macroEnv.system.maxContext) },
        { regex: /{{maxResponse}}/gi, replace: () => String(macroEnv.system.maxResponse) },
        { regex: /{{maxResponseTokens}}/gi, replace: () => String(macroEnv.system.maxResponse) },
        { regex: /{{lastMessage}}/gi, replace: () => macroEnv.chat.lastMessage },
        { regex: /{{lastMessageId}}/gi, replace: () => String(macroEnv.chat.lastMessageId ?? '') },
        { regex: /{{lastUserMessage}}/gi, replace: () => macroEnv.chat.lastUserMessage },
        { regex: /{{lastCharMessage}}/gi, replace: () => macroEnv.chat.lastCharMessage },
        { regex: /{{firstIncludedMessageId}}/gi, replace: () => String(macroEnv.chat.firstIncludedMessageId ?? '') },
        { regex: /{{firstDisplayedMessageId}}/gi, replace: () => String(macroEnv.chat.firstDisplayedMessageId ?? '') },
        { regex: /{{lastSwipeId}}/gi, replace: () => String(macroEnv.chat.lastSwipeId ?? '') },
        { regex: /{{currentSwipeId}}/gi, replace: () => String(macroEnv.chat.currentSwipeId ?? '') },
        { regex: /{{allChatRange}}/gi, replace: () => macroEnv.chat.allChatRange },
        { regex: /{{reverse:(.+?)}}/gi, replace: (_, str) => Array.from(str).reverse().join('') },
        { regex: /\{\{\/\/([\s\S]*?)\}\}/gm, replace: () => '' },
        { regex: /{{time}}/gi, replace: () => moment(macroEnv.now).format('LT') },
        { regex: /{{date}}/gi, replace: () => moment(macroEnv.now).format('LL') },
        { regex: /{{weekday}}/gi, replace: () => moment(macroEnv.now).format('dddd') },
        { regex: /{{isotime}}/gi, replace: () => moment(macroEnv.now).format('HH:mm') },
        { regex: /{{isodate}}/gi, replace: () => moment(macroEnv.now).format('YYYY-MM-DD') },
        { regex: /{{datetimeformat +([^}]*)}}/gi, replace: (_, format) => moment(macroEnv.now).format(format) },
        { regex: /{{idle_duration}}/gi, replace: () => macroEnv.chat.idleDuration },
        { regex: /{{time_UTC([-+]\d+)}}/gi, replace: (_, offset) => moment(macroEnv.now).utc().utcOffset(parseInt(offset, 10)).format('LT') },
        { regex: /{{outlet::(.+?)}}/gi, replace: (_, key) => getOutletPrompt(key.trim(), macroEnv) || '' },
        getTimeDiffMacro(),
        getBannedWordsMacro(macroEnv),
        getRandomReplaceMacro(),
        getPickReplaceMacro(rawContent, macroEnv),
    ];

    // Add all registered macros to the env object
    MacrosParser.populateEnv(env);
    const nonce = uuidv4();
    const envMacros = [];

    // Substitute passed-in variables
    for (const varName in env) {
        if (!Object.hasOwn(env, varName)) continue;

        const envRegex = new RegExp(`{{${escapeRegex(varName)}}}`, 'gi');
        const envReplace = () => {
            const param = env[varName];
            const value = MacrosParser.sanitizeMacroValue(typeof param === 'function' ? param(nonce, macroEnv) : param);
            return value;
        };

        envMacros.push({ regex: envRegex, replace: envReplace });
    }

    const macros = [...preEnvMacros, ...envMacros, ...postEnvMacros];

    for (const macro of macros) {
        // Stop if the content is empty
        if (!content) {
            break;
        }

        // Short-circuit if no curly braces are found
        if (!macro.regex.source.startsWith('<') && !content.includes('{{')) {
            break;
        }

        try {
            content = content.replace(macro.regex, (...args) => postProcessFn(macro.replace(...args)));
        } catch (e) {
            console.warn(`Macro content can't be replaced: ${macro.regex} in ${content}`, e);
        }
    }

    return content;
}

export function initMacros() {
    // Only manually register those is new macro engine is not on. In the new one, they are already registered automatically
    if (!power_user.experimental_macro_engine) {
        MacrosParser.registerMacro('lastGenerationType',
            (_nonce, env) => env.state.lastGenerationType,
            'Returns the type of the last generation (e.g., "normal", "swipe", "continue", "impersonate", "quiet").',
        );

        MacrosParser.registerMacro('isMobile',
            (_nonce, env) => String(env.state.isMobile),
            'Returns "true" if the user is on a mobile device, "false" otherwise.',
        );
    }

    // TODO: Needs to be moved once old macros are deprecated and removed
    initRegisterMacros();
}
