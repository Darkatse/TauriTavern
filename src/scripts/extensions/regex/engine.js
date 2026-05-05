import { characters, saveSettingsDebounced, substituteParams, substituteParamsExtended, this_chid } from '../../../script.js';
import { extension_settings, writeExtensionField } from '../../extensions.js';
import { getPresetManager } from '../../preset-manager.js';
import { regexFromString } from '../../utils.js';
import { lodash } from '../../../lib.js';
import { applyNativeRegexBatch, isNativeRegexBackendAvailable } from '../../tauri/regex/native-regex-transform.js';
import { isNativeRegexBackendEnabled } from '../../tauri/regex/native-regex-settings.js';

/**
 * @readonly
 * @enum {number} Regex scripts types
 */
export const SCRIPT_TYPES = {
    // ORDER MATTERS: defines the regex script priority
    GLOBAL: 0,
    PRESET: 2,
    SCOPED: 1,
};

/**
 * Special type for unknown/invalid script types.
 */
export const SCRIPT_TYPE_UNKNOWN = -1;

/**
 * @typedef {import('../../char-data.js').RegexScriptData} RegexScript
 */

/**
 * @typedef {object} GetRegexScriptsOptions
 * @property {boolean} allowedOnly Only return allowed scripts
 */

/**
 * @type {Readonly<GetRegexScriptsOptions>}
 */
const DEFAULT_GET_REGEX_SCRIPTS_OPTIONS = Object.freeze({ allowedOnly: false });
const NATIVE_REGEX_SUPPORTED_FLAGS = new Set(['g', 'i', 'm', 's', 'u', 'v']);
const SUBSTITUTE_PARAM_TOKEN_REGEX = /{{|<(?:USER|BOT|CHAR|CHARIFNOTGROUP|GROUP)>/i;
const REPLACEMENT_CAPTURE_REF_REGEX = /\$(?:\d+|<[^>]+>)/;

/**
 * Manages the compiled regex cache with LRU eviction.
 */
export class RegexProvider {
    /** @type {Map<string, RegExp>} */
    #cache = new Map();
    /** @type {number} */
    #maxSize = 1000;

    static instance = new RegexProvider();

    /**
     * Gets a regex instance by its string representation.
     * @param {string} regexString The regex string to retrieve
     * @returns {RegExp?} Compiled regex or null if invalid
     */
    get(regexString) {
        const isCached = this.#cache.has(regexString);
        const regex = isCached
            ? this.#cache.get(regexString)
            : regexFromString(regexString);

        if (!regex) {
            return null;
        }

        if (isCached) {
            // LRU: Move to end by re-inserting
            this.#cache.delete(regexString);
            this.#cache.set(regexString, regex);
        } else {
            // Evict oldest if at capacity
            if (this.#cache.size >= this.#maxSize) {
                const firstKey = this.#cache.keys().next().value;
                this.#cache.delete(firstKey);
            }
            this.#cache.set(regexString, regex);
        }

        // Reset lastIndex for global/sticky regexes
        if (regex.global || regex.sticky) {
            regex.lastIndex = 0;
        }

        return regex;
    }

    /**
     * Clears the entire cache.
     */
    clear() {
        this.#cache.clear();
    }
}

/**
 * Retrieves the list of regex scripts by combining the scripts from the extension settings and the character data
 *
 * @param {GetRegexScriptsOptions} options Options for retrieving the regex scripts
 * @returns {RegexScript[]} An array of regex scripts, where each script is an object containing the necessary information.
 */
export function getRegexScripts(options = DEFAULT_GET_REGEX_SCRIPTS_OPTIONS) {
    return [...Object.values(SCRIPT_TYPES).flatMap(type => getScriptsByType(type, options))];
}

/**
 * Retrieves the regex scripts for a specific type.
 * @param {SCRIPT_TYPES} scriptType The type of regex scripts to retrieve.
 * @param {GetRegexScriptsOptions} options Options for retrieving the regex scripts
 * @returns {RegexScript[]} An array of regex scripts for the specified type.
 */
export function getScriptsByType(scriptType, { allowedOnly } = DEFAULT_GET_REGEX_SCRIPTS_OPTIONS) {
    switch (scriptType) {
        case SCRIPT_TYPE_UNKNOWN:
            return [];
        case SCRIPT_TYPES.GLOBAL:
            return extension_settings.regex ?? [];
        case SCRIPT_TYPES.SCOPED: {
            if (allowedOnly && !extension_settings?.character_allowed_regex?.includes(characters?.[this_chid]?.avatar)) {
                return [];
            }
            const scopedScripts = characters[this_chid]?.data?.extensions?.regex_scripts;
            return Array.isArray(scopedScripts) ? scopedScripts : [];
        }
        case SCRIPT_TYPES.PRESET: {
            if (allowedOnly && !extension_settings?.preset_allowed_regex?.[getCurrentPresetAPI()]?.includes(getCurrentPresetName())) {
                return [];
            }
            const presetManager = getPresetManager();
            const presetScripts = presetManager?.readPresetExtensionField({ path: 'regex_scripts' });
            return Array.isArray(presetScripts) ? presetScripts : [];
        }
        default:
            console.warn(`getScriptsByType: Invalid script type ${scriptType}`);
            return [];
    }
}

/**
 * Saves an array of regex scripts for a specific type.
 * @param {RegexScript[]} scripts An array of regex scripts to save.
 * @param {SCRIPT_TYPES} scriptType The type of regex scripts to save.
 * @returns {Promise<void>}
 */
export async function saveScriptsByType(scripts, scriptType) {
    switch (scriptType) {
        case SCRIPT_TYPES.GLOBAL:
            extension_settings.regex = scripts;
            saveSettingsDebounced();
            break;
        case SCRIPT_TYPES.SCOPED:
            await writeExtensionField(this_chid, 'regex_scripts', scripts);
            break;
        case SCRIPT_TYPES.PRESET: {
            const presetManager = getPresetManager();
            await presetManager.writePresetExtensionField({ path: 'regex_scripts', value: scripts });
            break;
        }
        default:
            console.warn(`saveScriptsByType: Invalid script type ${scriptType}`);
            break;
    }
}

/**
 * Check if character's regexes are allowed to be used; if character is undefined, returns false
 * @param {Character|undefined} character
 * @returns {boolean}
 */
export function isScopedScriptsAllowed(character) {
    return !!extension_settings?.character_allowed_regex?.includes(character?.avatar);
}

/**
 * Allow character's regexes to be used; if character is undefined, do nothing
 * @param {Character|undefined} character
 * @returns {void}
 */
export function allowScopedScripts(character) {
    const avatar = character?.avatar;
    if (!avatar) {
        return;
    }
    if (!Array.isArray(extension_settings?.character_allowed_regex)) {
        extension_settings.character_allowed_regex = [];
    }
    if (!extension_settings.character_allowed_regex.includes(avatar)) {
        extension_settings.character_allowed_regex.push(avatar);
        saveSettingsDebounced();
    }
}

/**
 * Disallow character's regexes to be used; if character is undefined, do nothing
 * @param {Character|undefined} character
 * @returns {void}
 */
export function disallowScopedScripts(character) {
    const avatar = character?.avatar;
    if (!avatar) {
        return;
    }
    if (!Array.isArray(extension_settings?.character_allowed_regex)) {
        return;
    }
    const index = extension_settings.character_allowed_regex.indexOf(avatar);
    if (index !== -1) {
        extension_settings.character_allowed_regex.splice(index, 1);
        saveSettingsDebounced();
    }
}

/**
 * Check if preset's regexes are allowed to be used
 * @param {string} apiId API ID
 * @param {string} presetName Preset name
 * @returns {boolean} True if allowed, false if not
 */
export function isPresetScriptsAllowed(apiId, presetName) {
    if (!apiId || !presetName) {
        return false;
    }
    return !!extension_settings?.preset_allowed_regex?.[apiId]?.includes(presetName);
}

/**
 * Allow preset's regexes to be used
 * @param {string} apiId API ID
 * @param {string} presetName Preset name
 * @returns {void}
 */
export function allowPresetScripts(apiId, presetName) {
    if (!apiId || !presetName) {
        return;
    }
    if (!Array.isArray(extension_settings?.preset_allowed_regex?.[apiId])) {
        lodash.set(extension_settings, ['preset_allowed_regex', apiId], []);
    }
    if (!extension_settings.preset_allowed_regex[apiId].includes(presetName)) {
        extension_settings.preset_allowed_regex[apiId].push(presetName);
        saveSettingsDebounced();
    }
}

/**
 * Disallow preset's regexes to be used
 * @param {string} apiId API ID
 * @param {string} presetName Preset name
 * @returns {void}
 */
export function disallowPresetScripts(apiId, presetName) {
    if (!apiId || !presetName) {
        return;
    }
    if (!Array.isArray(extension_settings?.preset_allowed_regex?.[apiId])) {
        return;
    }
    const index = extension_settings.preset_allowed_regex[apiId].indexOf(presetName);
    if (index !== -1) {
        extension_settings.preset_allowed_regex[apiId].splice(index, 1);
        saveSettingsDebounced();
    }
}

/**
 * Gets the current API ID from the preset manager.
 * @returns {string|null} Current API ID, or null if no preset manager
 */
export function getCurrentPresetAPI() {
    return getPresetManager()?.apiId ?? null;
}

/**
 * Gets the name of the currently selected preset.
 * @returns {string|null} The name of the currently selected preset, or null if no preset manager
 */
export function getCurrentPresetName() {
    return getPresetManager()?.getSelectedPresetName() ?? null;
}

/**
 * @readonly
 * @enum {number} Where the regex script should be applied
 */
export const regex_placement = {
    /**
     * @deprecated MD Display is deprecated. Do not use.
     */
    MD_DISPLAY: 0,
    USER_INPUT: 1,
    AI_OUTPUT: 2,
    SLASH_COMMAND: 3,
    // 4 - sendAs (legacy)
    WORLD_INFO: 5,
    REASONING: 6,
};

/**
 * @readonly
 * @enum {number} How to substitute parameters in the find regex
 */
export const substitute_find_regex = {
    NONE: 0,
    RAW: 1,
    ESCAPED: 2,
};

function sanitizeRegexMacro(x) {
    return (x && typeof x === 'string') ?
        x.replaceAll(/[\n\r\t\v\f\0.^$*+?{}[\]\\/|()]/gs, function (s) {
            switch (s) {
                case '\n':
                    return '\\n';
                case '\r':
                    return '\\r';
                case '\t':
                    return '\\t';
                case '\v':
                    return '\\v';
                case '\f':
                    return '\\f';
                case '\0':
                    return '\\0';
                default:
                    return '\\' + s;
            }
        }) : x;
}

function resolveRegexString(regexScript) {
    switch (Number(regexScript.substituteRegex)) {
        case substitute_find_regex.NONE:
            return regexScript.findRegex;
        case substitute_find_regex.RAW:
            return substituteParamsExtended(regexScript.findRegex);
        case substitute_find_regex.ESCAPED:
            return substituteParamsExtended(regexScript.findRegex, {}, sanitizeRegexMacro);
        default:
            console.warn(`runRegexScript: Unknown substituteRegex value ${regexScript.substituteRegex}. Using raw regex.`);
            return regexScript.findRegex;
    }
}

function isRegexScriptActiveForParams(script, { placement, isMarkdown, isPrompt, isEdit, depth }) {
    const isScopeMatch =
        (script.markdownOnly && isMarkdown) ||
        (script.promptOnly && isPrompt) ||
        (!script.markdownOnly && !script.promptOnly && !isMarkdown && !isPrompt);

    if (!isScopeMatch) {
        return false;
    }

    if (isEdit && !script.runOnEdit) {
        console.debug(`getRegexedString: Skipping script ${script.scriptName} because it does not run on edit`);
        return false;
    }

    if (typeof depth === 'number') {
        if (!isNaN(script.minDepth) && script.minDepth !== null && script.minDepth >= -1 && depth < script.minDepth) {
            console.debug(`getRegexedString: Skipping script ${script.scriptName} because depth ${depth} is less than minDepth ${script.minDepth}`);
            return false;
        }

        if (!isNaN(script.maxDepth) && script.maxDepth !== null && script.maxDepth >= 0 && depth > script.maxDepth) {
            console.debug(`getRegexedString: Skipping script ${script.scriptName} because depth ${depth} is greater than maxDepth ${script.maxDepth}`);
            return false;
        }
    }

    return Array.isArray(script.placement) && script.placement.includes(placement);
}

function getApplicableRegexScripts(placement, { isMarkdown, isPrompt, isEdit, depth } = {}) {
    if (extension_settings.disabledExtensions.includes('regex') || placement === undefined) {
        return [];
    }

    return getRegexScripts({ allowedOnly: true })
        .filter(script => isRegexScriptActiveForParams(script, { placement, isMarkdown, isPrompt, isEdit, depth }));
}

function hasSubstituteParamToken(value) {
    return SUBSTITUTE_PARAM_TOKEN_REGEX.test(String(value ?? ''));
}

function containsAstralCodePoint(value) {
    return /[\uD800-\uDBFF][\uDC00-\uDFFF]/.test(value);
}

function canApplyNativeUnicodeSemantics(nativeScripts, rawString) {
    if (!containsAstralCodePoint(rawString)) {
        return true;
    }

    return nativeScripts.every(script => script.flags.includes('u') || script.flags.includes('v'));
}

function toNativeRegexScript(regexScript, rawString) {
    const regexString = resolveRegexString(regexScript);
    const findRegex = regexFromString(regexString);

    if (!findRegex) {
        return null;
    }

    if ([...findRegex.flags].some(flag => !NATIVE_REGEX_SUPPORTED_FLAGS.has(flag))) {
        return null;
    }

    const replacement = regexScript.replaceString.replace(/{{match}}/gi, '$0');
    if (hasSubstituteParamToken(replacement)) {
        return null;
    }

    if (hasSubstituteParamToken(rawString) && REPLACEMENT_CAPTURE_REF_REGEX.test(replacement)) {
        return null;
    }

    const trimStrings = regexScript.trimStrings ?? [];
    if (trimStrings.some(hasSubstituteParamToken)) {
        return null;
    }

    return {
        scriptName: String(regexScript.scriptName || ''),
        pattern: findRegex.source,
        flags: findRegex.flags,
        global: findRegex.global,
        replacement,
        trimStrings,
    };
}

function runRegexScripts(scripts, rawString, { characterOverride } = {}) {
    let finalString = rawString;
    for (const script of scripts) {
        finalString = runRegexScript(script, finalString, { characterOverride });
    }
    return finalString;
}

/**
 * Parent function to fetch a regexed version of a raw string
 * @param {string} rawString The raw string to be regexed
 * @param {regex_placement} placement The placement of the string
 * @param {RegexParams} params The parameters to use for the regex script
 * @returns {string} The regexed string
 * @typedef {{characterOverride?: string, isMarkdown?: boolean, isPrompt?: boolean, isEdit?: boolean, depth?: number }} RegexParams The parameters to use for the regex script
 */
export function getRegexedString(rawString, placement, { characterOverride, isMarkdown, isPrompt, isEdit, depth } = {}) {
    // WTF have you passed me?
    if (typeof rawString !== 'string') {
        console.warn('getRegexedString: rawString is not a string. Returning empty string.');
        return '';
    }

    let finalString = rawString;
    if (extension_settings.disabledExtensions.includes('regex') || !rawString || placement === undefined) {
        return finalString;
    }

    const scripts = getApplicableRegexScripts(placement, { isMarkdown, isPrompt, isEdit, depth });
    finalString = runRegexScripts(scripts, finalString, { characterOverride });

    return finalString;
}

/**
 * Native-accelerated batch version of getRegexedString.
 * Existing SillyTavern-facing APIs remain synchronous; this is only for Tauri-owned async paths.
 * @param {{ rawString: string, placement: regex_placement, params?: RegexParams }[]} items Inputs to transform
 * @returns {Promise<string[]>} Transformed strings in the same order
 */
export async function getRegexedStringBatchAsync(items) {
    const results = new Array(items.length);
    const nativeTasks = [];
    const nativeIndexes = [];
    const nativeBackendAvailable = isNativeRegexBackendAvailable() && isNativeRegexBackendEnabled();

    for (const [index, item] of items.entries()) {
        const rawString = item?.rawString;
        const placement = item?.placement;
        const params = item?.params ?? {};

        if (typeof rawString !== 'string') {
            console.warn('getRegexedStringBatchAsync: rawString is not a string. Returning empty string.');
            results[index] = '';
            continue;
        }

        if (!rawString || extension_settings.disabledExtensions.includes('regex') || placement === undefined) {
            results[index] = rawString;
            continue;
        }

        const scripts = getApplicableRegexScripts(placement, params);
        if (scripts.length === 0) {
            results[index] = rawString;
            continue;
        }

        if (!nativeBackendAvailable) {
            results[index] = runRegexScripts(scripts, rawString, params);
            continue;
        }

        const nativeScripts = scripts.map(script => toNativeRegexScript(script, rawString));
        if (nativeScripts.every(Boolean) && canApplyNativeUnicodeSemantics(nativeScripts, rawString)) {
            nativeIndexes.push(index);
            nativeTasks.push({ text: rawString, scripts: nativeScripts });
        } else {
            results[index] = runRegexScripts(scripts, rawString, params);
        }
    }

    if (nativeTasks.length > 0) {
        const response = await applyNativeRegexBatch({ tasks: nativeTasks });
        if (!Array.isArray(response?.tasks) || response.tasks.length !== nativeTasks.length) {
            throw new Error('Native regex backend returned an invalid batch response');
        }

        response.tasks.forEach((task, offset) => {
            results[nativeIndexes[offset]] = String(task?.text ?? '');
        });
    }

    return results;
}

/**
 * Async single-input convenience wrapper for Tauri-owned call sites.
 * @param {string} rawString The raw string to be regexed
 * @param {regex_placement} placement The placement of the string
 * @param {RegexParams} params The parameters to use for the regex script
 * @returns {Promise<string>} The regexed string
 */
export async function getRegexedStringAsync(rawString, placement, params = {}) {
    const [result] = await getRegexedStringBatchAsync([{ rawString, placement, params }]);
    return result;
}

/**
 * Runs the provided regex script on the given string
 * @param {RegexScript} regexScript The regex script to run
 * @param {string} rawString The string to run the regex script on
 * @param {RegexScriptParams} params The parameters to use for the regex script
 * @returns {string} The new string
 * @typedef {{characterOverride?: string}} RegexScriptParams The parameters to use for the regex script
 */
export function runRegexScript(regexScript, rawString, { characterOverride } = {}) {
    let newString = rawString;
    if (!regexScript || !!(regexScript.disabled) || !regexScript?.findRegex || !rawString) {
        return newString;
    }

    const regexString = resolveRegexString(regexScript);
    const findRegex = RegexProvider.instance.get(regexString);

    // The user skill issued. Return with nothing.
    if (!findRegex) {
        return newString;
    }

    // Run replacement. Currently does not support the Overlay strategy
    newString = rawString.replace(findRegex, function (match) {
        const args = [...arguments];
        const replaceString = regexScript.replaceString.replace(/{{match}}/gi, '$0');
        const replaceWithGroups = replaceString.replaceAll(/\$(\d+)|\$<([^>]+)>/g, (_, num, groupName) => {
            if (num) {
                // Handle numbered capture groups ($1, $2, etc.)
                match = args[Number(num)];
            } else if (groupName) {
                // Handle named capture groups ($<name>)
                const groups = args[args.length - 1];
                match = groups && typeof groups === 'object' && groups[groupName];
            }

            // No match found - return the empty string
            if (!match) {
                return '';
            }

            // Remove trim strings from the match
            const filteredMatch = filterString(match, regexScript.trimStrings, { characterOverride });

            return filteredMatch;
        });

        // Substitute at the end
        return substituteParams(replaceWithGroups);
    });

    return newString;
}

/**
 * Filters anything to trim from the regex match
 * @param {string} rawString The raw string to filter
 * @param {string[]} trimStrings The strings to trim
 * @param {RegexScriptParams} params The parameters to use for the regex filter
 * @returns {string} The filtered string
 */
function filterString(rawString, trimStrings, { characterOverride } = {}) {
    let finalString = rawString;
    trimStrings.forEach((trimString) => {
        const subTrimString = substituteParams(trimString, { name2Override: characterOverride });
        finalString = finalString.replaceAll(subTrimString, '');
    });

    return finalString;
}
