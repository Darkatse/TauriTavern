import { DOMPurify, Fuse } from '../../../lib.js';

import { activateSendButtons, deactivateSendButtons, event_types, eventSource, main_api, online_status, saveSettingsDebounced, withConnectionValidationSuspended } from '../../../script.js';
import { extension_settings, getContext, renderExtensionTemplateAsync } from '../../extensions.js';
import { callGenericPopup, Popup, POPUP_RESULT, POPUP_TYPE } from '../../popup.js';
import { SlashCommand } from '../../slash-commands/SlashCommand.js';
import { SlashCommandAbortController } from '../../slash-commands/SlashCommandAbortController.js';
import { ARGUMENT_TYPE, SlashCommandArgument, SlashCommandNamedArgument } from '../../slash-commands/SlashCommandArgument.js';
import { commonEnumProviders, enumIcons } from '../../slash-commands/SlashCommandCommonEnumsProvider.js';
import { SlashCommandDebugController } from '../../slash-commands/SlashCommandDebugController.js';
import { enumTypes, SlashCommandEnumValue } from '../../slash-commands/SlashCommandEnumValue.js';
import { SlashCommandClosure } from '../../slash-commands/SlashCommandClosure.js';
import { SlashCommandParser } from '../../slash-commands/SlashCommandParser.js';
import { SlashCommandScope } from '../../slash-commands/SlashCommandScope.js';
import { collapseSpaces, getUniqueName, isFalseBoolean, isTrueBoolean, uuidv4, waitUntilCondition } from '../../utils.js';
import { t } from '../../i18n.js';
import { getSecretLabelById, resolveSecretKey, SECRET_KEYS, secret_state } from '../../secrets.js';
import { getAdditionalParametersForSource, oai_settings, proxies, settingsToUpdate } from '../../openai.js';
import { performFuzzySearch } from '../../power-user.js';
import { connectCurrentApi } from '../../slash-commands.js';
import { StreamingDisplay } from '/scripts/streaming-display.js';
import { ConnectionManagerRequestService, getSelectedConnectionItemId, modelTargetAsProfile } from '../shared.js';
import { MODEL_TARGET_KIND, MODEL_TARGET_SELECTION_KIND, modelTargetConnectionRef } from '../../tauritavern/agent/model-target-llm-connection.js';
import { getLoadedPresetBody, installConnectionOwnership } from './connection-ownership.js';
import { installDriftTracker } from './drift.js';
import { installSidebarSelect } from './sidebar-select.js';
import { createOverflowMenuButton } from '../../tauri/compact-rows/overflow-menu.js';
import { formatReasoning } from '/scripts/reasoning.js';

const MODULE_NAME = 'connection-manager';
const NONE = '<None>';
const EMPTY = '<Empty>';
const NO_PROXY_PRESET = 'None';
const MODEL_TARGET_SCHEMA_VERSION = 1;
const CONNECTION_ITEM_KIND = {
    PROFILE: 'profile',
    MODEL_TARGET: MODEL_TARGET_SELECTION_KIND,
};
const CREATE_MODEL_TARGET_RESULT = POPUP_RESULT.CUSTOM1;
const CREATE_MODEL_AND_PRESET_RESULT = POPUP_RESULT.CUSTOM2;
/** Fields a "model + preset" profile keeps: the model route (as a model saves it) plus the settings preset. */
const MODEL_AND_PRESET_FIELDS = ['api', 'custom-api-format', 'api-url', 'model', 'proxy', 'secret-id', 'prompt-post-processing', 'preset'];
/** `profile.extensions.tauritavern.kind` of a profile saved with "Save Model + Preset". */
const MODEL_AND_PRESET_PROFILE_KIND = 'modelAndPreset';

/**
 * UI parts `init()` installs. Null until then, so a call that runs too early fails
 * instead of silently doing nothing.
 * @type {{
 *   drift: ReturnType<typeof installDriftTracker>,
 *   sidebar: ReturnType<typeof installSidebarSelect>,
 *   syncBindToggle: () => void,
 * }|null}
 */
let ui = null;
/** Selection applications in flight; drift checks wait for them to settle. */
let applyingCount = 0;
/** Resolves when the latest selector change finished: `false` if it could not apply. */
let pendingSelection = Promise.resolve(true);
/**
 * The item whose application last wrote the live request hints, so leaving it can undo
 * exactly that. Items created from the live settings wrote nothing.
 * @type {ConnectionProfile|LlmModelTarget|null}
 */
let hintsWriter = null;

/** @returns {NonNullable<typeof ui>} */
function requireUi() {
    if (!ui) {
        throw new Error('Connection Manager UI is not installed yet');
    }
    return ui;
}

/** Starts drift tracking from the current settings for the selected item, if any. */
function trackSelectedItem() {
    const { drift } = requireUi();
    const selected = getSelectedItem();
    if (!selected) {
        drift.clear();
        return;
    }
    drift.track(() => readItemFingerprint(selected)).catch((error) => {
        console.error('Connection Manager: could not read the settings the selected item records', error);
    });
}

const DEFAULT_SETTINGS = {
    profiles: [],
    selectedProfile: null,
    selectedItem: null,
    modelTargets: [],
};

let profileApplicationVersion = 0;
// Profile application replays slash commands; serialize those replays so only the newest profile validates.
let profileApplicationQueue = Promise.resolve();

// Commands that can record an empty value into the profile
const ALLOW_EMPTY = [
    'stop-strings',
    'start-reply-with',
];

const CC_COMMANDS = [
    'api',
    'preset',
    // Do not fix; CC needs to set the API twice because it could be overridden by the preset
    'api',
    'custom-api-format',
    'api-url',
    'model',
    'proxy',
    'stop-strings',
    'start-reply-with',
    'reasoning-template',
    'prompt-post-processing',
    'secret-id',
    'regex-preset',
];

const TC_COMMANDS = [
    'api',
    'preset',
    'api-url',
    'model',
    'sysprompt',
    'sysprompt-state',
    'instruct',
    'context',
    'instruct-state',
    'tokenizer',
    'stop-strings',
    'start-reply-with',
    'reasoning-template',
    'secret-id',
    'regex-preset',
];

/**
 * Everything outside the model route and preset, from both API types, so a later
 * "update" or an API-type switch can never pull formatting settings into the profile.
 * @returns {string[]}
 */
function getModelAndPresetExclude() {
    return [...new Set([...CC_COMMANDS, ...TC_COMMANDS])].filter(command => !MODEL_AND_PRESET_FIELDS.includes(command));
}

const FANCY_NAMES = {
    'api': 'API',
    'api-url': 'Server URL',
    'custom-api-format': 'Custom API Format',
    'preset': 'Settings Preset',
    'model': 'Model',
    'proxy': 'Proxy Preset',
    'sysprompt-state': 'Use System Prompt',
    'sysprompt': 'System Prompt Name',
    'instruct-state': 'Instruct Mode',
    'instruct': 'Instruct Template',
    'context': 'Context Template',
    'tokenizer': 'Tokenizer',
    'stop-strings': 'Custom Stopping Strings',
    'start-reply-with': 'Start Reply With',
    'reasoning-template': 'Reasoning Template',
    'prompt-post-processing': 'Prompt Post-Processing',
    'secret-id': 'Secret',
    'regex-preset': 'Regex Preset',
};

/**
 * A wrapper for the connection manager spinner.
 */
class ConnectionManagerSpinner {
    /**
     * @type {AbortController[]}
     */
    static abortControllers = [];

    /** @type {HTMLElement[]} The API panel spinner and its sidebar twin */
    spinnerElements;

    /** @type {AbortController} */
    abortController = new AbortController();

    constructor() {
        this.spinnerElements = [...document.querySelectorAll('#connection_profile_spinner, #tt_sidebar_connection_spinner')];
        this.abortController = new AbortController();
    }

    start() {
        ConnectionManagerSpinner.abortControllers.push(this.abortController);
        this.spinnerElements.forEach(element => element.classList.remove('hidden'));
    }

    stop() {
        this.spinnerElements.forEach(element => element.classList.add('hidden'));
    }

    isAborted() {
        return this.abortController.signal.aborted;
    }

    static abort() {
        for (const controller of ConnectionManagerSpinner.abortControllers) {
            controller.abort();
        }
        ConnectionManagerSpinner.abortControllers = [];
    }
}

/**
 * Get named arguments for the command callback.
 * @param {object} [args] Additional named arguments
 * @param {string} [args.force] Whether to force setting the value
 * @returns {object} Named arguments
 */
function getNamedArguments(args = {}) {
    // None of the commands here use underscored args, but better safe than sorry
    return {
        _scope: new SlashCommandScope(),
        _abortController: new SlashCommandAbortController(),
        _debugController: new SlashCommandDebugController(),
        _parserFlags: {},
        _hasUnnamedArgument: false,
        quiet: 'true',
        ...args,
    };
}

/** @type {() => SlashCommandEnumValue[]} */
const profilesProvider = () => [
    new SlashCommandEnumValue(NONE),
    ...extension_settings.connectionManager.profiles.map(p => new SlashCommandEnumValue(p.name, null, enumTypes.name, enumIcons.server)),
    ...extension_settings.connectionManager.modelTargets.map(target => new SlashCommandEnumValue(target.name, t`Model`, enumTypes.name, enumIcons.server)),
];

/**
 * @typedef {Object} ConnectionProfile
 * @property {string} id Unique identifier
 * @property {string} mode Mode of the connection profile
 * @property {string} [name] Name of the connection profile
 * @property {string} [api] API
 * @property {string} [preset] Settings Preset
 * @property {string} [model] Model
 * @property {string} [proxy] Proxy Preset
 * @property {string} [instruct] Instruct Template
 * @property {string} [context] Context Template
 * @property {string} [instruct-state] Instruct Mode
 * @property {string} [tokenizer] Tokenizer
 * @property {string} [stop-strings] Custom Stopping Strings
 * @property {string} [start-reply-with] Start Reply With
 * @property {string} [reasoning-template] Reasoning Template
 * @property {string} [prompt-post-processing] Prompt Post-Processing
 * @property {string} [custom-api-format] Custom API Format
 * @property {string} [sysprompt] System Prompt Name
 * @property {string} [sysprompt-state] Use System Prompt
 * @property {string} [api-url] Server URL
 * @property {string} [secret-id] Secret ID
 * @property {string} [regex-preset] Regex Preset ID
 * @property {string[]} [exclude] Commands to exclude
 * @property {Record<string, string>} [adapterHints] Request hints ("model + preset" profiles; see REQUEST_HINTS)
 * @property {{tauritavern?: {kind?: string}}} [extensions] Namespaced additions upstream code keeps as they are
 */

/**
 * @typedef {Object} LlmModelTarget
 * @property {number} schemaVersion Schema version
 * @property {string} kind Object kind
 * @property {string} id Unique identifier
 * @property {string} mode Mode of the model target
 * @property {string} name Name of the model target
 * @property {string} [api] API
 * @property {string} [model] Model
 * @property {string} [proxy] Proxy Preset
 * @property {string} [custom-api-format] Custom API Format
 * @property {string} [api-url] Server URL
 * @property {{key:string, id:string, labelSnapshot?:string}} [secretRef] Secret reference
 * @property {Record<string, string>} [adapterHints] Request hints and native adapter opt-ins (see REQUEST_HINTS)
 */

/**
 * @typedef {Object} ConnectionManagerItemRef
 * @property {string} kind Item kind
 * @property {string} id Item identifier
 */

/**
 * Builds a stable select option value for a managed item.
 * @param {string} kind Item kind
 * @param {string} id Item identifier
 * @returns {string}
 */
function makeItemOptionValue(kind, id) {
    if (kind === CONNECTION_ITEM_KIND.PROFILE) {
        // Keep legacy profile option values as raw IDs; shared consumers and slash commands read this DOM contract directly.
        return id;
    }

    if (kind === CONNECTION_ITEM_KIND.MODEL_TARGET) {
        return `${kind}:${id}`;
    }

    throw new Error(`Unknown connection manager item kind: ${kind}`);
}

/**
 * Parses a managed item select option value.
 * @param {string} value Option value
 * @returns {ConnectionManagerItemRef|null}
 */
function parseItemOptionValue(value) {
    if (!value) {
        return null;
    }

    const separatorIndex = value.indexOf(':');
    if (separatorIndex === -1) {
        return { kind: CONNECTION_ITEM_KIND.PROFILE, id: value };
    }

    return {
        kind: value.slice(0, separatorIndex),
        id: value.slice(separatorIndex + 1),
    };
}

/**
 * Gets the selected managed item reference, migrating legacy selectedProfile on read.
 * @returns {ConnectionManagerItemRef|null}
 */
function getSelectedItemRef() {
    const selectedItem = extension_settings.connectionManager.selectedItem;
    if (selectedItem?.kind && selectedItem?.id) {
        return selectedItem;
    }

    const selectedProfile = extension_settings.connectionManager.selectedProfile;
    if (selectedProfile) {
        return { kind: CONNECTION_ITEM_KIND.PROFILE, id: selectedProfile };
    }

    return null;
}

/**
 * Sets the selected managed item while preserving selectedProfile's legacy meaning.
 * @param {ConnectionManagerItemRef|null} ref Selected item
 */
function setSelectedItemRef(ref) {
    extension_settings.connectionManager.selectedItem = ref ? { kind: ref.kind, id: ref.id } : null;
    // Upstream readers (/profile, /profile-update, /profile-genstream, shared.js) treat
    // selectedProfile as "the selected profile", so a selected model leaves it empty.
    extension_settings.connectionManager.selectedProfile = ref?.kind === CONNECTION_ITEM_KIND.PROFILE ? ref.id : null;
}

/**
 * Resolves a managed item reference.
 * @param {ConnectionManagerItemRef|null} ref Item reference
 * @returns {{kind:string, item:ConnectionProfile|LlmModelTarget}|null}
 */
function resolveItemRef(ref) {
    if (!ref) {
        return null;
    }

    if (ref.kind === CONNECTION_ITEM_KIND.PROFILE) {
        const item = extension_settings.connectionManager.profiles.find(p => p.id === ref.id);
        return item ? { kind: ref.kind, item } : null;
    }

    if (ref.kind === CONNECTION_ITEM_KIND.MODEL_TARGET) {
        const item = extension_settings.connectionManager.modelTargets.find(t => t.id === ref.id);
        return item ? { kind: ref.kind, item } : null;
    }

    throw new Error(`Unknown connection manager item kind: ${ref.kind}`);
}

/**
 * Resolves the currently selected managed item.
 * @returns {{kind:string, item:ConnectionProfile|LlmModelTarget}|null}
 */
function getSelectedItem() {
    return resolveItemRef(getSelectedItemRef());
}

/**
 * Gets the currently selected option value if the referenced item still exists.
 * @returns {string}
 */
function getSelectedOptionValue() {
    const selected = getSelectedItem();
    return selected ? makeItemOptionValue(selected.kind, selected.item.id) : '';
}

/**
 * Whether a profile was saved with "Save Model + Preset". Other profiles (upstream-style)
 * keep their full field checklist when edited.
 * @param {ConnectionProfile} profile Connection profile
 * @returns {boolean}
 */
function isModelAndPresetProfile(profile) {
    return profile.extensions?.tauritavern?.kind === MODEL_AND_PRESET_PROFILE_KIND;
}

/**
 * Marks a profile as "model + preset"; the marker survives `/profile-update` and edits,
 * which keep fields they do not know.
 * @param {ConnectionProfile} profile Connection profile
 */
function markModelAndPresetProfile(profile) {
    profile.extensions = {
        ...profile.extensions,
        tauritavern: { ...profile.extensions?.tauritavern, kind: MODEL_AND_PRESET_PROFILE_KIND },
    };
}

/**
 * Migrates legacy selection state, and once marks "model + preset" profiles saved before
 * the marker existed (they were told apart by their exclude list).
 */
function normalizeConnectionManagerSettings() {
    const settings = extension_settings.connectionManager;
    if (!settings.selectedItem && settings.selectedProfile) {
        settings.selectedItem = { kind: CONNECTION_ITEM_KIND.PROFILE, id: settings.selectedProfile };
    }
    if (settings.selectedItem?.kind) {
        settings.selectedProfile = settings.selectedItem.kind === CONNECTION_ITEM_KIND.PROFILE ? settings.selectedItem.id : null;
    }
}

/**
 * Whether the selected item decides the connection: a saved model always does, a
 * profile when it records the API. A profile without one (say, preset only) leaves the
 * connection to the upstream "bind presets to connections" toggle.
 * @param {{kind:string, item:ConnectionProfile|LlmModelTarget}|null} selected Selected item
 * @returns {boolean}
 */
function itemOwnsConnection(selected) {
    if (!selected) {
        return false;
    }
    return selected.kind === CONNECTION_ITEM_KIND.MODEL_TARGET || Boolean(selected.item.api);
}

/**
 * How an item reads as a profile: profiles as they are, saved models as the read-only
 * profile view every profile-based caller uses.
 * @param {{kind:string, item:ConnectionProfile|LlmModelTarget}} selected Item
 * @returns {ConnectionProfile}
 */
function itemAsProfile(selected) {
    return selected.kind === CONNECTION_ITEM_KIND.PROFILE
        ? /** @type {ConnectionProfile} */ (selected.item)
        : modelTargetAsProfile(/** @type {LlmModelTarget} */ (selected.item));
}

/**
 * Names are unique across both kinds, so the shared dropdown never shows two equal entries.
 * @param {string} name Candidate name
 * @param {object|null} [except] Item being renamed
 * @returns {boolean}
 */
function isItemNameTaken(name, except = null) {
    const { profiles, modelTargets } = extension_settings.connectionManager;
    return name === NONE || [...profiles, ...modelTargets].some(item => item !== except && item.name === name);
}

/**
 * Finds a profile or a saved model by name. Exact names win over fuzzy ones and
 * profiles win over models at the same level, so upstream `/profile` usage resolves
 * exactly as before.
 * @param {string} value Search value
 * @returns {{kind:string, item:ConnectionProfile|LlmModelTarget}|null}
 */
function findItemByName(value) {
    const { profiles, modelTargets } = extension_settings.connectionManager;
    const exactProfile = profiles.find(p => p.name === value);
    if (exactProfile) {
        return { kind: CONNECTION_ITEM_KIND.PROFILE, item: exactProfile };
    }
    const exactTarget = modelTargets.find(target => target.name === value);
    if (exactTarget) {
        return { kind: CONNECTION_ITEM_KIND.MODEL_TARGET, item: exactTarget };
    }
    const fuzzyProfile = findProfileByName(value);
    if (fuzzyProfile) {
        return { kind: CONNECTION_ITEM_KIND.PROFILE, item: fuzzyProfile };
    }
    const fuzzyTarget = new Fuse(modelTargets, { keys: ['name'] }).search(value)[0]?.item;
    return fuzzyTarget ? { kind: CONNECTION_ITEM_KIND.MODEL_TARGET, item: fuzzyTarget } : null;
}

/**
 * Finds a profile or a saved model for `/profile-genstream profile=`, with upstream's
 * fuzzy search for that argument (stricter than `/profile`); profiles come first on a tie.
 * @param {string} value Search value
 * @returns {{kind:string, item:ConnectionProfile|LlmModelTarget}|null}
 */
function findItemForGeneration(value) {
    const { profiles, modelTargets } = extension_settings.connectionManager;
    const items = [
        ...profiles.map(item => ({ kind: CONNECTION_ITEM_KIND.PROFILE, item })),
        ...modelTargets.map(item => ({ kind: CONNECTION_ITEM_KIND.MODEL_TARGET, item })),
    ];
    return performFuzzySearch('profile', items, [{ name: 'item.name', weight: 10 }], value)[0]?.item ?? null;
}

/**
 * Finds the best match for the search value.
 * @param {string} value Search value
 * @returns {ConnectionProfile|null} Best match or null
 */
function findProfileByName(value) {
    // Try to find exact match
    const profile = extension_settings.connectionManager.profiles.find(p => p.name === value);

    if (profile) {
        return profile;
    }

    // Try to find fuzzy match
    const fuse = new Fuse(extension_settings.connectionManager.profiles, { keys: ['name'] });
    const results = fuse.search(value);

    if (results.length === 0) {
        return null;
    }

    const bestMatch = results[0];
    return bestMatch.item;
}

/**
 * Reads the connection profile from the commands.
 * @param {string} mode Mode of the connection profile
 * @param {ConnectionProfile} profile Connection profile
 * @param {boolean} [cleanUp] Whether to clean up the profile
 */
async function readProfileFromCommands(mode, profile, cleanUp = false) {
    const commands = mode === 'cc' ? CC_COMMANDS : TC_COMMANDS;
    const opposingCommands = mode === 'cc' ? TC_COMMANDS : CC_COMMANDS;
    const excludeList = Array.isArray(profile.exclude) ? profile.exclude : [];
    for (const command of commands) {
        try {
            if (excludeList.includes(command)) {
                continue;
            }

            const allowEmpty = ALLOW_EMPTY.includes(command);
            const args = getNamedArguments();
            const result = await SlashCommandParser.commands[command].callback(args, '');
            if (result || (allowEmpty && result === '')) {
                profile[command] = result;
                continue;
            }
        } catch (error) {
            console.error(`Failed to execute command: ${command}`, error);
        }
    }

    if (cleanUp) {
        for (const command of commands) {
            if (command.endsWith('-state') && profile[command] === 'false') {
                delete profile[command.replace('-state', '')];
            }
        }
        for (const command of opposingCommands) {
            if (commands.includes(command)) {
                continue;
            }

            delete profile[command];
        }
    }
}

/**
 * Executes a slash command through the same route Connection Profiles use.
 * @param {string} command Command name
 * @param {string} [value] Unnamed argument
 * @param {object} [args] Named arguments
 * @returns {Promise<string>}
 */
async function executeManagedCommand(command, value = '', args = {}) {
    const slashCommand = SlashCommandParser.commands[command];
    if (!slashCommand) {
        throw new Error(`Slash command not found: ${command}`);
    }

    const result = await slashCommand.callback(getNamedArguments(args), value);
    return result?.toString() ?? '';
}

/**
 * Executes a slash command and requires it to produce a value.
 * @param {string} command Command name
 * @param {string} [value] Unnamed argument
 * @param {object} [args] Named arguments
 * @returns {Promise<string>}
 */
async function requireManagedCommand(command, value = '', args = {}) {
    const result = await executeManagedCommand(command, value, args);
    if (!result) {
        throw new Error(`Slash command /${command} did not return a value`);
    }
    return result;
}

/**
 * The slash commands whose values an item decides, with their arguments: for a saved
 * model the route it applies, for a profile the fields it recorded (legacy profiles may
 * record only a few; excluded fields are not stored).
 * @param {{kind:string, item:ConnectionProfile|LlmModelTarget}} selected Item
 * @returns {Array<[string, object]>}
 */
function recordedCommands({ kind, item }) {
    if (kind === CONNECTION_ITEM_KIND.MODEL_TARGET) {
        const target = /** @type {LlmModelTarget} */ (item);
        return [
            ['api', {}],
            ...(target.mode === 'cc' ? [['custom-api-format', {}], ['proxy', {}]] : []),
            ['api-url', {}],
            ['model', {}],
            ...(target.secretRef?.id ? [['secret-id', { key: target.secretRef.key }]] : []),
        ];
    }
    const commands = new Set(item.mode === 'cc' ? CC_COMMANDS : TC_COMMANDS);
    return [...commands]
        .filter(command => item[command] || (ALLOW_EMPTY.includes(command) && item[command] === ''))
        .map(command => [command, {}]);
}

/**
 * The live values of what an item records, read the way the item was captured: its
 * slash commands and its request hints. Drift compares two of these.
 * @param {{kind:string, item:ConnectionProfile|LlmModelTarget}} selected Item
 * @returns {Promise<string>}
 */
async function readItemFingerprint(selected) {
    const values = [];
    for (const [command, args] of recordedCommands(selected)) {
        // Its extension is off: applying the item cannot set it either.
        if (!SlashCommandParser.commands[command]) continue;
        values.push([command, await executeManagedCommand(command, '', args)]);
    }
    const route = hintRoute(selected.item);
    for (const [hint] of decidedHints(selected.item)) {
        values.push([hint.setting ?? hint.parameter, readHint(hint, oai_settings, route)]);
    }
    return JSON.stringify(values);
}

/**
 * Sets or removes an optional target field.
 * @param {object} target Target object
 * @param {string} key Field key
 * @param {string} value Field value
 */
function setOptionalField(target, key, value) {
    if (value) {
        target[key] = value;
    } else {
        delete target[key];
    }
}

/**
 * @typedef {object} RequestHint
 * @property {string} [setting] Chat Completion setting holding the value
 * @property {string} [parameter] Field of the endpoint's Additional Parameters holding the value
 * @property {string} [flag] Stored word when the (boolean) setting is on; nothing is stored when off
 * @property {boolean} [customOnly] Only recorded for Custom endpoints
 * @property {string} [format] Only recorded for this custom API format
 */

/**
 * Endpoint request shaping a model (or a "model + preset" profile) records in
 * `adapterHints`, so a selected item does not depend on whichever preset is loaded.
 * The keys are the `adapterHints` contract shared with shared.js and the Agent LLM
 * connections built from saved models.
 * @type {Readonly<Record<string, RequestHint>>}
 */
const REQUEST_HINTS = Object.freeze({
    promptPostProcessing: { setting: 'custom_prompt_post_processing' },
    customIncludeHeaders: { parameter: 'include_headers', customOnly: true },
    customIncludeBody: { parameter: 'include_body', customOnly: true },
    customExcludeBody: { parameter: 'exclude_body', customOnly: true },
    claudePromptCaching: { setting: 'custom_claude_prompt_caching', flag: 'enabled', customOnly: true, format: 'claude_messages' },
    openaiResponsesMode: { setting: 'custom_openai_responses_websocket', flag: 'websocket', customOnly: true, format: 'openai_responses' },
});

/**
 * The endpoint an item's hints belong to.
 * @param {ConnectionProfile|LlmModelTarget} item Model or profile
 * @returns {{ isCustom: boolean, format: string }}
 */
function hintRoute(item) {
    return {
        isCustom: String(item.api || '').startsWith('custom'),
        format: String(item['custom-api-format'] || 'openai_compat'),
    };
}

/**
 * Reads a hint from Chat Completion settings: the live ones, or a stored preset body.
 * Additional Parameters are kept per endpoint, so the item's endpoint picks the entry.
 * @param {RequestHint} hint Hint
 * @param {Record<string, any>} settings Settings or preset body
 * @param {{ format: string }} route Endpoint of the item
 * @returns {unknown}
 */
function readHint(hint, settings, route) {
    if (!hint.parameter) {
        return settings[hint.setting];
    }
    const entry = getAdditionalParametersForSource({
        chat_completion_source: 'custom',
        custom_api_format: route.format,
        additional_parameters_by_source: settings.additional_parameters_by_source,
    }, undefined, { create: false });
    return entry[hint.parameter];
}

/**
 * Writes a hint into the live settings and the control showing it, if any.
 * @param {RequestHint} hint Hint
 * @param {{ format: string }} route Endpoint of the item
 * @param {unknown} value Setting-level value
 */
function writeHint(hint, route, value) {
    if (hint.parameter) {
        const entry = getAdditionalParametersForSource({
            chat_completion_source: 'custom',
            custom_api_format: route.format,
            additional_parameters_by_source: oai_settings.additional_parameters_by_source,
        });
        entry[hint.parameter] = value;
        return;
    }
    oai_settings[hint.setting] = value;
    const [selector, , isCheckbox] = settingsToUpdate[hint.setting];
    if (isCheckbox) {
        $(selector).prop('checked', Boolean(value));
    } else {
        $(selector).val(String(value));
    }
}

/**
 * Records the request hints that apply to an item's endpoint from the live settings.
 * @param {ConnectionProfile|LlmModelTarget} item Model or "model + preset" profile to populate
 */
function readRequestHints(item) {
    const route = hintRoute(item);
    /** @type {Record<string, string>} */
    const hints = {};
    for (const [key, hint] of Object.entries(item.mode === 'cc' ? REQUEST_HINTS : {})) {
        if ((hint.customOnly && !route.isCustom) || (hint.format && hint.format !== route.format)) {
            continue;
        }
        const value = readHint(hint, oai_settings, route);
        if (hint.flag) {
            if (value) hints[key] = hint.flag;
        } else {
            // Recorded even when empty: an empty value is a choice, unlike a hint older models never saved.
            hints[key] = String(value ?? '');
        }
    }
    item.adapterHints = hints;
}

/**
 * The live values an item decides, as setting-level values. A flag is decided by every
 * item that records hints (absent means off); any other hint only once recorded, so a
 * model saved before that hint existed leaves it alone.
 * @param {ConnectionProfile|LlmModelTarget} item Model or profile
 * @returns {Array<[RequestHint, unknown]>}
 */
function decidedHints(item) {
    const hints = item.adapterHints;
    if (!hints || item.mode !== 'cc') {
        return [];
    }
    return Object.entries(REQUEST_HINTS).flatMap(([key, hint]) => {
        if (hint.flag) {
            return [[hint, hints[key] === hint.flag]];
        }
        return Object.hasOwn(hints, key) ? [[hint, hints[key]]] : [];
    });
}

/**
 * Hands the live request hints over to the item being applied (or to none): what the
 * previous item's application wrote goes back to what the loaded preset stores (empty or
 * off when it stores nothing), then the entered item writes its own.
 * @param {ConnectionProfile|LlmModelTarget|null} entering Item being applied, or null
 */
function switchRequestHints(entering) {
    if (hintsWriter) {
        const preset = getLoadedPresetBody() ?? {};
        // Reading Additional Parameters normalizes the entry it finds; keep the stored preset as it is.
        const stored = { ...preset, additional_parameters_by_source: structuredClone(preset.additional_parameters_by_source ?? {}) };
        const route = hintRoute(hintsWriter);
        for (const [hint] of decidedHints(hintsWriter)) {
            writeHint(hint, route, readHint(hint, stored, route) ?? (hint.flag ? false : ''));
        }
        hintsWriter = null;
    }
    const decided = entering ? decidedHints(entering) : [];
    if (entering && decided.length > 0) {
        const route = hintRoute(entering);
        for (const [hint, value] of decided) {
            writeHint(hint, route, value);
        }
        hintsWriter = entering;
    }
    saveSettingsDebounced();
}

/**
 * Reads the current UI state as a model-only target.
 * @param {LlmModelTarget} target Model target to populate
 * @returns {Promise<void>}
 */
async function readModelTargetFromCommands(target) {
    const mode = main_api === 'openai' ? 'cc' : 'tc';

    target.schemaVersion = MODEL_TARGET_SCHEMA_VERSION;
    target.kind = MODEL_TARGET_KIND;
    target.mode = mode;
    target.api = await requireManagedCommand('api');

    if (mode === 'cc') {
        setOptionalField(target, 'custom-api-format', await executeManagedCommand('custom-api-format'));
        setOptionalField(target, 'proxy', await executeManagedCommand('proxy'));
    } else {
        delete target['custom-api-format'];
        delete target.proxy;
    }

    setOptionalField(target, 'api-url', await executeManagedCommand('api-url', '', { quiet: 'true' }));
    target.model = await requireManagedCommand('model', '', { quiet: 'true' });
    readRequestHints(target);

    const secretKey = resolveSecretKey();
    if (secretKey) {
        const secretId = await executeManagedCommand('secret-id', '', { key: secretKey, quiet: 'true' });
        if (secretId) {
            const label = getSecretLabelById(secretId);
            target.secretRef = {
                key: secretKey,
                id: secretId,
                ...(label ? { labelSnapshot: label } : {}),
            };
            return;
        }
    }

    delete target.secretRef;
}

/**
 * Binds profile include/exclude checkbox changes.
 * @param {JQuery<HTMLElement>} template Popup template
 * @param {ConnectionProfile} profile Connection profile
 */
function bindProfileExcludeToggles(template, profile) {
    template.find('input[name="exclude"]').on('input', function () {
        const fancyName = String($(this).val());
        const keyName = Object.entries(FANCY_NAMES).find(x => x[1] === fancyName)?.[0];
        if (!keyName) {
            console.warn('Key not found for fancy name:', fancyName);
            return;
        }

        if (!Array.isArray(profile.exclude)) {
            profile.exclude = [];
        }

        const excludeState = !$(this).prop('checked');
        if (excludeState) {
            profile.exclude.push(keyName);
        } else {
            const index = profile.exclude.indexOf(keyName);
            index !== -1 && profile.exclude.splice(index, 1);
        }
    });
}

/**
 * Normalizes a popup name result.
 * @param {string|boolean|null} name Raw popup result
 * @returns {string|null}
 */
function normalizeItemName(name) {
    if (!name) {
        return null;
    }

    const normalized = DOMPurify.sanitize(String(name));
    if (!normalized) {
        toastr.error(t`Name cannot be empty.`);
        return null;
    }

    return normalized;
}

/**
 * Removes fields omitted from a connection profile.
 * @param {ConnectionProfile} profile Connection profile
 */
function removeExcludedProfileFields(profile) {
    if (!Array.isArray(profile.exclude)) {
        return;
    }

    for (const command of profile.exclude) {
        delete profile[command];
    }
}

/**
 * Creates a connection profile snapshot from the current settings.
 * @returns {Promise<ConnectionProfile>}
 */
async function createConnectionProfileSnapshot() {
    const mode = main_api === 'openai' ? 'cc' : 'tc';
    const profile = {
        id: uuidv4(),
        mode,
        exclude: [],
    };

    await readProfileFromCommands(mode, profile);
    return profile;
}

/**
 * Creates a model target snapshot from the current settings.
 * @param {string} name Model target name
 * @returns {Promise<LlmModelTarget>}
 */
async function createModelTarget(name) {
    const target = {
        schemaVersion: MODEL_TARGET_SCHEMA_VERSION,
        kind: MODEL_TARGET_KIND,
        id: uuidv4(),
        mode: main_api === 'openai' ? 'cc' : 'tc',
        name: String(name),
    };

    await readModelTargetFromCommands(target);
    return target;
}

/**
 * Creates a new connection profile.
 * @param {string} [forceName] Name of the connection profile
 * @returns {Promise<ConnectionProfile>} Created connection profile
 */
async function createConnectionProfile(forceName = null) {
    const profile = await createConnectionProfileSnapshot();

    const profileForDisplay = makeFancyProfile(profile);
    const template = $(await renderExtensionTemplateAsync(MODULE_NAME, 'profile', { profile: profileForDisplay }));
    bindProfileExcludeToggles(template, profile);
    const suggestedName = getUniqueName(collapseSpaces(`${profile.api ?? ''} ${profile.model ?? ''} - ${profile.preset ?? ''}`), isItemNameTaken);
    const name = normalizeItemName(forceName ?? await callGenericPopup(template, POPUP_TYPE.INPUT, suggestedName));
    if (!name) {
        return null;
    }

    if (isItemNameTaken(name)) {
        toastr.error(t`This name is already in use.`);
        return null;
    }

    removeExcludedProfileFields(profile);
    profile.name = String(name);
    return profile;
}

/**
 * Creates a connection-manager item from the current settings: a model target
 * (model route only) or an ordinary upstream connection profile holding the
 * model route plus the settings preset. The other profile fields are global
 * formatting settings (stop strings, reply prefix, reasoning template, regex
 * state) that stay out of both; `/profile-create` still captures them.
 * @returns {Promise<{kind:string, item:ConnectionProfile|LlmModelTarget}|null>}
 */
async function createConnectionItem() {
    const mode = main_api === 'openai' ? 'cc' : 'tc';
    const suggestedName = getUniqueName(
        collapseSpaces(await executeManagedCommand('model', '', { quiet: 'true' })),
        isItemNameTaken,
        // Start at 0 so a free model name is suggested as is, not as "name (1)".
        { startIndex: 0 },
    );
    const popup = new Popup(`<h3>${t`Enter a name:`}</h3>`, POPUP_TYPE.INPUT, suggestedName, {
        okButton: false,
        // Enter saves a model; a name comes back only for one of the two buttons.
        defaultResult: CREATE_MODEL_TARGET_RESULT,
        customButtons: [
            {
                text: t`Save Model`,
                result: CREATE_MODEL_TARGET_RESULT,
                classes: ['popup-button-ok'],
                tooltip: t`Save the connection only: API, server URL, model, proxy, secret and request settings.`,
            },
            {
                text: t`Save Model + Preset`,
                result: CREATE_MODEL_AND_PRESET_RESULT,
                classes: ['popup-button-ok'],
                tooltip: t`Save the model together with the current settings preset.`,
            },
        ],
    });
    const name = normalizeItemName(await popup.show());
    if (!name) {
        return null;
    }
    if (isItemNameTaken(name)) {
        toastr.error(t`This name is already in use.`);
        return null;
    }

    if (popup.result === CREATE_MODEL_TARGET_RESULT) {
        return { kind: CONNECTION_ITEM_KIND.MODEL_TARGET, item: await createModelTarget(name) };
    }
    return { kind: CONNECTION_ITEM_KIND.PROFILE, item: await captureProfile(mode, name, { exclude: getModelAndPresetExclude(), modelAndPreset: true }) };
}

/**
 * Captures a profile from the current settings.
 * @param {string} mode Profile mode
 * @param {string} name Profile name
 * @param {{ exclude: string[], modelAndPreset: boolean }} shape Commands left out, and whether it is a "model + preset" item
 * @returns {Promise<ConnectionProfile>}
 */
async function captureProfile(mode, name, { exclude, modelAndPreset }) {
    /** @type {ConnectionProfile} */
    const profile = { id: uuidv4(), mode, name: String(name), exclude: [...exclude] };
    if (modelAndPreset) {
        markModelAndPresetProfile(profile);
    }
    // Excluded commands are not read, so only what the shape records is captured.
    await readProfileFromCommands(mode, profile);
    if (modelAndPreset) {
        readRequestHints(profile);
    }
    return profile;
}

/**
 * Saves the current settings as a new item shaped like the selected one: a model stays a
 * model, a "model + preset" item stays one, and an older profile keeps the fields it records.
 * @param {{kind:string, item:ConnectionProfile|LlmModelTarget}} selected Item to copy the shape of
 * @returns {Promise<{kind:string, item:ConnectionProfile|LlmModelTarget}|null>}
 */
async function saveItemAs(selected) {
    const suggestedName = getUniqueName(selected.item.name, isItemNameTaken);
    const name = normalizeItemName(await Popup.show.input(t`Enter a name:`, null, suggestedName));
    if (!name) {
        return null;
    }
    if (isItemNameTaken(name)) {
        toastr.error(t`This name is already in use.`);
        return null;
    }
    if (selected.kind === CONNECTION_ITEM_KIND.MODEL_TARGET) {
        return { kind: CONNECTION_ITEM_KIND.MODEL_TARGET, item: await createModelTarget(name) };
    }
    const profile = /** @type {ConnectionProfile} */ (selected.item);
    const mode = main_api === 'openai' ? 'cc' : 'tc';
    const shape = { exclude: Array.isArray(profile.exclude) ? profile.exclude : [], modelAndPreset: isModelAndPresetProfile(profile) };
    return { kind: CONNECTION_ITEM_KIND.PROFILE, item: await captureProfile(mode, name, shape) };
}

/**
 * Deletes the selected connection profile.
 * @returns {Promise<boolean>}
 */
async function deleteConnectionProfile() {
    const selectedProfile = extension_settings.connectionManager.selectedProfile;
    if (!selectedProfile) {
        return false;
    }

    const index = extension_settings.connectionManager.profiles.findIndex(p => p.id === selectedProfile);
    if (index === -1) {
        return false;
    }

    const profile = extension_settings.connectionManager.profiles[index];
    const name = profile.name;
    const confirm = await Popup.show.confirm(t`Are you sure you want to delete the selected profile?`, name);

    if (!confirm) {
        return false;
    }

    extension_settings.connectionManager.profiles.splice(index, 1);
    setSelectedItemRef(null);
    saveSettingsDebounced();

    await eventSource.emit(event_types.CONNECTION_PROFILE_DELETED, profile);
    return true;
}

/**
 * Names of the Agent profiles, including the shared Session profile, bound to a saved model.
 * @param {LlmModelTarget} target Saved model
 * @returns {Promise<string[]>}
 */
async function listAgentProfilesUsing(target) {
    const agent = window.__TAURITAVERN__?.api?.agent;
    if (!agent) {
        throw new Error('The Agent API is not available');
    }
    const connectionRef = modelTargetConnectionRef(target);
    const usesTarget = (profile) => profile?.model?.mode === 'connectionRef' && profile.model.connectionRef === connectionRef;
    const { profiles } = await agent.profiles.list();
    const loaded = await Promise.all(profiles.map(({ id }) => agent.profiles.load({ profileId: id })));
    const names = loaded.map(({ profile }) => profile).filter(usesTarget).map(profile => profile.displayName);
    const { profile: sessionProfile } = await agent.sessions.profile.load();
    if (usesTarget(sessionProfile)) {
        names.push(sessionProfile.displayName);
    }
    return names;
}

/**
 * Deletes the selected model target.
 * @returns {Promise<boolean>}
 */
async function deleteModelTarget() {
    const selected = getSelectedItem();
    if (selected?.kind !== CONNECTION_ITEM_KIND.MODEL_TARGET) {
        return false;
    }

    const index = extension_settings.connectionManager.modelTargets.findIndex(t => t.id === selected.item.id);
    if (index === -1) {
        return false;
    }

    const target = extension_settings.connectionManager.modelTargets[index];
    let agentProfiles;
    try {
        agentProfiles = await listAgentProfilesUsing(target);
    } catch (error) {
        throw new Error(t`Could not check which Agent profiles use this model: ${error?.message ?? error}`);
    }
    const usage = agentProfiles.length > 0
        ? `<p>${t`These Agent profiles use it and will report the model as missing:`}</p><p>${DOMPurify.sanitize(agentProfiles.join(', '))}</p>`
        : '';
    const confirm = await Popup.show.confirm(t`Are you sure you want to delete the selected model?`, `${DOMPurify.sanitize(target.name)}${usage}`);

    if (!confirm) {
        return false;
    }

    extension_settings.connectionManager.modelTargets.splice(index, 1);
    setSelectedItemRef(null);
    saveSettingsDebounced();

    await eventSource.emit(event_types.MODEL_TARGET_DELETED, target);
    return true;
}

/**
 * Formats the connection profile for display.
 * @param {ConnectionProfile} profile Connection profile
 * @returns {Object} Fancy profile
 */
function makeFancyProfile(profile) {
    return Object.entries(FANCY_NAMES).reduce((acc, [key, value]) => {
        const allowEmpty = ALLOW_EMPTY.includes(key);
        if (!profile[key]) {
            if (profile[key] === '' && allowEmpty) {
                acc[value] = EMPTY;
            }
            return acc;
        }

        // UUID is not very useful in the UI, so we replace it with a label (if available)
        if (key === 'secret-id') {
            const label = getSecretLabelById(profile[key]);
            if (label) {
                acc[value] = label;
                return acc;
            }
        }

        if (key === 'regex-preset') {
            const label = extension_settings.regex_presets?.find(p => p.id === profile[key])?.name;
            if (label) {
                acc[value] = label;
                return acc;
            }
        }

        acc[value] = profile[key];
        return acc;
    }, {});
}

/**
 * Formats a model target for display.
 * @param {LlmModelTarget} target Model target
 * @returns {Object} Fancy model target
 */
function makeFancyModelTarget(target) {
    const result = {};
    const fields = ['api', 'custom-api-format', 'api-url', 'model', 'proxy'];

    for (const field of fields) {
        if (!target[field] || (field === 'proxy' && target[field] === NO_PROXY_PRESET)) {
            continue;
        }
        result[FANCY_NAMES[field]] = target[field];
    }

    if (target.secretRef?.id) {
        result[FANCY_NAMES['secret-id']] = target.secretRef.labelSnapshot || getSecretLabelById(target.secretRef.id) || target.secretRef.id;
    }
    if (target.adapterHints?.promptPostProcessing) {
        result[FANCY_NAMES['prompt-post-processing']] = target.adapterHints.promptPostProcessing;
    }
    if (target.adapterHints?.claudePromptCaching === 'enabled') {
        result['Claude Prompt Caching'] = t`Enabled`;
    }
    if (target.adapterHints?.openaiResponsesMode === 'websocket') {
        result['Responses API Mode'] = 'WebSocket';
    }

    return result;
}

/**
 * Marks a superseded application, which callers drop silently instead of reporting.
 * @param {string} message Error message
 * @returns {Error}
 */
function applicationAborted(message) {
    const error = new Error(message);
    error.name = 'AbortError';
    return error;
}

/**
 * Asserts that a model target can be applied. Runs before any setting changes, so a
 * missing key or proxy preset fails without leaving a half-switched connection.
 * @param {LlmModelTarget} target Model target
 */
function assertModelTargetCanApply(target) {
    if (target.kind !== MODEL_TARGET_KIND) {
        throw new Error(`Invalid model target kind: ${target.kind}`);
    }
    if (!target.api) {
        throw new Error(`Model target "${target.name}" is missing API`);
    }
    if (!target.model) {
        throw new Error(`Model target "${target.name}" is missing model`);
    }
    const { secretRef } = target;
    if (secretRef?.id && !secret_state[secretRef.key]?.some(secret => secret.id === secretRef.id)) {
        throw new Error(t`The key saved with model "${target.name}" no longer exists. Select a key and update the model.`);
    }
    if (target.proxy && target.proxy !== NO_PROXY_PRESET && !proxies.some(proxy => proxy.name === target.proxy)) {
        throw new Error(t`The proxy preset "${target.proxy}" saved with model "${target.name}" no longer exists.`);
    }
}

/**
 * Runs a UI action and reports its failure, instead of leaving an unhandled rejection.
 * @param {() => Promise<void>} action UI action
 * @returns {Promise<boolean>} Whether the action succeeded (a superseded application counts as success)
 */
async function runUiAction(action) {
    try {
        await action();
        return true;
    } catch (error) {
        if (error?.name === 'AbortError') {
            return true;
        }
        console.error('Connection Manager action failed', error);
        toastr.error(String(error?.message ?? error));
        return false;
    }
}

/**
 * Applies a model target without changing preset or prompt-formatting settings.
 * @param {LlmModelTarget} target Model target
 * @returns {Promise<void>}
 */
async function applyModelTarget(target) {
    if (!target) {
        return;
    }
    assertModelTargetCanApply(target);

    const applicationVersion = ++profileApplicationVersion;
    ConnectionManagerSpinner.abort();
    const previousApplication = profileApplicationQueue;
    const application = (async () => {
        await previousApplication;

        if (applicationVersion !== profileApplicationVersion) {
            throw applicationAborted('Model target application aborted');
        }

        const spinner = new ConnectionManagerSpinner();
        spinner.start();

        try {
            await withConnectionValidationSuspended('Model target application', async () => {
                await requireManagedCommand('api', target.api);

                if (target.api === 'vertexai') {
                    const mode = target.secretRef?.key === SECRET_KEYS.VERTEXAI_SERVICE_ACCOUNT ? 'full' : 'express';
                    $('#vertexai_auth_mode').val(mode).trigger('change');
                }

                if (target['custom-api-format']) {
                    await requireManagedCommand('custom-api-format', target['custom-api-format']);
                } else if (target.api === 'custom') {
                    // /api custom intentionally preserves the current custom format for full profiles; model targets must not inherit it.
                    await requireManagedCommand('custom-api-format', 'openai_compat');
                }

                if (target['api-url']) {
                    await requireManagedCommand('api-url', target['api-url'], { connect: 'false', quiet: 'true' });
                } else {
                    // A missing route field is part of the target snapshot, not an instruction to keep a previous proxy/server URL.
                    await executeManagedCommand('api-url', '', { connect: 'false', quiet: 'true', clear: 'true' });
                }

                if (target.secretRef?.id) {
                    await requireManagedCommand('secret-id', target.secretRef.id, { key: target.secretRef.key, quiet: 'true' });
                }

                if (target.proxy) {
                    await requireManagedCommand('proxy', target.proxy);
                } else {
                    await requireManagedCommand('proxy', NO_PROXY_PRESET);
                }

                await requireManagedCommand('model', target.model, { quiet: 'true' });
                switchRequestHints(target);
            });
        } finally {
            spinner.stop();
        }

        if (applicationVersion === profileApplicationVersion) {
            connectCurrentApi();
        }
    })();

    profileApplicationQueue = application.catch(() => {});
    return application;
}

/**
 * Applies the connection profile.
 * @param {ConnectionProfile} profile Connection profile
 * @returns {Promise<void>}
 */
async function applyConnectionProfile(profile) {
    if (!profile) {
        return;
    }

    // Abort in-flight replay work and let the queued latest application own the final validation.
    const applicationVersion = ++profileApplicationVersion;
    ConnectionManagerSpinner.abort();
    const previousApplication = profileApplicationQueue;
    const application = (async () => {
        await previousApplication;

        if (applicationVersion !== profileApplicationVersion) {
            throw applicationAborted('Profile application aborted');
        }

        const mode = profile.mode;
        const commands = mode === 'cc' ? CC_COMMANDS : TC_COMMANDS;
        const spinner = new ConnectionManagerSpinner();
        spinner.start();

        try {
            await withConnectionValidationSuspended('Connection profile application', async () => {
                for (const command of commands) {
                    if (spinner.isAborted() || applicationVersion !== profileApplicationVersion) {
                        throw applicationAborted('Profile application aborted');
                    }

                    const argument = profile[command];
                    const allowEmpty = ALLOW_EMPTY.includes(command);
                    if (!argument && !(allowEmpty && argument === '')) {
                        continue;
                    }

                    try {
                        const commandArgs = allowEmpty ? { force: 'true' } : {};
                        if (command === 'api-url') {
                            // The final connect below validates the fully applied profile once.
                            commandArgs.connect = 'false';
                        }
                        const args = getNamedArguments(commandArgs);
                        await SlashCommandParser.commands[command].callback(args, argument);
                    } catch (error) {
                        console.error(`Failed to execute command: ${command} ${argument}`, error);
                    }
                }
                switchRequestHints(profile);
            });
        } finally {
            spinner.stop();
        }

        if (applicationVersion === profileApplicationVersion) {
            // Validate only after all profile fields, including custom format and secret id, have settled.
            connectCurrentApi();
        }
    })();

    // Keep later applications queued even when an older replay is aborted or fails.
    profileApplicationQueue = application.catch(() => {});
    return application;
}

/**
 * Updates the selected connection profile.
 * @param {ConnectionProfile} profile Connection profile
 * @returns {Promise<void>}
 */
async function updateConnectionProfile(profile) {
    profile.mode = main_api === 'openai' ? 'cc' : 'tc';
    await readProfileFromCommands(profile.mode, profile, true);
    if (isModelAndPresetProfile(profile)) {
        readRequestHints(profile);
    }
}

/**
 * Updates a model target from the current settings.
 * @param {LlmModelTarget} target Model target
 * @returns {Promise<void>}
 */
async function updateModelTarget(target) {
    const mode = main_api === 'openai' ? 'cc' : 'tc';
    if (target.mode && target.mode !== mode) {
        throw new Error(t`This model was saved for another API type. Switch the API type back before updating it.`);
    }
    await readModelTargetFromCommands(target);
}

/**
 * Overwrites an item with the current settings and announces the update.
 * @param {{kind:string, item:ConnectionProfile|LlmModelTarget}} selected Item to overwrite
 */
async function overwriteItem(selected) {
    const oldItem = structuredClone(selected.item);
    if (selected.kind === CONNECTION_ITEM_KIND.PROFILE) {
        await updateConnectionProfile(/** @type {ConnectionProfile} */ (selected.item));
        await eventSource.emit(event_types.CONNECTION_PROFILE_UPDATED, oldItem, selected.item);
    } else {
        await updateModelTarget(/** @type {LlmModelTarget} */ (selected.item));
        await eventSource.emit(event_types.MODEL_TARGET_UPDATED, oldItem, selected.item);
    }
    // The item now describes the live settings.
    trackSelectedItem();
}

/**
 * Renames a model or a "model + preset" item. Overwriting has its own action.
 * @param {{kind:string, item:ConnectionProfile|LlmModelTarget}} selected Item to rename
 * @returns {Promise<boolean>} Whether anything changed
 */
async function renameItem(selected) {
    const { item } = selected;
    const newName = normalizeItemName(await Popup.show.input(t`Name:`, null, item.name));
    if (!newName || newName === item.name) {
        return false;
    }
    if (isItemNameTaken(newName, item)) {
        toastr.error(t`This name is already in use.`);
        return false;
    }

    const oldItem = structuredClone(item);
    item.name = newName;
    const updated = selected.kind === CONNECTION_ITEM_KIND.PROFILE ? event_types.CONNECTION_PROFILE_UPDATED : event_types.MODEL_TARGET_UPDATED;
    await eventSource.emit(updated, oldItem, item);
    toastr.success(t`Renamed.`);
    return true;
}

/**
 * Upstream editor for full profiles: pick the recorded settings and rename.
 * @param {ConnectionProfile} profile Connection profile
 * @returns {Promise<boolean>} Whether anything changed
 */
async function editLegacyProfile(profile) {
    if (!Array.isArray(profile.exclude)) {
        profile.exclude = [];
    }

    const sortByViewOrder = (a, b) => Object.keys(FANCY_NAMES).indexOf(a) - Object.keys(FANCY_NAMES).indexOf(b);
    const commands = profile.mode === 'cc' ? CC_COMMANDS : TC_COMMANDS;
    const settings = commands.slice().sort(sortByViewOrder).reduce((acc, command) => {
        acc[FANCY_NAMES[command]] = !profile.exclude.includes(command);
        return acc;
    }, {});
    const template = $(await renderExtensionTemplateAsync(MODULE_NAME, 'edit', { name: profile.name, settings }));
    const popup = new Popup(template, POPUP_TYPE.INPUT, profile.name, {
        customButtons: [{
            text: t`Save and Update`,
            classes: ['popup-button-ok'],
            result: POPUP_RESULT.CUSTOM1,
        }],
    });

    const newName = normalizeItemName(await popup.show());
    if (!newName) {
        return false;
    }
    if (isItemNameTaken(newName, profile)) {
        toastr.error(t`This name is already in use.`);
        return false;
    }

    const newExcludeList = template.find('input[name="exclude"]:not(:checked)').map(function () {
        return Object.entries(FANCY_NAMES).find(x => x[1] === String($(this).val()))?.[0];
    }).get();

    const oldProfile = structuredClone(profile);
    const excludeChanged = newExcludeList.length !== profile.exclude.length || !newExcludeList.every(e => profile.exclude.includes(e));
    if (excludeChanged) {
        profile.exclude = newExcludeList;
        for (const command of newExcludeList) {
            delete profile[command];
        }
    }
    const update = popup.result === POPUP_RESULT.CUSTOM1;
    if (update) {
        await updateConnectionProfile(profile);
    } else if (excludeChanged) {
        toastr.info(t`Press 💾 to record them into the profile.`, t`Included settings list updated`);
    }
    if (update || excludeChanged) {
        // The profile records other fields now.
        trackSelectedItem();
    }
    if (profile.name !== newName) {
        profile.name = newName;
        toastr.success(t`Renamed.`);
    }

    await eventSource.emit(event_types.CONNECTION_PROFILE_UPDATED, oldProfile, profile);
    return true;
}

/**
 * Appends an option group, if it has any entries.
 * @param {HTMLSelectElement} select Selector
 * @param {string} label Group label
 * @param {Array<{kind:string, item:ConnectionProfile|LlmModelTarget}>} entries Items
 * @param {string} selectedValue Selected option value
 */
function appendItemGroup(select, label, entries, selectedValue) {
    if (entries.length === 0) {
        return;
    }
    const group = document.createElement('optgroup');
    group.label = label;
    for (const { kind, item } of entries.sort((a, b) => a.item.name.localeCompare(b.item.name))) {
        const option = document.createElement('option');
        option.value = makeItemOptionValue(kind, item.id);
        // Just the name: extensions read the option text as the profile name.
        option.textContent = item.name;
        option.selected = option.value === selectedValue;
        group.appendChild(option);
    }
    select.appendChild(group);
}

/**
 * Renders the selector: "Models" (saved models, and profiles that store no preset)
 * first, then "Models + Presets", the profiles that also switch the preset.
 * @param {HTMLSelectElement} profiles Selector
 */
function renderConnectionProfiles(profiles) {
    const { modelTargets, profiles: connectionProfiles } = extension_settings.connectionManager;
    const selectedValue = getSelectedOptionValue();
    profiles.innerHTML = '';

    const noneOption = document.createElement('option');
    noneOption.value = '';
    noneOption.textContent = t`(Current API settings)`;
    noneOption.selected = !selectedValue;
    profiles.appendChild(noneOption);

    const asEntries = (kind, items) => items.map(item => ({ kind, item }));
    appendItemGroup(profiles, t`Models`, [
        ...asEntries(CONNECTION_ITEM_KIND.MODEL_TARGET, modelTargets),
        ...asEntries(CONNECTION_ITEM_KIND.PROFILE, connectionProfiles.filter(profile => !profile.preset)),
    ], selectedValue);
    appendItemGroup(profiles, t`Models + Presets`, asEntries(CONNECTION_ITEM_KIND.PROFILE, connectionProfiles.filter(profile => profile.preset)), selectedValue);

    requireUi().sidebar.sync();
}

/**
 * Renders the content of the details element.
 * @param {HTMLElement} detailsContent Content element of the details
 */
async function renderDetailsContent(detailsContent) {
    detailsContent.innerHTML = '';
    if (detailsContent.classList.contains('hidden')) {
        return;
    }
    const selected = getSelectedItem();
    if (selected?.kind === CONNECTION_ITEM_KIND.PROFILE) {
        const profileForDisplay = makeFancyProfile(selected.item);
        const templateParams = { profile: profileForDisplay };
        // A model + preset profile excludes everything else by definition; listing it is noise.
        if (!isModelAndPresetProfile(selected.item) && Array.isArray(selected.item.exclude) && selected.item.exclude.length > 0) {
            templateParams.omitted = selected.item.exclude.map(e => FANCY_NAMES[e]).join(', ');
        }
        const template = await renderExtensionTemplateAsync(MODULE_NAME, 'view', templateParams);
        detailsContent.innerHTML = template;
    } else if (selected?.kind === CONNECTION_ITEM_KIND.MODEL_TARGET) {
        const template = await renderExtensionTemplateAsync(MODULE_NAME, 'view', { profile: makeFancyModelTarget(selected.item) });
        detailsContent.innerHTML = template;
    } else {
        detailsContent.textContent = t`No model selected`;
    }
}

/**
 * Callback for the /profile-genstream command.
 * Generates text using Connection Manager with streaming display support.
 * @param {object} args Named arguments
 * @param {string} value Unnamed argument (the prompt)
 * @returns {Promise<string>} The generated text, optionally with formatted reasoning
 */
async function generateStreamCallback(args, value) {
    if (!value) {
        console.warn('WARN: No argument provided for /profile-genstream command');
        return '';
    }

    const context = getContext();
    if (context.extensionSettings.disabledExtensions.includes('connection-manager')) {
        toastr.error(t`Connection Manager is required for /profile-genstream. Use /gen or /genraw instead.`);
        return '';
    }

    const profileIdOrName = args?.profile;
    const includeReasoning = isTrueBoolean(args?.reasoning);
    const systemPrompt = typeof args?.system == 'string' ? args.system : '';
    const maxTokens = Number(args?.length ?? 2048) || 2048;
    const lock = isTrueBoolean(args?.lock);
    const generatingLabel = typeof args?.generating === 'string' ? args.generating : 'Generating...';
    const completedLabel = typeof args?.completed === 'string' ? args.completed : 'Generated';
    const enableStop = !isFalseBoolean(args?.stop);
    const onStopClosure = args?.onStop instanceof SlashCommandClosure ? args.onStop : null;
    const onCompleteClosure = args?.onComplete instanceof SlashCommandClosure ? args.onComplete : null;

    let completeDelay = 3000;
    if (args?.delay !== undefined) {
        if (typeof args.delay === 'string' && args.delay.toLowerCase() === 'infinite') {
            completeDelay = null;
        } else {
            const parsed = Number(args.delay);
            if (!isNaN(parsed) && parsed >= 0) {
                completeDelay = parsed;
            } else if (!isNaN(parsed) && parsed < 0) {
                completeDelay = null;
            }
        }
    }

    const abortController = enableStop ? new AbortController() : null;
    const onStopHandler = enableStop ? async () => {
        abortController.abort();
        if (onStopClosure) {
            try {
                const localClosure = onStopClosure.getCopy();
                localClosure.onProgress = () => { };
                await localClosure.execute();
            } catch (e) {
                console.error('[GenStream] Error executing onStop closure', e);
            }
        }
    } : null;

    try {
        if (lock) {
            deactivateSendButtons();
        }

        let effectiveProfileId = getSelectedConnectionItemId();

        if (profileIdOrName) {
            const byId = ConnectionManagerRequestService.findProfile(profileIdOrName);
            const byName = byId ? null : findItemForGeneration(profileIdOrName);
            if (byId) {
                effectiveProfileId = byId.id;
            } else if (byName) {
                effectiveProfileId = makeItemOptionValue(byName.kind, byName.item.id);
            } else {
                toastr.warning(t`Connection profile not found: ${profileIdOrName}`);
                return '';
            }
        }

        if (!effectiveProfileId) {
            toastr.error(t`No connection profile specified or selected. Use profile= argument or select a profile in Connection Manager.`);
            return '';
        }

        const effectiveProfile = ConnectionManagerRequestService.getProfile(effectiveProfileId);
        const selectedApiMap = ConnectionManagerRequestService.validateProfile(effectiveProfile);
        if (globalThis.__TAURI_RUNNING__ === true && selectedApiMap.selected === 'textgenerationwebui') {
            throw new Error('Text Completion profiles are not supported by the native TauriTavern generation backend yet. Use a Chat Completion profile.');
        }

        const display = new StreamingDisplay();
        display.show({
            label: generatingLabel,
            icon: ConnectionManagerRequestService.getProfileIcon(effectiveProfileId),
            onStop: onStopHandler,
        });

        const messages = [
            ...(systemPrompt ? [{ role: 'system', content: systemPrompt }] : []),
            { role: 'user', content: value },
        ];

        let finalText = '';
        let finalReasoning = '';

        function buildResultText() {
            if (includeReasoning && finalReasoning) {
                const { formatted } = formatReasoning(finalReasoning, finalText);
                return formatted;
            }

            return finalText;
        }

        try {
            const streamResponse = await ConnectionManagerRequestService.sendRequest(
                effectiveProfileId,
                messages,
                maxTokens,
                { extractData: true, includePreset: true, stream: true, signal: abortController?.signal ?? undefined },
            );

            if (typeof streamResponse === 'function') {
                const generator = streamResponse();
                for await (const chunk of generator) {
                    finalText = chunk.text;
                    finalReasoning = chunk.state?.reasoning || '';
                    display.updateReasoning(finalReasoning);
                    display.updateContent(finalText);
                }
            } else {
                finalText = streamResponse?.content || '';
                finalReasoning = streamResponse?.reasoning || '';
                if (finalReasoning) {
                    display.updateReasoning(finalReasoning);
                }
                display.updateContent(finalText);
            }
        } catch (error) {
            if (abortController?.signal?.aborted) {
                display.markStopped({ label: `${generatingLabel} [Stopped]` });
                return buildResultText();
            }

            console.warn('[Slash Commands] Streaming failed, falling back to non-streaming:', error);
            display.hide({ instant: true });

            const response = await ConnectionManagerRequestService.sendRequest(
                effectiveProfileId,
                messages,
                maxTokens,
                { extractData: true, includePreset: true, stream: false },
            );

            finalText = response?.content || '';
            finalReasoning = response?.reasoning || '';

            display.show({
                label: generatingLabel,
                icon: ConnectionManagerRequestService.getProfileIcon(effectiveProfileId),
            });
            if (finalReasoning) {
                display.updateReasoning(finalReasoning);
            }
            display.updateContent(finalText);
        }

        display.complete({ label: completedLabel, delay: completeDelay });

        if (onCompleteClosure) {
            try {
                const localClosure = onCompleteClosure.getCopy();
                localClosure.onProgress = () => { };
                await localClosure.execute();
            } catch (e) {
                console.error('[GenStream] Error executing onComplete closure', e);
            }
        }

        if (!finalText) {
            toastr.warning(t`Generation returned empty result`);
            return '';
        }

        return buildResultText();
    } catch (err) {
        console.error('Error on /profile-genstream generation', err);
        toastr.error(err.message, t`API Error`, { preventDuplicates: true });
        return '';
    } finally {
        if (lock) {
            activateSendButtons();
        }
    }
}

export async function init() {
    extension_settings.connectionManager = extension_settings.connectionManager || structuredClone(DEFAULT_SETTINGS);

    for (const key of Object.keys(DEFAULT_SETTINGS)) {
        if (extension_settings.connectionManager[key] === undefined) {
            extension_settings.connectionManager[key] = DEFAULT_SETTINGS[key];
        }
    }
    normalizeConnectionManagerSettings();

    const container = document.getElementById('rm_api_block');
    const settings = await renderExtensionTemplateAsync(MODULE_NAME, 'settings');
    container.insertAdjacentHTML('afterbegin', settings);

    /** @type {HTMLSelectElement} */
    // @ts-ignore
    const profiles = document.getElementById('connection_profiles');
    /** @type {HTMLElement} */
    const viewDetails = document.getElementById('view_connection_profile');
    const detailsContent = document.getElementById('connection_profile_details_content');
    const reloadButton = document.getElementById('reload_connection_profile');
    const updateButton = document.getElementById('update_connection_profile');
    const saveAsButton = document.getElementById('save_as_connection_profile');
    const editButton = document.getElementById('edit_connection_profile');
    const deleteButton = document.getElementById('delete_connection_profile');

    // Explicit order: everything below may render, track or sync through `ui`.
    const sidebar = installSidebarSelect(profiles, { onReapply: () => reloadButton.click() });
    ui = {
        sidebar,
        syncBindToggle: installConnectionOwnership(() => itemOwnsConnection(getSelectedItem())),
        drift: installDriftTracker({
            isBusy: () => applyingCount > 0,
            onChange: (dirty) => {
                // "Reapply" sits in the row only while the settings differ from the selected
                // item (otherwise it lives in the menu); "save" stays and is highlighted then.
                reloadButton.classList.toggle('tt-hidden', !dirty);
                for (const button of [reloadButton, updateButton]) {
                    button.classList.toggle('tt-cm-dirty', dirty);
                }
                sidebar.setDirty(dirty);
            },
        }),
    };
    // The selected item's application wrote the live hints, in this session or an earlier one.
    const selectedAtStart = getSelectedItem();
    hintsWriter = selectedAtStart && decidedHints(selectedAtStart.item).length > 0 ? selectedAtStart.item : null;
    installCompactActions();

    function toggleProfileSpecificButtons() {
        const hasSelection = Boolean(getSelectedItem());
        for (const button of [updateButton, saveAsButton, editButton, reloadButton, deleteButton]) {
            button.classList.toggle('disabled', !hasSelection);
        }
    }

    /** Keeps save / save as / create in the row; the rest moves into the "⋯" menu. */
    function installCompactActions() {
        profiles.parentElement.classList.add('tt-sel-row');
        for (const button of [viewDetails, editButton, deleteButton, reloadButton]) {
            button.classList.add('tt-hidden');
        }
        const needsSelection = () => (getSelectedItem() ? null : t`No model selected`);
        deleteButton.after(createOverflowMenuButton({
            title: t`More`,
            items: () => [
                { label: t`Show/hide details`, icon: 'circle-info', onSelect: () => viewDetails.click() },
                { label: t`Rename`, icon: 'pencil', disabledReason: needsSelection, onSelect: () => editButton.click() },
                { label: t`Reapply`, icon: 'recycle', disabledReason: needsSelection, onSelect: () => reloadButton.click() },
                { label: t`Delete`, icon: 'trash-can', danger: true, separatorBefore: true, disabledReason: needsSelection, onSelect: () => deleteButton.click() },
            ],
        }));
    }

    /** Brings everything that depends on the selection or its item in line with them. */
    async function refreshSelectionUi() {
        renderConnectionProfiles(profiles);
        toggleProfileSpecificButtons();
        requireUi().syncBindToggle();
        await renderDetailsContent(detailsContent);
    }

    await refreshSelectionUi();
    // Settings loaded from disk are taken as matching the selected item, once every
    // extension registered the slash commands an item may record.
    eventSource.on(event_types.APP_READY, () => trackSelectedItem());

    /**
     * Applies an item and announces it. Model targets validate before changing
     * anything, so a failure leaves the previous connection intact.
     * @param {{kind:string, item:ConnectionProfile|LlmModelTarget}} selected
     */
    async function applySelectedItem(selected) {
        applyingCount++;
        try {
            if (selected.kind === CONNECTION_ITEM_KIND.PROFILE) {
                await applyConnectionProfile(selected.item);
            } else {
                await applyModelTarget(selected.item);
            }
        } finally {
            applyingCount--;
        }
        trackSelectedItem();
        const loaded = selected.kind === CONNECTION_ITEM_KIND.PROFILE ? event_types.CONNECTION_PROFILE_LOADED : event_types.MODEL_TARGET_LOADED;
        await eventSource.emit(loaded, selected.item.name);
    }

    /**
     * Selects an item and applies it; the selection snaps back when it cannot apply.
     * @param {ConnectionManagerItemRef|null} ref Item to select, or null for none
     * @returns {Promise<boolean>} Whether the selection took effect
     */
    async function changeSelection(ref) {
        const previousRef = getSelectedItemRef();
        setSelectedItemRef(ref);
        saveSettingsDebounced();
        await refreshSelectionUi();

        if (!ref) {
            // None keeps the live connection as it is, request hints included; the next
            // item entered restores what the previous one wrote.
            trackSelectedItem();
            await eventSource.emit(event_types.CONNECTION_PROFILE_LOADED, NONE);
            return true;
        }

        const selected = resolveItemRef(ref);
        if (!selected) {
            console.warn(`Connection Manager item not found: ${ref.kind}:${ref.id}`);
            return false;
        }

        if (await runUiAction(() => applySelectedItem(selected))) {
            return true;
        }
        setSelectedItemRef(previousRef);
        saveSettingsDebounced();
        await refreshSelectionUi();
        return false;
    }

    profiles.addEventListener('change', () => {
        pendingSelection = changeSelection(parseItemOptionValue(profiles.value));
    });

    reloadButton.addEventListener('click', () => runUiAction(async () => {
        const selected = getSelectedItem();
        if (!selected) {
            return;
        }
        await applySelectedItem(selected);
        await renderDetailsContent(detailsContent);
        toastr.success(t`Reapplied`, '', { timeOut: 1500 });
    }));

    /**
     * Stores a newly captured item and selects it.
     * @param {{kind:string, item:ConnectionProfile|LlmModelTarget}} created
     */
    async function addCreatedItem(created) {
        if (decidedHints(created.item).length > 0) {
            switchRequestHints(created.item);
        }
        const isProfile = created.kind === CONNECTION_ITEM_KIND.PROFILE;
        (isProfile ? extension_settings.connectionManager.profiles : extension_settings.connectionManager.modelTargets).push(created.item);
        setSelectedItemRef({ kind: created.kind, id: created.item.id });
        saveSettingsDebounced();
        // The new item was captured from the live settings, so they match it now.
        trackSelectedItem();
        await eventSource.emit(isProfile ? event_types.CONNECTION_PROFILE_CREATED : event_types.MODEL_TARGET_CREATED, created.item);
        await eventSource.emit(isProfile ? event_types.CONNECTION_PROFILE_LOADED : event_types.MODEL_TARGET_LOADED, created.item.name);
        await refreshSelectionUi();
    }

    const createButton = document.getElementById('create_connection_profile');
    createButton.addEventListener('click', () => runUiAction(async () => {
        const created = await createConnectionItem();
        if (created) {
            await addCreatedItem(created);
        }
    }));

    saveAsButton.addEventListener('click', () => runUiAction(async () => {
        const selected = getSelectedItem();
        const created = selected ? await saveItemAs(selected) : null;
        if (created) {
            await addCreatedItem(created);
        }
    }));

    updateButton.addEventListener('click', () => runUiAction(async () => {
        const selected = getSelectedItem();
        if (!selected) {
            return;
        }
        const confirmed = await Popup.show.confirm(t`Overwrite with the current settings?`, selected.item.name);
        if (!confirmed) {
            return;
        }
        await overwriteItem(selected);
        saveSettingsDebounced();
        await refreshSelectionUi();
        toastr.success(t`Updated`, '', { timeOut: 1500 });
    }));

    deleteButton.addEventListener('click', () => runUiAction(async () => {
        const selected = getSelectedItem();
        const isTarget = selected?.kind === CONNECTION_ITEM_KIND.MODEL_TARGET;
        const deleted = isTarget ? await deleteModelTarget() : await deleteConnectionProfile();
        if (!deleted) {
            return;
        }
        // Nothing is selected now; as with choosing none, the live connection stays.
        trackSelectedItem();
        await eventSource.emit(isTarget ? event_types.MODEL_TARGET_LOADED : event_types.CONNECTION_PROFILE_LOADED, NONE);
        await refreshSelectionUi();
    }));

    editButton.addEventListener('click', () => runUiAction(async () => {
        const selected = getSelectedItem();
        if (!selected) {
            return;
        }
        const edited = selected.kind === CONNECTION_ITEM_KIND.PROFILE && !isModelAndPresetProfile(selected.item)
            ? await editLegacyProfile(selected.item)
            : await renameItem(selected);
        if (!edited) {
            return;
        }
        saveSettingsDebounced();
        await refreshSelectionUi();
    }));

    viewDetails.addEventListener('click', async () => {
        viewDetails.classList.toggle('active');
        detailsContent.classList.toggle('hidden');
        await renderDetailsContent(detailsContent);
    });

    SlashCommandParser.addCommandObject(SlashCommand.fromProps({
        name: 'profile',
        helpString: 'Switch to a connection profile or return the name of the current profile in no argument is provided. Use <code>&lt;None&gt;</code> to switch to no profile.',
        returns: 'name of the profile',
        unnamedArgumentList: [
            SlashCommandArgument.fromProps({
                description: 'Name of the connection profile',
                enumProvider: profilesProvider,
                isRequired: false,
            }),
        ],
        namedArgumentList: [
            SlashCommandNamedArgument.fromProps({
                name: 'await',
                description: 'Wait for the connection profile to be applied before returning.',
                isRequired: false,
                typeList: [ARGUMENT_TYPE.BOOLEAN],
                defaultValue: 'true',
                enumList: commonEnumProviders.boolean('trueFalse')(),
            }),
            SlashCommandNamedArgument.fromProps({
                name: 'timeout',
                description: 'Maximum time to wait for the API connection to be established, in milliseconds. Set to 0 to disable. Only applies when await=true.',
                isRequired: false,
                typeList: [ARGUMENT_TYPE.NUMBER],
                defaultValue: '2000',
            }),
        ],
        callback: async (args, value) => {
            if (!value || typeof value !== 'string') {
                // Names are unique across profiles and models, so `/profile <name>` restores either.
                return getSelectedItem()?.item.name ?? NONE;
            }

            if (value === NONE) {
                profiles.selectedIndex = 0;
                profiles.dispatchEvent(new Event('change'));
                return NONE;
            }

            const match = findItemByName(value);

            if (!match) {
                return '';
            }

            const shouldAwait = !isFalseBoolean(String(args?.await));

            profiles.value = makeItemOptionValue(match.kind, match.item.id);
            profiles.dispatchEvent(new Event('change'));

            if (shouldAwait) {
                if (!await pendingSelection) {
                    return '';
                }

                // We should also await the connection to be established
                const parsedTimeout = parseInt(args?.timeout?.toString());
                const timeout = !isNaN(parsedTimeout) ? Math.max(0, parsedTimeout) : 2000;
                if (timeout > 0) {
                    await waitUntilCondition(() => online_status !== 'no_connection', timeout, 100, { rejectOnTimeout: false });
                }
            }

            return match.item.name;
        },
    }));

    SlashCommandParser.addCommandObject(SlashCommand.fromProps({
        name: 'profile-list',
        helpString: 'List all connection profile names.',
        returns: 'list of profile names',
        callback: () => JSON.stringify(extension_settings.connectionManager.profiles.map(p => p.name)),
    }));

    SlashCommandParser.addCommandObject(SlashCommand.fromProps({
        name: 'profile-create',
        returns: 'name of the new profile',
        helpString: 'Create a new connection profile using the current settings.',
        unnamedArgumentList: [
            SlashCommandArgument.fromProps({
                description: 'name of the new connection profile',
                isRequired: true,
                typeList: [ARGUMENT_TYPE.STRING],
            }),
        ],
        callback: async (_args, name) => {
            if (!name || typeof name !== 'string') {
                toastr.warning(t`Please provide a name for the new connection profile.`);
                return '';
            }
            const profile = await createConnectionProfile(name);
            if (!profile) {
                return '';
            }
            extension_settings.connectionManager.profiles.push(profile);
            setSelectedItemRef({ kind: CONNECTION_ITEM_KIND.PROFILE, id: profile.id });
            saveSettingsDebounced();
            // Captured from the live settings, so they match it now.
            trackSelectedItem();
            await refreshSelectionUi();
            await eventSource.emit(event_types.CONNECTION_PROFILE_CREATED, profile);
            return profile.name;
        },
    }));

    SlashCommandParser.addCommandObject(SlashCommand.fromProps({
        name: 'profile-update',
        helpString: 'Update the selected connection profile.',
        callback: async () => {
            const selectedProfile = extension_settings.connectionManager.selectedProfile;
            const profile = extension_settings.connectionManager.profiles.find(p => p.id === selectedProfile);
            if (!profile) {
                toastr.warning(t`No profile selected`);
                return '';
            }
            const oldProfile = structuredClone(profile);
            await updateConnectionProfile(profile);
            // The profile describes the live settings now, and may have gained or lost a preset or the API.
            trackSelectedItem();
            await refreshSelectionUi();
            saveSettingsDebounced();
            await eventSource.emit(event_types.CONNECTION_PROFILE_UPDATED, oldProfile, profile);
            return profile.name;
        },
    }));

    SlashCommandParser.addCommandObject(SlashCommand.fromProps({
        name: 'profile-get',
        helpString: 'Get the details of the connection profile. Returns the selected profile if no argument is provided.',
        returns: 'object of the selected profile',
        unnamedArgumentList: [
            SlashCommandArgument.fromProps({
                description: 'Name of the connection profile',
                enumProvider: profilesProvider,
                isRequired: false,
            }),
        ],
        callback: async (_args, value) => {
            const selected = value && typeof value === 'string' ? findItemByName(value) : getSelectedItem();
            return selected ? JSON.stringify(itemAsProfile(selected)) : '';
        },
    }));

    SlashCommandParser.addCommandObject(SlashCommand.fromProps({
        name: 'profile-genstream',
        callback: generateStreamCallback,
        returns: t`generated text`,
        namedArgumentList: [
            new SlashCommandNamedArgument(
                'lock', t`lock user input during generation`, [ARGUMENT_TYPE.BOOLEAN], false, false, 'off', commonEnumProviders.boolean('onOff')(),
            ),
            SlashCommandNamedArgument.fromProps({
                name: 'profile',
                description: t`connection profile ID to use for generation`,
                typeList: [ARGUMENT_TYPE.STRING],
                enumProvider: commonEnumProviders.connectionProfiles(),
            }),
            SlashCommandNamedArgument.fromProps({
                name: 'reasoning',
                description: t`include formatted reasoning in the output`,
                typeList: [ARGUMENT_TYPE.BOOLEAN],
                defaultValue: 'false',
                enumProvider: commonEnumProviders.boolean('trueFalse'),
            }),
            SlashCommandNamedArgument.fromProps({
                name: 'system',
                description: t`system prompt at the start`,
                typeList: [ARGUMENT_TYPE.STRING],
            }),
            SlashCommandNamedArgument.fromProps({
                name: 'length',
                description: t`API response length in tokens`,
                typeList: [ARGUMENT_TYPE.NUMBER],
                defaultValue: '2048',
            }),
            SlashCommandNamedArgument.fromProps({
                name: 'generating',
                description: t`label/title for the generation display`,
                typeList: [ARGUMENT_TYPE.STRING],
                defaultValue: 'Generating...',
            }),
            SlashCommandNamedArgument.fromProps({
                name: 'completed',
                description: t`updated label/title for when generation completes`,
                typeList: [ARGUMENT_TYPE.STRING],
                defaultValue: 'Generated',
            }),
            SlashCommandNamedArgument.fromProps({
                name: 'delay',
                description: t`auto-hide delay in ms after generation completes. Use "infinite" or negative to keep until manually closed`,
                typeList: [ARGUMENT_TYPE.NUMBER],
                defaultValue: '3000',
                enumList: [
                    new SlashCommandEnumValue('infinite', 'Keep the streaming display open until manually closed', 'command', 'infinity'),
                    new SlashCommandEnumValue('any delay in seconds', null, 'number', 'time', () => true, input => input),
                ],
            }),
            SlashCommandNamedArgument.fromProps({
                name: 'stop',
                description: t`show a stop button on the streaming display that aborts generation when clicked`,
                typeList: [ARGUMENT_TYPE.BOOLEAN],
                defaultValue: 'true',
                enumProvider: commonEnumProviders.boolean('trueFalse'),
            }),
            SlashCommandNamedArgument.fromProps({
                name: 'onStop',
                description: t`closure to execute when the stop button is clicked (in addition to aborting the request)`,
                typeList: [ARGUMENT_TYPE.CLOSURE],
            }),
            SlashCommandNamedArgument.fromProps({
                name: 'onComplete',
                description: t`closure to execute after generation completes successfully`,
                typeList: [ARGUMENT_TYPE.CLOSURE],
            }),
        ],
        unnamedArgumentList: [
            SlashCommandArgument.fromProps({
                description: 'prompt',
                typeList: [ARGUMENT_TYPE.STRING],
                isRequired: true,
            }),
        ],
        helpString: `
            <div>
                ${t`Generates text using Connection Manager with streaming display. Shows live generation progress including reasoning (thinking) and content.`}
            </div>
            <div>
                ${t`Requires Connection Manager extension. Uses the currently selected profile or the specified profile= argument.`}
            </div>
            <div>
                ${t`Use reasoning=true to include formatted reasoning in the output (using the defined reasoning template). This can be parsed later with /reasoning-parse.`}
            </div>
            <div>
                ${t`Use delay to control auto-hide behavior: number (ms), "infinite", or negative to keep the display open until manually closed. The display shows a green LED when complete.`}
            </div>
            <div>
                ${t`A stop button is shown by default (stop=true). Click it to abort generation and return whatever was streamed so far. Use stop=false to hide the stop button.`}
            </div>
            <div>
                ${t`Use onStop and onComplete closures for custom behavior when generation is stopped or completes.`}
            </div>
            <div>
                ${t`Example: <pre><code>/profile-genstream profile=my-profile-id reasoning=true Summarize the following text</code></pre>`}
            </div>
            <div>
                ${t`Example with infinite display: <pre><code>/profile-genstream delay=infinite Tell me a story</code></pre>`}
            </div>
            <div>
                ${t`Example with custom stop handler: <pre><code>/profile-genstream onStop={: /echo "Generation stopped!" :} Tell me a story</code></pre>`}
            </div>
        `,
    }));
}
