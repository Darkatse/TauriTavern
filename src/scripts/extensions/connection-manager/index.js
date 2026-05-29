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
import { getSecretLabelById, resolveSecretKey } from '../../secrets.js';
import { connectCurrentApi } from '../../slash-commands.js';
import { performFuzzySearch } from '/scripts/power-user.js';
import { StreamingDisplay } from '/scripts/streaming-display.js';
import { ConnectionManagerRequestService } from '../shared.js';
import { formatReasoning } from '/scripts/reasoning.js';

const MODULE_NAME = 'connection-manager';
const NONE = '<None>';
const EMPTY = '<Empty>';
const NO_PROXY_PRESET = 'None';
const MODEL_TARGET_KIND = 'tauritavern.modelTarget';
const MODEL_TARGET_SCHEMA_VERSION = 1;
const CONNECTION_ITEM_KIND = {
    PROFILE: 'profile',
    MODEL_TARGET: 'modelTarget',
};
const CREATE_MODEL_TARGET_RESULT = POPUP_RESULT.CUSTOM1;

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

    /** @type {HTMLElement} */
    spinnerElement;

    /** @type {AbortController} */
    abortController = new AbortController();

    constructor() {
        // @ts-ignore
        this.spinnerElement = document.getElementById('connection_profile_spinner');
        this.abortController = new AbortController();
    }

    start() {
        ConnectionManagerSpinner.abortControllers.push(this.abortController);
        this.spinnerElement.classList.remove('hidden');
    }

    stop() {
        this.spinnerElement.classList.add('hidden');
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
    if (!ref) {
        extension_settings.connectionManager.selectedProfile = null;
    } else if (ref.kind === CONNECTION_ITEM_KIND.PROFILE) {
        extension_settings.connectionManager.selectedProfile = ref.id;
    }
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
 * Migrates legacy selection state without changing profile data.
 */
function normalizeConnectionManagerSettings() {
    const settings = extension_settings.connectionManager;
    if (!settings.selectedItem && settings.selectedProfile) {
        settings.selectedItem = { kind: CONNECTION_ITEM_KIND.PROFILE, id: settings.selectedProfile };
    }
    if (settings.selectedItem?.kind === CONNECTION_ITEM_KIND.PROFILE) {
        settings.selectedProfile = settings.selectedItem.id;
    }
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
 * @returns {Promise<LlmModelTarget|null>}
 */
async function createModelTarget(name) {
    if (extension_settings.connectionManager.modelTargets.some(t => t.name === name) || name === NONE) {
        toastr.error(t`A model with the same name already exists.`);
        return null;
    }

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
    const isNameTaken = (n) => extension_settings.connectionManager.profiles.some(p => p.name === n);
    const suggestedName = getUniqueName(collapseSpaces(`${profile.api ?? ''} ${profile.model ?? ''} - ${profile.preset ?? ''}`), isNameTaken);
    const name = normalizeItemName(forceName ?? await callGenericPopup(template, POPUP_TYPE.INPUT, suggestedName));
    if (!name) {
        return null;
    }

    if (isNameTaken(name) || name === NONE) {
        toastr.error(t`A profile with the same name already exists.`);
        return null;
    }

    removeExcludedProfileFields(profile);
    profile.name = String(name);
    return profile;
}

/**
 * Creates a connection-manager item from the current settings.
 * @returns {Promise<{kind:string, item:ConnectionProfile|LlmModelTarget}|null>}
 */
async function createConnectionItem() {
    const profile = await createConnectionProfileSnapshot();
    const profileForDisplay = makeFancyProfile(profile);
    const template = $(await renderExtensionTemplateAsync(MODULE_NAME, 'profile', { profile: profileForDisplay }));
    bindProfileExcludeToggles(template, profile);

    const suggestedName = getUniqueName(
        collapseSpaces(`${profile.api ?? ''} ${profile.model ?? ''} - ${profile.preset ?? ''}`),
        (n) => extension_settings.connectionManager.profiles.some(p => p.name === n),
    );
    const popup = new Popup(template, POPUP_TYPE.INPUT, suggestedName, {
        customButtons: [{
            text: t`Save Model Only`,
            result: CREATE_MODEL_TARGET_RESULT,
            classes: ['popup-button-ok'],
            tooltip: t`Save only API, server URL, model, proxy, and secret.`,
        }],
    });
    const name = normalizeItemName(await popup.show());

    if (!name) {
        return null;
    }

    const createKind = popup.result === CREATE_MODEL_TARGET_RESULT
        ? CONNECTION_ITEM_KIND.MODEL_TARGET
        : CONNECTION_ITEM_KIND.PROFILE;
    if (createKind === CONNECTION_ITEM_KIND.MODEL_TARGET) {
        const item = await createModelTarget(name);
        return item ? { kind: CONNECTION_ITEM_KIND.MODEL_TARGET, item } : null;
    }

    if (extension_settings.connectionManager.profiles.some(p => p.name === name) || name === NONE) {
        toastr.error(t`A profile with the same name already exists.`);
        return null;
    }

    removeExcludedProfileFields(profile);
    profile.name = String(name);
    return { kind: CONNECTION_ITEM_KIND.PROFILE, item: profile };
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
    const confirm = await Popup.show.confirm(t`Are you sure you want to delete the selected model?`, target.name);

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

    result['Saved Object'] = t`Model`;
    for (const field of fields) {
        if (!target[field]) {
            continue;
        }
        result[FANCY_NAMES[field]] = target[field];
    }

    if (target.secretRef?.id) {
        result[FANCY_NAMES['secret-id']] = target.secretRef.labelSnapshot || getSecretLabelById(target.secretRef.id) || target.secretRef.id;
    }

    return result;
}

/**
 * Asserts that a model target can be applied.
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
            throw new Error('Model target application aborted');
        }

        const spinner = new ConnectionManagerSpinner();
        spinner.start();

        try {
            await withConnectionValidationSuspended('Model target application', async () => {
                await requireManagedCommand('api', target.api);

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
            throw new Error('Profile application aborted');
        }

        const mode = profile.mode;
        const commands = mode === 'cc' ? CC_COMMANDS : TC_COMMANDS;
        const spinner = new ConnectionManagerSpinner();
        spinner.start();

        try {
            await withConnectionValidationSuspended('Connection profile application', async () => {
                for (const command of commands) {
                    if (spinner.isAborted() || applicationVersion !== profileApplicationVersion) {
                        throw new Error('Profile application aborted');
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
}

/**
 * Updates a model target from the current settings.
 * @param {LlmModelTarget} target Model target
 * @returns {Promise<void>}
 */
async function updateModelTarget(target) {
    await readModelTargetFromCommands(target);
}

/**
 * Edits a model target name and optionally refreshes its captured model route.
 * @param {LlmModelTarget} target Model target
 * @returns {Promise<LlmModelTarget|null>}
 */
async function editModelTarget(target) {
    const template = $(await renderExtensionTemplateAsync(MODULE_NAME, 'view', { profile: makeFancyModelTarget(target) }));
    const nameHeading = $('<h3 data-i18n="Model name:"></h3>');
    nameHeading.text(t`Model name:`);
    template.append(nameHeading);
    const popup = new Popup(template, POPUP_TYPE.INPUT, target.name, {
        customButtons: [{
            text: t`Save and Update`,
            classes: ['popup-button-ok'],
            result: POPUP_RESULT.CUSTOM1,
            tooltip: t`Rename and refresh the saved model route from the current connection settings.`,
        }],
    });

    let newName = await popup.show();
    newName = normalizeItemName(newName);
    if (!newName) {
        return null;
    }

    if (target.name !== newName && extension_settings.connectionManager.modelTargets.some(t => t.name === newName)) {
        toastr.error(t`A model with the same name already exists.`);
        return null;
    }

    const oldTarget = structuredClone(target);
    if (popup.result === POPUP_RESULT.CUSTOM1) {
        await updateModelTarget(target);
    }
    if (target.name !== newName) {
        target.name = newName;
        toastr.success(t`Model renamed.`);
    }

    return oldTarget;
}

/**
 * Renders the connection profile details.
 * @param {HTMLSelectElement} profiles Select element containing connection profiles
 */
function renderConnectionProfiles(profiles) {
    profiles.innerHTML = '';
    const noneOption = document.createElement('option');
    const selectedValue = getSelectedOptionValue();

    noneOption.value = '';
    noneOption.textContent = NONE;
    noneOption.selected = !selectedValue;
    profiles.appendChild(noneOption);

    const profileGroup = document.createElement('optgroup');
    profileGroup.label = t`Connection Profiles`;
    for (const profile of extension_settings.connectionManager.profiles.slice().sort((a, b) => a.name.localeCompare(b.name))) {
        const value = makeItemOptionValue(CONNECTION_ITEM_KIND.PROFILE, profile.id);
        const option = document.createElement('option');
        option.value = value;
        option.textContent = profile.name;
        option.selected = value === selectedValue;
        profileGroup.appendChild(option);
    }
    if (profileGroup.children.length > 0) {
        profiles.appendChild(profileGroup);
    }

    const modelTargetGroup = document.createElement('optgroup');
    modelTargetGroup.label = t`Models`;
    for (const target of extension_settings.connectionManager.modelTargets.slice().sort((a, b) => a.name.localeCompare(b.name))) {
        const value = makeItemOptionValue(CONNECTION_ITEM_KIND.MODEL_TARGET, target.id);
        const option = document.createElement('option');
        option.value = value;
        option.textContent = target.name;
        option.selected = value === selectedValue;
        modelTargetGroup.appendChild(option);
    }
    if (modelTargetGroup.children.length > 0) {
        profiles.appendChild(modelTargetGroup);
    }
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
        if (Array.isArray(selected.item.exclude) && selected.item.exclude.length > 0) {
            templateParams.omitted = selected.item.exclude.map(e => FANCY_NAMES[e]).join(', ');
        }
        const template = await renderExtensionTemplateAsync(MODULE_NAME, 'view', templateParams);
        detailsContent.innerHTML = template;
    } else if (selected?.kind === CONNECTION_ITEM_KIND.MODEL_TARGET) {
        const template = await renderExtensionTemplateAsync(MODULE_NAME, 'view', { profile: makeFancyModelTarget(selected.item) });
        detailsContent.innerHTML = template;
    } else {
        detailsContent.textContent = t`No profile selected`;
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

        let effectiveProfileId = context.extensionSettings.connectionManager.selectedProfile;
        const profiles = context.extensionSettings.connectionManager.profiles;

        if (profileIdOrName) {
            const profile = profiles.find(p => p.id === profileIdOrName);
            if (profile) {
                effectiveProfileId = profile.id;
            } else {
                const fuseResults = performFuzzySearch('profile', profiles, [{ name: 'name', weight: 10 }], profileIdOrName);
                if (fuseResults.length > 0) {
                    effectiveProfileId = fuseResults[0].item.id;
                } else {
                    toastr.warning(t`Connection profile not found: ${profileIdOrName}`);
                    return '';
                }
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
    renderConnectionProfiles(profiles);

    function toggleProfileSpecificButtons() {
        const hasSelection = Boolean(getSelectedItem());
        const profileSpecificButtons = ['update_connection_profile', 'edit_connection_profile', 'reload_connection_profile', 'delete_connection_profile'];
        profileSpecificButtons.forEach(id => document.getElementById(id).classList.toggle('disabled', !hasSelection));
    }
    toggleProfileSpecificButtons();

    profiles.addEventListener('change', async function () {
        const selectedOption = profiles.selectedOptions[0];
        if (!selectedOption) {
            // Safety net for preventing the command getting stuck
            await eventSource.emit(event_types.CONNECTION_PROFILE_LOADED, NONE);
            return;
        }

        const selectedRef = parseItemOptionValue(selectedOption.value);
        setSelectedItemRef(selectedRef);
        saveSettingsDebounced();
        await renderDetailsContent(detailsContent);

        toggleProfileSpecificButtons();

        // None option selected
        if (!selectedRef) {
            await eventSource.emit(event_types.CONNECTION_PROFILE_LOADED, NONE);
            return;
        }

        const selected = resolveItemRef(selectedRef);
        if (!selected) {
            console.log(`Connection Manager item not found: ${selectedRef.kind}:${selectedRef.id}`);
            return;
        }

        if (selected.kind === CONNECTION_ITEM_KIND.PROFILE) {
            await applyConnectionProfile(selected.item);
            await eventSource.emit(event_types.CONNECTION_PROFILE_LOADED, selected.item.name);
        } else if (selected.kind === CONNECTION_ITEM_KIND.MODEL_TARGET) {
            await applyModelTarget(selected.item);
            await eventSource.emit(event_types.MODEL_TARGET_LOADED, selected.item.name);
        }
    });

    const reloadButton = document.getElementById('reload_connection_profile');
    reloadButton.addEventListener('click', async () => {
        const selected = getSelectedItem();
        if (!selected) {
            console.log('No profile selected');
            return;
        }
        if (selected.kind === CONNECTION_ITEM_KIND.PROFILE) {
            await applyConnectionProfile(selected.item);
            await eventSource.emit(event_types.CONNECTION_PROFILE_LOADED, selected.item.name);
            toastr.success(t`Connection profile reloaded`, '', { timeOut: 1500 });
        } else if (selected.kind === CONNECTION_ITEM_KIND.MODEL_TARGET) {
            await applyModelTarget(selected.item);
            await eventSource.emit(event_types.MODEL_TARGET_LOADED, selected.item.name);
            toastr.success(t`Model reloaded`, '', { timeOut: 1500 });
        }
        await renderDetailsContent(detailsContent);
    });

    const createButton = document.getElementById('create_connection_profile');
    createButton.addEventListener('click', async () => {
        const created = await createConnectionItem();
        if (!created) {
            return;
        }

        if (created.kind === CONNECTION_ITEM_KIND.PROFILE) {
            extension_settings.connectionManager.profiles.push(created.item);
            setSelectedItemRef({ kind: CONNECTION_ITEM_KIND.PROFILE, id: created.item.id });
            await eventSource.emit(event_types.CONNECTION_PROFILE_CREATED, created.item);
            await eventSource.emit(event_types.CONNECTION_PROFILE_LOADED, created.item.name);
        } else if (created.kind === CONNECTION_ITEM_KIND.MODEL_TARGET) {
            extension_settings.connectionManager.modelTargets.push(created.item);
            setSelectedItemRef({ kind: CONNECTION_ITEM_KIND.MODEL_TARGET, id: created.item.id });
            await eventSource.emit(event_types.MODEL_TARGET_CREATED, created.item);
            await eventSource.emit(event_types.MODEL_TARGET_LOADED, created.item.name);
        }

        saveSettingsDebounced();
        renderConnectionProfiles(profiles);
        await renderDetailsContent(detailsContent);
        toggleProfileSpecificButtons();
    });

    const updateButton = document.getElementById('update_connection_profile');
    updateButton.addEventListener('click', async () => {
        const selected = getSelectedItem();
        if (!selected) {
            console.log('No profile selected');
            return;
        }
        const oldItem = structuredClone(selected.item);
        if (selected.kind === CONNECTION_ITEM_KIND.PROFILE) {
            await updateConnectionProfile(selected.item);
            await eventSource.emit(event_types.CONNECTION_PROFILE_UPDATED, oldItem, selected.item);
            await eventSource.emit(event_types.CONNECTION_PROFILE_LOADED, selected.item.name);
            toastr.success(t`Connection profile updated`, '', { timeOut: 1500 });
        } else if (selected.kind === CONNECTION_ITEM_KIND.MODEL_TARGET) {
            await updateModelTarget(selected.item);
            await eventSource.emit(event_types.MODEL_TARGET_UPDATED, oldItem, selected.item);
            await eventSource.emit(event_types.MODEL_TARGET_LOADED, selected.item.name);
            toastr.success(t`Model updated`, '', { timeOut: 1500 });
        }
        await renderDetailsContent(detailsContent);
        saveSettingsDebounced();
    });

    const deleteButton = document.getElementById('delete_connection_profile');
    deleteButton.addEventListener('click', async () => {
        const selected = getSelectedItem();
        let deleted = false;
        if (selected?.kind === CONNECTION_ITEM_KIND.MODEL_TARGET) {
            deleted = await deleteModelTarget();
            if (deleted) {
                await eventSource.emit(event_types.MODEL_TARGET_LOADED, NONE);
            }
        } else {
            deleted = await deleteConnectionProfile();
            if (deleted) {
                await eventSource.emit(event_types.CONNECTION_PROFILE_LOADED, NONE);
            }
        }
        if (!deleted) {
            return;
        }
        renderConnectionProfiles(profiles);
        await renderDetailsContent(detailsContent);
        toggleProfileSpecificButtons();
    });

    const editButton = document.getElementById('edit_connection_profile');
    editButton.addEventListener('click', async () => {
        const selected = getSelectedItem();
        if (!selected) {
            console.log('No profile selected');
            return;
        }
        if (selected.kind === CONNECTION_ITEM_KIND.MODEL_TARGET) {
            const oldTarget = await editModelTarget(selected.item);
            if (!oldTarget) {
                return;
            }
            saveSettingsDebounced();
            await eventSource.emit(event_types.MODEL_TARGET_UPDATED, oldTarget, selected.item);
            renderConnectionProfiles(profiles);
            await renderDetailsContent(detailsContent);
            return;
        }

        const profile = selected.item;
        if (!Array.isArray(profile.exclude)) {
            profile.exclude = [];
        }

        const sortByViewOrder = (a, b) => Object.keys(FANCY_NAMES).indexOf(a) - Object.keys(FANCY_NAMES).indexOf(b);
        const commands = profile.mode === 'cc' ? CC_COMMANDS : TC_COMMANDS;
        const settings = commands.slice().sort(sortByViewOrder).reduce((acc, command) => {
            const fancyName = FANCY_NAMES[command];
            acc[fancyName] = !profile.exclude.includes(command);
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

        let newName = await popup.show();
        if (!newName) {
            return;
        }
        newName = DOMPurify.sanitize(String(newName));
        if (!newName) {
            toastr.error(t`Name cannot be empty.`);
            return;
        }

        if (profile.name !== newName && extension_settings.connectionManager.profiles.some(p => p.name === newName)) {
            toastr.error(t`A profile with the same name already exists.`);
            return;
        }

        const newExcludeList = template.find('input[name="exclude"]:not(:checked)').map(function () {
            return Object.entries(FANCY_NAMES).find(x => x[1] === String($(this).val()))?.[0];
        }).get();

        const oldProfile = structuredClone(profile);
        if (newExcludeList.length !== profile.exclude.length || !newExcludeList.every(e => profile.exclude.includes(e))) {
            profile.exclude = newExcludeList;
            for (const command of newExcludeList) {
                delete profile[command];
            }
            if (popup.result === POPUP_RESULT.CUSTOM1) {
                await updateConnectionProfile(profile);
            } else {
                toastr.info(t`Press "Update" to record them into the profile.`, t`Included settings list updated`);
            }
        }

        if (profile.name !== newName) {
            toastr.success(t`Connection profile renamed.`);
            profile.name = newName;
        }

        saveSettingsDebounced();
        await eventSource.emit(event_types.CONNECTION_PROFILE_UPDATED, oldProfile, profile);
        renderConnectionProfiles(profiles);
        await renderDetailsContent(detailsContent);
    });

    /** @type {HTMLElement} */
    const viewDetails = document.getElementById('view_connection_profile');
    const detailsContent = document.getElementById('connection_profile_details_content');
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
                const selectedProfile = extension_settings.connectionManager.selectedProfile;
                const profile = extension_settings.connectionManager.profiles.find(p => p.id === selectedProfile);
                if (!profile) {
                    return NONE;
                }
                return profile.name;
            }

            if (value === NONE) {
                profiles.selectedIndex = 0;
                profiles.dispatchEvent(new Event('change'));
                return NONE;
            }

            const profile = findProfileByName(value);

            if (!profile) {
                return '';
            }

            const shouldAwait = !isFalseBoolean(String(args?.await));
            const awaitPromise = new Promise((resolve) => eventSource.once(event_types.CONNECTION_PROFILE_LOADED, resolve));

            profiles.selectedIndex = Array.from(profiles.options).findIndex(o => o.value === makeItemOptionValue(CONNECTION_ITEM_KIND.PROFILE, profile.id));
            profiles.dispatchEvent(new Event('change'));

            if (shouldAwait) {
                await awaitPromise;

                // We should also await the connection to be established
                const parsedTimeout = parseInt(args?.timeout?.toString());
                const timeout = !isNaN(parsedTimeout) ? Math.max(0, parsedTimeout) : 2000;
                if (timeout > 0) {
                    await waitUntilCondition(() => online_status !== 'no_connection', timeout, 100, { rejectOnTimeout: false });
                }
            }

            return profile.name;
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
            renderConnectionProfiles(profiles);
            await renderDetailsContent(detailsContent);
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
            await renderDetailsContent(detailsContent);
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
            if (!value || typeof value !== 'string') {
                const selectedProfile = extension_settings.connectionManager.selectedProfile;
                const profile = extension_settings.connectionManager.profiles.find(p => p.id === selectedProfile);
                if (!profile) {
                    return '';
                }
                return JSON.stringify(profile);
            }

            const profile = findProfileByName(value);
            if (!profile) {
                return '';
            }
            return JSON.stringify(profile);
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
