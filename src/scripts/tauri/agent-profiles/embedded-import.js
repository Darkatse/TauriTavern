// @ts-check

import { t } from '../../i18n.js';
import { POPUP_RESULT, POPUP_TYPE, Popup } from '../../popup.js';
import { SURFACE, applySurface } from '../../tauritavern/layout-kit.js';
import { buildSkillImportReminderKey, hasSkillImportReminder, setSkillImportReminder } from '../agent-skills/reminders.js';

const EMBEDDED_PROFILES_VERSION = 1;

/**
 * @param {unknown} value
 * @returns {Record<string, any>}
 */
function requirePlainObject(value) {
    if (!value || typeof value !== 'object' || Array.isArray(value)) {
        throw new Error('Embedded Agent Profile payload must be an object');
    }

    return /** @type {Record<string, any>} */ (value);
}

/**
 * @param {unknown} value
 * @param {string} label
 */
function requireNonEmptyString(value, label) {
    const resolved = String(value || '').trim();
    if (!resolved) {
        throw new Error(`${label} is required`);
    }

    return resolved;
}

/**
 * @param {unknown} embedded
 */
function normalizeEmbeddedProfiles(embedded) {
    if (embedded === null || embedded === undefined) {
        return [];
    }

    const payload = requirePlainObject(embedded);
    if (Number(payload.version) !== EMBEDDED_PROFILES_VERSION) {
        throw new Error(`Unsupported embedded Agent Profile schema version: ${payload.version}`);
    }
    if (payload.items === null || payload.items === undefined) {
        return [];
    }
    if (!Array.isArray(payload.items)) {
        throw new Error('Embedded Agent Profile items must be an array');
    }

    return payload.items.map((item, index) => {
        const object = requirePlainObject(item);
        const profile = requirePlainObject(object.profile);
        const id = requireNonEmptyString(profile.id, `items[${index}].profile.id`);
        if (id === 'default-writer') {
            throw new Error('Embedded Agent Profile cannot replace built-in profile: default-writer');
        }
        return { index, profile };
    });
}

/**
 * @param {any} preset
 */
export function extractPresetEmbeddedProfiles(preset) {
    return normalizeEmbeddedProfiles(preset?.extensions?.tauritavern?.agentProfiles);
}

/**
 * @param {any} character
 */
export function extractCharacterEmbeddedProfiles(character) {
    return normalizeEmbeddedProfiles(
        character?.data?.extensions?.tauritavern?.agentProfiles
        ?? character?.extensions?.tauritavern?.agentProfiles,
    );
}

function getAgentProfileApi() {
    const hostAbi = window.__TAURITAVERN__;
    if (!hostAbi) {
        return null;
    }

    const profileApi = hostAbi.api?.agent?.profiles;
    if (!profileApi || typeof profileApi !== 'object') {
        throw new Error('TauriTavern Agent Profile API is not available');
    }

    if (typeof profileApi.load !== 'function' || typeof profileApi.save !== 'function') {
        throw new Error('TauriTavern Agent Profile API is incomplete');
    }

    return profileApi;
}

/**
 * @param {unknown} value
 * @returns {string}
 */
function stableStringify(value) {
    if (value === null || typeof value !== 'object') {
        const primitive = JSON.stringify(value);
        return primitive === undefined ? 'undefined' : primitive;
    }

    if (Array.isArray(value)) {
        return `[${value.map(stableStringify).join(',')}]`;
    }

    const object = /** @type {Record<string, unknown>} */ (value);
    return `{${Object.keys(object).sort().map(key => `${JSON.stringify(key)}:${stableStringify(object[key])}`).join(',')}}`;
}

/**
 * @param {string} text
 */
function fnv1aHex(text) {
    let hash = 0x811c9dc5;
    for (let i = 0; i < text.length; i++) {
        hash ^= text.charCodeAt(i);
        hash = Math.imul(hash, 0x01000193);
    }

    return (hash >>> 0).toString(16).padStart(8, '0');
}

/**
 * @returns {any}
 */
function getToastr() {
    return /** @type {any} */ (toastr);
}

/**
 * @param {any} profile
 */
function profileTitle(profile) {
    return String(profile.displayName || profile.id || t`Unnamed Agent Profile`);
}

/**
 * @param {any} existing
 * @param {any} profile
 */
function profileConflictKind(existing, profile) {
    if (!existing) {
        return 'new';
    }
    return stableStringify(existing) === stableStringify(profile) ? 'same' : 'different';
}

/**
 * @param {string} kind
 */
function conflictLabel(kind) {
    if (kind === 'new') {
        return t`New Agent Profile`;
    }
    if (kind === 'same') {
        return t`Already installed`;
    }
    if (kind === 'different') {
        return t`Different installed version`;
    }
    return kind;
}

/**
 * @param {{ item: any; existing: any; conflictKind: string }[]} previews
 * @param {string} sourceLabel
 */
function buildPreviewPopupContent(previews, sourceLabel) {
    const root = document.createElement('div');
    root.classList.add('tauritavern-agent-profile-import');

    const title = document.createElement('h3');
    title.textContent = t`Import embedded Agent Profiles?`;
    root.append(title);

    const source = document.createElement('p');
    source.textContent = t`Source: ${sourceLabel}`;
    root.append(source);

    const list = document.createElement('div');
    list.style.display = 'flex';
    list.style.flexDirection = 'column';
    list.style.gap = '0.75rem';
    root.append(list);

    for (const entry of previews) {
        const profile = entry.item.profile;
        const section = document.createElement('section');
        section.style.border = '1px solid var(--SmartThemeBorderColor)';
        section.style.borderRadius = '6px';
        section.style.padding = '0.75rem';
        section.style.display = 'flex';
        section.style.flexDirection = 'column';
        section.style.gap = '0.45rem';
        list.append(section);

        const header = document.createElement('div');
        header.style.display = 'flex';
        header.style.justifyContent = 'space-between';
        header.style.gap = '0.75rem';
        section.append(header);

        const name = document.createElement('strong');
        name.textContent = profileTitle(profile);
        name.style.overflowWrap = 'anywhere';
        header.append(name);

        const conflict = document.createElement('span');
        conflict.textContent = conflictLabel(entry.conflictKind);
        conflict.style.opacity = '0.8';
        header.append(conflict);

        const id = document.createElement('div');
        id.textContent = String(profile.id);
        id.style.fontSize = '0.9em';
        id.style.opacity = '0.8';
        id.style.overflowWrap = 'anywhere';
        section.append(id);

        const description = String(profile.description || '').trim();
        if (description) {
            const desc = document.createElement('div');
            desc.textContent = description;
            desc.style.overflowWrap = 'anywhere';
            section.append(desc);
        }

        const action = document.createElement('div');
        action.style.display = 'flex';
        action.style.justifyContent = 'flex-end';
        action.style.alignItems = 'center';
        action.style.gap = '0.5rem';
        section.append(action);

        if (entry.conflictKind === 'new') {
            const label = document.createElement('label');
            label.classList.add('checkbox_label');
            const checkbox = document.createElement('input');
            checkbox.type = 'checkbox';
            checkbox.checked = true;
            checkbox.dataset.profileImportIndex = String(entry.item.index);
            label.append(checkbox);
            const text = document.createElement('span');
            text.textContent = t`Import`;
            label.append(text);
            action.append(label);
        } else if (entry.conflictKind === 'different') {
            const select = document.createElement('select');
            select.classList.add('text_pole');
            select.dataset.profileConflictIndex = String(entry.item.index);

            const skip = document.createElement('option');
            skip.value = 'skip';
            skip.textContent = t`Skip`;
            select.append(skip);

            const replace = document.createElement('option');
            replace.value = 'replace';
            replace.textContent = t`Replace`;
            select.append(replace);

            action.append(select);
        } else {
            const text = document.createElement('span');
            text.textContent = t`No action needed`;
            action.append(text);
        }
    }

    return root;
}

/**
 * @param {HTMLElement} root
 * @param {{ item: any; existing: any; conflictKind: string }[]} previews
 */
function collectImportDecisions(root, previews) {
    const decisions = [];
    for (const entry of previews) {
        if (entry.conflictKind === 'new') {
            const checkbox = root.querySelector(`[data-profile-import-index="${entry.item.index}"]`);
            if (checkbox instanceof HTMLInputElement && checkbox.checked) {
                decisions.push(entry.item);
            }
            continue;
        }

        if (entry.conflictKind === 'different') {
            const select = root.querySelector(`[data-profile-conflict-index="${entry.item.index}"]`);
            if (select instanceof HTMLSelectElement && select.value === 'replace') {
                decisions.push(entry.item);
            }
        }
    }
    return decisions;
}

/**
 * @param {{ item: any; existing: any; conflictKind: string }[]} previews
 */
function hasImportablePreviews(previews) {
    return previews.some(entry => entry.conflictKind === 'new' || entry.conflictKind === 'different');
}

/**
 * @param {{ item: any; existing: any; conflictKind: string }[]} previews
 * @param {string} sourceLabel
 */
async function showImportPopup(previews, sourceLabel) {
    const root = buildPreviewPopupContent(previews, sourceLabel);
    const hasImportableProfile = hasImportablePreviews(previews);
    const popup = new Popup(root, hasImportableProfile ? POPUP_TYPE.CONFIRM : POPUP_TYPE.TEXT, '', {
        okButton: hasImportableProfile ? t`Import selected Profiles` : t`OK`,
        cancelButton: hasImportableProfile ? t`Not now` : false,
        wide: true,
        allowVerticalScrolling: true,
        leftAlign: true,
    });
    popup.dlg.classList.add('tauritavern-agent-profile-import-popup');
    applySurface(popup.dlg, SURFACE.FullscreenWindow);

    const result = await popup.show();
    if (result !== POPUP_RESULT.AFFIRMATIVE) {
        return null;
    }
    if (!hasImportableProfile) {
        return [];
    }
    return collectImportDecisions(root, previews);
}

/**
 * @param {{ items: any[]; sourceLabel: string; storageKey: string }} options
 */
async function previewAndPromptEmbeddedProfiles({ items, sourceLabel, storageKey }) {
    const profileApi = getAgentProfileApi();
    if (!profileApi || items.length === 0) {
        return;
    }

    if (hasSkillImportReminder(storageKey)) {
        return;
    }

    const previews = [];
    for (const item of items) {
        const existing = (await profileApi.load({ profileId: item.profile.id }))?.profile || null;
        previews.push({
            item,
            existing,
            conflictKind: profileConflictKind(existing, item.profile),
        });
    }

    const decisions = await showImportPopup(previews, sourceLabel);
    if (decisions === null) {
        setSkillImportReminder(storageKey);
        return;
    }

    for (const item of decisions) {
        await profileApi.save({ profile: item.profile });
    }
    if (decisions.length > 0) {
        getToastr().success(t`${decisions.length} Agent Profile(s) imported`, t`Agent Profiles`);
    }
    setSkillImportReminder(storageKey);
}

/**
 * @param {unknown} error
 */
function reportEmbeddedProfileError(error) {
    console.error('Agent Profile embedded import failed', error);
    const message = error instanceof Error ? error.message : String(error || t`Unknown error`);
    getToastr().error(message, t`Agent Profile import failed`);
}

/**
 * @param {{ apiId: string; name: string; preset: any }} options
 */
export async function maybePromptForPresetEmbeddedProfiles({ apiId, name, preset }) {
    try {
        if (!getAgentProfileApi()) {
            return;
        }

        const embedded = preset?.extensions?.tauritavern?.agentProfiles;
        const items = extractPresetEmbeddedProfiles(preset);
        if (items.length === 0) {
            return;
        }

        const hash = fnv1aHex(stableStringify(embedded));
        const storageKey = buildSkillImportReminderKey(['AlertProfilePreset', apiId, name, hash]);
        await previewAndPromptEmbeddedProfiles({
            items,
            sourceLabel: String(name || t`Imported preset`),
            storageKey,
        });
    } catch (error) {
        reportEmbeddedProfileError(error);
    }
}

/**
 * @param {{ avatarFileName: string; label: string; loadCharacter: () => Promise<any> }} options
 */
export async function maybePromptForCharacterEmbeddedProfiles({ avatarFileName, label, loadCharacter }) {
    try {
        if (!getAgentProfileApi()) {
            return;
        }

        if (typeof loadCharacter !== 'function') {
            throw new Error('loadCharacter is required');
        }

        const character = await loadCharacter();
        const embedded = character?.data?.extensions?.tauritavern?.agentProfiles
            ?? character?.extensions?.tauritavern?.agentProfiles;
        const items = extractCharacterEmbeddedProfiles(character);
        if (items.length === 0) {
            return;
        }

        const hash = fnv1aHex(stableStringify(embedded));
        const storageKey = buildSkillImportReminderKey(['AlertProfileCharacter', avatarFileName, hash]);
        await previewAndPromptEmbeddedProfiles({
            items,
            sourceLabel: String(label || character?.name || character?.data?.name || t`Imported character`),
            storageKey,
        });
    } catch (error) {
        reportEmbeddedProfileError(error);
    }
}
