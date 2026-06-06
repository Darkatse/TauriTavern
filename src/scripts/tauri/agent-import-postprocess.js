// @ts-check

import { t } from '../i18n.js';

/**
 * @typedef {object} ImportedCharacterAgentAssetScanOptions
 * @property {string} avatarFileName
 * @property {string} label
 * @property {any} [character]
 * @property {any} [postImport]
 */

/** @type {ImportedCharacterAgentAssetScanOptions[]} */
const importedCharacterAgentAssetQueue = [];
let importedCharacterAgentAssetQueuePaused = 0;
let importedCharacterAgentAssetQueueRunning = false;
let importedCharacterAgentAssetQueueScheduled = false;

function scheduleImportedCharacterAgentAssetQueue() {
    if (importedCharacterAgentAssetQueueScheduled
        || importedCharacterAgentAssetQueueRunning
        || importedCharacterAgentAssetQueuePaused > 0
        || importedCharacterAgentAssetQueue.length === 0) {
        return;
    }

    importedCharacterAgentAssetQueueScheduled = true;
    setTimeout(() => {
        importedCharacterAgentAssetQueueScheduled = false;
        void drainImportedCharacterAgentAssetQueue();
    }, 0);
}

async function drainImportedCharacterAgentAssetQueue() {
    if (importedCharacterAgentAssetQueueRunning || importedCharacterAgentAssetQueuePaused > 0) {
        return;
    }

    importedCharacterAgentAssetQueueRunning = true;
    try {
        while (importedCharacterAgentAssetQueue.length > 0 && importedCharacterAgentAssetQueuePaused === 0) {
            const options = importedCharacterAgentAssetQueue.shift();
            if (!options) {
                continue;
            }
            await maybePromptForImportedCharacterAgentAssets(options);
        }
    } finally {
        importedCharacterAgentAssetQueueRunning = false;
        scheduleImportedCharacterAgentAssetQueue();
    }
}

export function pauseImportedCharacterAgentAssetQueue() {
    importedCharacterAgentAssetQueuePaused += 1;
}

export function resumeImportedCharacterAgentAssetQueue() {
    importedCharacterAgentAssetQueuePaused = Math.max(0, importedCharacterAgentAssetQueuePaused - 1);
    scheduleImportedCharacterAgentAssetQueue();
}

/**
 * @param {ImportedCharacterAgentAssetScanOptions} options
 */
export function enqueueImportedCharacterAgentAssetScan(options) {
    importedCharacterAgentAssetQueue.push(options);
    scheduleImportedCharacterAgentAssetQueue();
}

/**
 * @param {any} character
 * @param {'agentProfiles' | 'skills'} field
 */
function getImportedCharacterTauriExtensionField(character, field) {
    const sources = [
        character?.data?.extensions?.tauritavern,
        character?.extensions?.tauritavern,
    ];

    for (const source of sources) {
        if (source && typeof source === 'object' && !Array.isArray(source)
            && Object.prototype.hasOwnProperty.call(source, field)) {
            return source[field];
        }
    }

    return undefined;
}

/**
 * @param {{ character?: any; postImport?: any }} options
 * @param {'agentProfiles' | 'skills'} field
 * @param {'has_agent_profiles' | 'has_agent_skills'} hintField
 */
function hasImportedCharacterAgentAsset({ character, postImport }, field, hintField) {
    if (postImport && typeof postImport === 'object' && !Array.isArray(postImport)
        && Object.prototype.hasOwnProperty.call(postImport, hintField)) {
        return Boolean(postImport[hintField]);
    }

    const value = getImportedCharacterTauriExtensionField(character, field);
    return value !== undefined && value !== null;
}

function getToastr() {
    return /** @type {any} */ (toastr);
}

/**
 * @param {unknown} error
 */
function reportImportedCharacterAgentAssetError(error) {
    console.error('Failed to start embedded Agent asset import prompt for character', error);
    const message = error instanceof Error ? error.message : String(error || t`Unknown error`);
    getToastr().error(message, t`Agent embedded import failed`);
}

/**
 * @param {ImportedCharacterAgentAssetScanOptions} options
 */
async function maybePromptForImportedCharacterAgentAssets(options) {
    try {
        await promptForImportedCharacterAgentAssets(options);
    } catch (error) {
        reportImportedCharacterAgentAssetError(error);
    }
}

/**
 * @param {ImportedCharacterAgentAssetScanOptions} options
 */
async function promptForImportedCharacterAgentAssets(options) {
    const hostAbi = window.__TAURITAVERN__;
    if (!hostAbi) {
        return;
    }

    const { avatarFileName, label, character = null, postImport = null } = options;
    const hasProfiles = hasImportedCharacterAgentAsset(
        { character, postImport },
        'agentProfiles',
        'has_agent_profiles',
    );
    const hasSkills = hasImportedCharacterAgentAsset(
        { character, postImport },
        'skills',
        'has_agent_skills',
    );

    if (!hasProfiles && !hasSkills) {
        return;
    }

    if (hasProfiles && !hostAbi.api?.agent?.profiles) {
        throw new Error('TauriTavern Agent Profile API is not available');
    }

    if (hasSkills && !hostAbi.api?.skill) {
        throw new Error('TauriTavern Agent Skill API is not available');
    }

    const importedCharacter = character;
    if (!importedCharacter) {
        throw new Error('Imported character payload is required for Agent embedded asset scan');
    }

    const loadCharacter = async () => importedCharacter;

    if (hasProfiles) {
        const { maybePromptForCharacterEmbeddedProfiles } = await import('./agent-profiles/embedded-import.js');
        await maybePromptForCharacterEmbeddedProfiles({ avatarFileName, label, loadCharacter });
    }

    if (hasSkills) {
        const { maybePromptForCharacterEmbeddedSkills } = await import('./agent-skills/embedded-import.js');
        await maybePromptForCharacterEmbeddedSkills({ avatarFileName, label, loadCharacter });
    }
}
