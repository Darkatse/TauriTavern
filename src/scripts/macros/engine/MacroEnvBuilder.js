import {
    name1, name2, characters, chat, chat_metadata, extension_prompts, main_api,
    getCharacterCardFieldsSource, getCharacterCardFieldsLazy, createCharacterCardFields, getGeneratingModel,
    getMaxPromptTokens, getMaxContextTokens, getMaxResponseTokens, substituteParams,
} from '../../../script.js';
import { groups, selected_group } from '../../group-chats.js';
import { power_user, collapseNewlines, EPHEMERAL_STOPPING_STRINGS } from '../../power-user.js';
import { extension_settings, extensionNames } from '../../extensions.js';
import { isMobile } from '../../RossAscends-mods.js';
import { textgenerationwebui_banned_in_macros } from '../../textgen-settings.js';
import {
    getLastMessage, getLastUserMessage, getLastCharMessage, getLastMessageId,
    getLastSwipeId, getCurrentSwipeId, getFirstIncludedMessageId, getFirstDisplayedMessageId,
    getTimeSinceLastMessage, getChatIdHash,
} from '../../macros.js';
import { liveVariables, createMacroVariables } from '../../variables/scopes.js';
import { getLastGenerationType } from '../definitions/state-macros.js';
import { evaluateWithContext } from '../macro-system.js';
import { logMacroGeneralError } from './MacroDiagnostics.js';
import { getStringHash } from '../../utils.js';

/** @typedef {import('./MacroEnv.types.js').MacroEnv} MacroEnv */
/** @typedef {import('./MacroEnv.types.js').MacroContext} MacroContext */
/**
 * @typedef {Object} MacroEnvRawContext
 * @property {string} content
 * @property {string|null} [name1Override]
 * @property {string|null} [name2Override]
 * @property {string|null} [original]
 * @property {string|null} [groupOverride]
 * @property {boolean} [replaceCharacterCard]
 * @property {Record<string, import('./MacroEnv.types.js').DynamicMacroValue>|null} [dynamicMacros]
 * @property {(value: string) => string} [postProcessFn]
 */
/** @typedef {(env: MacroEnv, ctx: MacroEnvRawContext) => void} MacroEnvProvider */

export const env_provider_order = { EARLIEST: 0, EARLY: 10, NORMAL: 50, LATE: 90, LATEST: 100 };

const characterFields = [
    ['charPrompt', 'system'], ['charInstruction', 'jailbreak'], ['description', 'description'],
    ['personality', 'personality'], ['scenario', 'scenario'], ['persona', 'persona'],
    ['mesExamplesRaw', 'mesExamples'], ['version', 'version'], ['charDepthPrompt', 'charDepthPrompt'],
    ['creatorNotes', 'creatorNotes'], ['firstMessage', 'firstMessage'], ['alternateGreetings', 'alternateGreetings'],
];

function readSettings() {
    const group = groups.find(entry => entry.id === selected_group);
    const members = (group?.members ?? []).map(avatar => characters.find(c => c.avatar === avatar)).filter(Boolean);
    return {
        instruct: power_user.instruct,
        sysprompt: power_user.sysprompt,
        context: power_user.context,
        reasoning: power_user.reasoning,
        prefer_character_prompt: power_user.prefer_character_prompt,
        collapse_newlines: power_user.collapse_newlines,
        pin_examples: power_user.pin_examples,
        custom_stopping_strings: power_user.custom_stopping_strings,
        custom_stopping_strings_macro: power_user.custom_stopping_strings_macro,
        isGroup: Boolean(selected_group),
        groupNames: members.map(character => character.name),
        groupNamesNotMuted: members.filter(c => !group.disabled_members.includes(c.avatar)).map(c => c.name),
    };
}

function readCharacter() {
    const source = getCharacterCardFieldsSource();
    return {
        ...Object.fromEntries(characterFields.map(([key, field]) => [key, source[field]])),
        groupCards: source.groupCards,
        personaPosition: Number(power_user.persona_description_position),
    };
}

function readExtensions() {
    return extensionNames.map(name => ({ name, enabled: !extension_settings.disabledExtensions.includes(name) }));
}

function readNames(settings, engine) {
    const user = name1 ?? '';
    const char = name2 ?? '';
    // Legacy includes muted members and the user; the new engine excludes both.
    const others = (engine === 'legacy' ? settings.groupNames : settings.groupNamesNotMuted)
        .filter(name => name !== char);
    if (engine === 'legacy') others.push(user);
    return {
        user, char,
        group: settings.isGroup ? settings.groupNames.join(', ') : char,
        groupNotMuted: settings.isGroup ? settings.groupNamesNotMuted.join(', ') : char,
        notChar: settings.isGroup ? others.join(', ') : user,
    };
}

// Stateless read-only views. Live reads stay current; capture materializes their values once.
const liveChat = Object.freeze({
    get lastMessage() { return getLastMessage(); },
    get lastUserMessage() { return getLastUserMessage(); },
    get lastCharMessage() { return getLastCharMessage(); },
    get lastMessageId() { return getLastMessageId() ?? ''; },
    get lastSwipeId() { return getLastSwipeId() ?? ''; },
    get currentSwipeId() { return getCurrentSwipeId() ?? ''; },
    get firstIncludedMessageId() { return getFirstIncludedMessageId(); },
    get firstDisplayedMessageId() { return getFirstDisplayedMessageId(); },
    get allChatRange() { return chat.length ? `0-${chat.length - 1}` : ''; },
    get idHash() { return getChatIdHash(); },
    get pickRerollSeed() { return chat_metadata.pick_reroll_seed || null; },
    get idleDuration() { return getTimeSinceLastMessage(); },
});

const liveState = Object.freeze({
    get input() { return String(document.querySelector('#send_textarea')?.value ?? ''); },
    get isMobile() { return isMobile(); },
    get lastGenerationType() { return getLastGenerationType(); },
    get extensions() { return readExtensions(); },
    get ephemeralStoppingStrings() { return EPHEMERAL_STOPPING_STRINGS; },
});

function readLiveContext(engine) {
    const settings = readSettings();
    return {
        engine,
        names: readNames(settings, engine),
        get character() { return readCharacter(); },
        system: {
            model: getGeneratingModel(),
            get api() { return main_api; },
            get maxPrompt() { return getMaxPromptTokens(); },
            get maxContext() { return getMaxContextTokens(); },
            get maxResponse() { return getMaxResponseTokens(); },
        },
        chat: liveChat,
        settings,
        state: liveState,
        now: Date.now(),
        variables: { local: chat_metadata.variables ?? {}, global: extension_settings.variables?.global ?? {} },
        extensionPrompts: extension_prompts,
        bannedWords: [],
        extra: {},
    };
}

class EnvBuilder {
    #providers = [];

    /** @param {MacroEnvProvider} provider @param {number} [order] */
    registerProvider(provider, order = env_provider_order.NORMAL) {
        if (typeof provider !== 'function') throw new Error('Provider must be a function');
        this.#providers.push({ fn: provider, order });
    }

    /**
     * Existing low-level entry defaults to the new engine; wrappers select their own engine.
     * @param {MacroEnvRawContext} ctx
     * @param {'new'|'legacy'} [engine='new']
     */
    buildFromRawEnv(ctx, engine = 'new') {
        const context = readLiveContext(engine);
        const env = this.#build({ ...ctx, replaceCharacterCard: ctx.replaceCharacterCard ?? false },
            context, liveVariables, substituteParams, getCharacterCardFieldsLazy);
        env.bannedWords = textgenerationwebui_banned_in_macros;
        for (const { fn } of this.#providers.slice().sort((a, b) => a.order - b.order)) {
            try {
                fn(env, ctx);
            } catch (error) {
                logMacroGeneralError({ message: 'MacroEnvBuilder: Provider error', error });
            }
        }
        return env;
    }

    /** Capture facts, never the interpreted lazy character fields or macro handlers. */
    captureContext() {
        const env = this.buildFromRawEnv({ content: '', replaceCharacterCard: false },
            power_user.experimental_macro_engine ? 'new' : 'legacy');
        return structuredClone({
            ...env.context,
            names: env.names,
            system: env.system,
            extra: env.extra,
            // Filters belong to prompt assembly. Macro outlets only consume text.
            extensionPrompts: Object.fromEntries(Object.entries(env.extensionPrompts)
                .map(([key, prompt]) => [key, { value: prompt.value ?? '' }])),
        });
    }

    /** A new top-level text shares the caller's data; providers have already run. */
    buildFromContext(ctx, context) {
        if (!context?.character || !context.names || !context.variables || !context.settings || !context.chat) {
            throw new Error('macro.context_required: Explicit evaluation requires a complete macro context');
        }
        const substitute = (text, options) => evaluateWithContext(text, context, options);
        const readFields = () => createCharacterCardFields({
            ...Object.fromEntries(characterFields.map(([key, field]) => [field, context.character[key]])),
            groupCards: context.character.groupCards,
        }, (text, name1Override = null, name2Override = null) => {
            if (typeof text !== 'string' || !text) return text;
            let result = substitute(text, { name1Override, name2Override, replaceCharacterCard: false });
            if (context.settings.collapse_newlines) result = collapseNewlines(result);
            return result.replace(/\r/g, '');
        });
        return this.#build(ctx, context, createMacroVariables(context.variables), substitute, readFields);
    }

    #build(ctx, context, variables, substitute, readFields) {
        const names = { ...context.names };
        if (ctx.name1Override != null) names.user = ctx.name1Override;
        if (ctx.name2Override != null) names.char = ctx.name2Override;
        if (!context.settings.isGroup && (ctx.name1Override != null || ctx.name2Override != null)) {
            names.group = names.groupNotMuted = names.char;
            names.notChar = names.user;
        } else if (ctx.name1Override != null || ctx.name2Override != null) {
            const members = context.engine === 'legacy' ? context.settings.groupNames : context.settings.groupNamesNotMuted;
            const others = members.filter(name => name !== names.char);
            if (context.engine === 'legacy') others.push(names.user);
            names.notChar = others.join(', ');
        }
        if (ctx.groupOverride != null) {
            names.group = names.groupNotMuted = ctx.groupOverride;
            if (context.engine === 'new') names.notChar = ctx.groupOverride;
        }
        const env = {
            context,
            engine: context.engine,
            system: context.system,
            chat: context.chat,
            settings: context.settings,
            state: context.state,
            now: context.now,
            extra: context.extra,
            extensionPrompts: context.extensionPrompts,
            bannedWords: context.bannedWords,
            content: ctx.content,
            contentHash: getStringHash(ctx.content),
            names,
            character: {},
            variables,
            functions: { postProcess: ctx.postProcessFn ?? (value => value), substitute },
            dynamicMacros: Object.fromEntries(Object.entries(ctx.dynamicMacros ?? {}).map(([key, value]) => [key.toLowerCase(), value])),
        };
        if (typeof ctx.original === 'string') {
            let available = true;
            env.functions.original = () => {
                if (!available) return '';
                available = false;
                return ctx.original;
            };
        }
        if (ctx.replaceCharacterCard !== false) {
            let fields;
            for (const [key, field] of characterFields) {
                Object.defineProperty(env.character, key, { enumerable: true, configurable: true, get: () => (fields ??= readFields())[field] });
            }
        }
        return env;
    }

    /** Session inputs have no current character or chat. Only user-wide settings are shared. */
    createSessionContext(messages, { sessionId = '', variables } = {}) {
        const text = message => (message?.parts ?? []).filter(part => part.type === 'text').map(part => part.text).join('\n');
        const lastUser = messages.findLast(message => message.role === 'user');
        const lastChar = messages.findLast(message => message.role === 'assistant');
        const settings = structuredClone(readSettings());
        settings.isGroup = false;
        settings.groupNames = [];
        settings.groupNamesNotMuted = [];
        settings.prefer_character_prompt = false;
        return {
            engine: power_user.experimental_macro_engine ? 'new' : 'legacy',
            names: { user: '', char: '', group: '', groupNotMuted: '', notChar: '' },
            character: { ...Object.fromEntries(characterFields.map(([key]) => [key, key === 'alternateGreetings' ? [] : ''])), groupCards: null, personaPosition: 0 },
            system: { model: '', api: 'openai', maxPrompt: 0, maxContext: 0, maxResponse: 0 },
            chat: {
                lastMessage: text(messages.at(-1)), lastUserMessage: text(lastUser), lastCharMessage: text(lastChar),
                lastMessageId: messages.length ? messages.length - 1 : '',
                lastSwipeId: '', currentSwipeId: '', firstIncludedMessageId: null, firstDisplayedMessageId: null,
                allChatRange: messages.length ? `0-${messages.length - 1}` : '',
                idHash: getStringHash(sessionId), pickRerollSeed: null, idleDuration: 'just now',
            },
            settings,
            state: { input: text(lastUser), isMobile: isMobile(), lastGenerationType: 'normal', extensions: readExtensions(), ephemeralStoppingStrings: [] },
            now: Date.now(),
            variables: structuredClone(variables ?? { local: {}, global: extension_settings.variables?.global ?? {} }),
            extensionPrompts: {}, bannedWords: [], extra: {},
        };
    }
}

export const MacroEnvBuilder = new EnvBuilder();
