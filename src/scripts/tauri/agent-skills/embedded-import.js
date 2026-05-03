// @ts-check

import { t, translate } from '../../i18n.js';
import { POPUP_RESULT, POPUP_TYPE, Popup } from '../../popup.js';
import { SURFACE, applySurface } from '../../tauritavern/layout-kit.js';
import { buildSkillImportReminderKey, hasSkillImportReminder, setSkillImportReminder } from './reminders.js';

const EMBEDDED_SKILLS_VERSION = 1;
const INLINE_FILES_BUNDLE_FORMAT = 'inline-files-v1';

/**
 * @param {unknown} value
 * @returns {Record<string, any>}
 */
function requirePlainObject(value) {
    if (!value || typeof value !== 'object' || Array.isArray(value)) {
        throw new Error('Embedded Agent Skill payload must be an object');
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
 * @param {unknown} value
 */
function normalizeEncoding(value) {
    const encoding = String(value || 'utf8').trim().toLowerCase();
    if (!['utf8', 'utf-8', 'base64'].includes(encoding)) {
        throw new Error(`Unsupported embedded Agent Skill file encoding: ${encoding}`);
    }

    return encoding;
}

/**
 * @param {unknown} value
 * @param {string} label
 */
function normalizeOptionalNonNegativeInteger(value, label) {
    if (value === null || value === undefined) {
        return undefined;
    }

    const number = Number(value);
    if (!Number.isInteger(number) || number < 0) {
        throw new Error(`${label} must be a non-negative integer`);
    }

    return number;
}

/**
 * @param {unknown} value
 * @param {{ kind: string; id: string; label: string }} fallbackSource
 */
function normalizeEmbeddedSource(value, fallbackSource) {
    const source = value === null || value === undefined ? {} : requirePlainObject(value);
    const kind = requireNonEmptyString(fallbackSource.kind || source.kind, 'source.kind');
    const id = requireNonEmptyString(fallbackSource.id || source.id, 'source.id');
    return {
        ...source,
        kind,
        id,
        label: String(source.label || fallbackSource.label || source.kind || fallbackSource.kind).trim(),
    };
}

/**
 * @param {unknown} value
 */
function normalizeEmbeddedFile(value) {
    const file = requirePlainObject(value);

    /** @type {Record<string, any>} */
    const output = {
        path: requireNonEmptyString(file.path, 'skill file path'),
        encoding: normalizeEncoding(file.encoding),
        content: file.content,
    };

    if (typeof output.content !== 'string') {
        throw new Error('skill file content must be a string');
    }

    const mediaType = String(file.mediaType || '').trim();
    if (mediaType) {
        output.mediaType = mediaType;
    }

    const sizeBytes = normalizeOptionalNonNegativeInteger(file.sizeBytes, 'sizeBytes');
    if (sizeBytes !== undefined) {
        output.sizeBytes = sizeBytes;
    }

    const sha256 = String(file.sha256 || '').trim();
    if (sha256) {
        output.sha256 = sha256;
    }

    return output;
}

/**
 * @param {unknown} value
 * @param {number} index
 * @param {{ kind: string; id: string; label: string }} fallbackSource
 */
function normalizeEmbeddedItem(value, index, fallbackSource) {
    const item = requirePlainObject(value);
    const bundleFormat = requireNonEmptyString(item.bundleFormat, `items[${index}].bundleFormat`);
    if (bundleFormat !== INLINE_FILES_BUNDLE_FORMAT) {
        throw new Error(`Unsupported embedded Agent Skill bundle format: ${bundleFormat}`);
    }

    if (!Array.isArray(item.files) || item.files.length === 0) {
        throw new Error(`Embedded Agent Skill item ${index + 1} requires at least one file`);
    }

    const source = normalizeEmbeddedSource(item.source, fallbackSource);
    return {
        index,
        input: {
            kind: 'inlineFiles',
            files: item.files.map(normalizeEmbeddedFile),
            source,
        },
    };
}

/**
 * @param {unknown} embedded
 * @param {{ kind: string; id: string; label: string }} fallbackSource
 */
function normalizeEmbeddedSkills(embedded, fallbackSource) {
    if (embedded === null || embedded === undefined) {
        return [];
    }

    const payload = requirePlainObject(embedded);
    if (Number(payload.version) !== EMBEDDED_SKILLS_VERSION) {
        throw new Error(`Unsupported embedded Agent Skill schema version: ${payload.version}`);
    }

    if (payload.items === null || payload.items === undefined) {
        return [];
    }

    if (!Array.isArray(payload.items)) {
        throw new Error('Embedded Agent Skill items must be an array');
    }

    return payload.items.map((item, index) => normalizeEmbeddedItem(item, index, fallbackSource));
}

/**
 * @param {any} preset
 * @param {{ apiId: string; name: string }} source
 */
export function extractPresetEmbeddedSkills(preset, source) {
    const label = String(source.name || '').trim() || t`Imported preset`;
    return normalizeEmbeddedSkills(preset?.extensions?.tauritavern?.skills, {
        kind: 'preset',
        id: buildPresetSkillSourceId(source.apiId, label),
        label,
    });
}

/**
 * @param {any} character
 * @param {{ label: string; avatarFileName?: string; characterId?: string }} source
 */
export function extractCharacterEmbeddedSkills(character, source) {
    const embedded = character?.data?.extensions?.tauritavern?.skills
        ?? character?.extensions?.tauritavern?.skills;
    const label = String(source.label || character?.name || character?.data?.name || '').trim() || t`Imported character`;
    return normalizeEmbeddedSkills(embedded, {
        kind: 'character',
        id: buildCharacterSkillSourceId(source.avatarFileName || source.characterId || label),
        label,
    });
}

/**
 * @param {unknown} apiId
 * @param {unknown} name
 */
function buildPresetSkillSourceId(apiId, name) {
    return `preset:${requireNonEmptyString(apiId, 'apiId')}:${requireNonEmptyString(name, 'preset name')}`;
}

/**
 * @param {unknown} value
 */
function buildCharacterSkillSourceId(value) {
    const characterId = requireNonEmptyString(value, 'character id').replace(/\.png$/i, '');
    return `character:${characterId}`;
}

function getSkillApi() {
    const hostAbi = window.__TAURITAVERN__;
    if (!hostAbi) {
        return null;
    }

    const skillApi = hostAbi.api?.skill;
    if (!skillApi || typeof skillApi !== 'object') {
        throw new Error('TauriTavern Agent Skill API is not available');
    }

    if (typeof skillApi.previewImport !== 'function' || typeof skillApi.installImport !== 'function') {
        throw new Error('TauriTavern Agent Skill API is incomplete');
    }

    return skillApi;
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
 * @param {number} bytes
 */
function formatBytes(bytes) {
    if (!Number.isFinite(bytes) || bytes <= 0) {
        return '0 B';
    }

    const units = ['B', 'KiB', 'MiB', 'GiB'];
    let value = bytes;
    let unitIndex = 0;
    while (value >= 1024 && unitIndex < units.length - 1) {
        value /= 1024;
        unitIndex++;
    }

    return `${value.toFixed(value >= 10 || unitIndex === 0 ? 0 : 1)} ${units[unitIndex]}`;
}

/**
 * @param {HTMLElement} parent
 * @param {string} tagName
 * @param {string} text
 */
function appendText(parent, tagName, text) {
    const element = document.createElement(tagName);
    element.textContent = text;
    parent.append(element);
    return element;
}

/**
 * @param {any} preview
 */
function getConflictLabel(preview) {
    const kind = String(preview?.conflict?.kind || '');
    if (kind === 'new') {
        return t`New Agent Skill`;
    }
    if (kind === 'same') {
        return t`Already installed`;
    }
    if (kind === 'different') {
        return t`Different installed version`;
    }

    return kind || t`Unknown`;
}

/**
 * @param {{ preview: any; item: any }[]} previews
 * @param {string} sourceLabel
 */
function buildPreviewPopupContent(previews, sourceLabel) {
    const root = document.createElement('div');
    root.classList.add('tauritavern-agent-skill-import');

    appendText(root, 'h3', t`Import embedded Agent Skills?`);
    appendText(root, 'p', t`Source: ${sourceLabel}`);

    const list = document.createElement('div');
    list.style.display = 'flex';
    list.style.flexDirection = 'column';
    list.style.gap = '0.75rem';
    root.append(list);

    for (const entry of previews) {
        const preview = entry.preview;
        const skill = preview?.skill || {};
        const conflictKind = String(preview?.conflict?.kind || '');

        const item = document.createElement('section');
        item.style.border = '1px solid var(--SmartThemeBorderColor)';
        item.style.borderRadius = '6px';
        item.style.padding = '0.75rem';
        item.style.display = 'flex';
        item.style.flexDirection = 'column';
        item.style.gap = '0.45rem';
        list.append(item);

        const header = document.createElement('div');
        header.style.display = 'flex';
        header.style.gap = '0.6rem';
        header.style.alignItems = 'center';
        header.style.justifyContent = 'space-between';
        item.append(header);

        const title = appendText(header, 'strong', String(skill.displayName || skill.name || t`Unnamed Skill`));
        title.style.overflowWrap = 'anywhere';

        const conflict = appendText(header, 'span', getConflictLabel(preview));
        conflict.style.fontSize = '0.9em';
        conflict.style.opacity = '0.8';

        const description = appendText(item, 'div', String(skill.description || ''));
        description.style.overflowWrap = 'anywhere';

        const totalBytes = Number(skill.totalBytes || 0);
        const metadata = [
            t`${Number(skill.fileCount || 0)} files`,
            formatBytes(totalBytes),
        ];
        if (skill.hasScripts) {
            metadata.push(t`scripts stored only`);
        }
        if (skill.hasBinary) {
            metadata.push(t`binary files`);
        }

        const meta = appendText(item, 'div', metadata.join(' · '));
        meta.style.fontSize = '0.9em';
        meta.style.opacity = '0.8';

        if (Array.isArray(preview.warnings) && preview.warnings.length > 0) {
            const warnings = document.createElement('ul');
            warnings.style.margin = '0.25rem 0 0 1rem';
            for (const warning of preview.warnings) {
                appendText(warnings, 'li', translate(String(warning)));
            }
            item.append(warnings);
        }

        const details = document.createElement('details');
        const summary = appendText(details, 'summary', t`Files`);
        summary.style.cursor = 'pointer';
        const files = document.createElement('ul');
        files.style.margin = '0.25rem 0 0 1rem';
        for (const file of Array.isArray(preview.files) ? preview.files : []) {
            appendText(files, 'li', `${String(file.path || '')} (${formatBytes(Number(file.sizeBytes || 0))})`);
        }
        details.append(files);
        item.append(details);

        const action = document.createElement('div');
        action.style.display = 'flex';
        action.style.justifyContent = 'flex-end';
        action.style.alignItems = 'center';
        action.style.gap = '0.5rem';
        item.append(action);

        if (conflictKind === 'new') {
            const label = document.createElement('label');
            label.classList.add('checkbox_label');
            const checkbox = document.createElement('input');
            checkbox.type = 'checkbox';
            checkbox.checked = true;
            checkbox.dataset.skillImportIndex = String(entry.item.index);
            label.append(checkbox);
            const text = document.createElement('span');
            text.textContent = t`Import`;
            label.append(text);
            action.append(label);
        } else if (conflictKind === 'different') {
            const select = document.createElement('select');
            select.classList.add('text_pole');
            select.dataset.skillConflictIndex = String(entry.item.index);

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
            appendText(action, 'span', t`Link source`);
        }
    }

    return root;
}

/**
 * @param {HTMLElement} root
 * @param {{ preview: any; item: any }[]} previews
 */
function collectImportDecisions(root, previews) {
    const decisions = [];
    for (const entry of previews) {
        const conflictKind = String(entry.preview?.conflict?.kind || '');
        if (conflictKind === 'new') {
            const checkbox = root.querySelector(`[data-skill-import-index="${entry.item.index}"]`);
            if (checkbox instanceof HTMLInputElement && checkbox.checked) {
                decisions.push({ item: entry.item });
            }
            continue;
        }

        if (conflictKind === 'different') {
            const select = root.querySelector(`[data-skill-conflict-index="${entry.item.index}"]`);
            if (select instanceof HTMLSelectElement && select.value === 'replace') {
                decisions.push({ item: entry.item, conflictStrategy: 'replace' });
            }
            continue;
        }

        if (conflictKind === 'same') {
            decisions.push({ item: entry.item });
        }
    }

    return decisions;
}

/**
 * @param {{ preview: any; item: any }[]} previews
 * @param {string} sourceLabel
 */
async function showImportPopup(previews, sourceLabel) {
    const root = buildPreviewPopupContent(previews, sourceLabel);
    const hasImportableSkill = hasImportablePreviews(previews);

    const popup = new Popup(root, hasImportableSkill ? POPUP_TYPE.CONFIRM : POPUP_TYPE.TEXT, '', {
        okButton: hasImportableSkill ? t`Import or link selected Skills` : t`OK`,
        cancelButton: hasImportableSkill ? t`Not now` : false,
        wide: true,
        allowVerticalScrolling: true,
        leftAlign: true,
    });
    popup.dlg.classList.add('tauritavern-agent-skill-import-popup');
    applySurface(popup.dlg, SURFACE.FullscreenWindow);

    const result = await popup.show();
    if (result !== POPUP_RESULT.AFFIRMATIVE) {
        return null;
    }

    if (!hasImportableSkill) {
        return [];
    }

    return collectImportDecisions(root, previews);
}

/**
 * @param {{ preview: any; item: any }[]} previews
 */
function hasImportablePreviews(previews) {
    return previews.some(entry => {
        const kind = String(entry.preview?.conflict?.kind || '');
        return kind === 'new' || kind === 'different' || kind === 'same';
    });
}

/**
 * @returns {any}
 */
function getToastr() {
    return /** @type {any} */ (toastr);
}

/**
 * @param {any[]} results
 */
function reportInstallResults(results) {
    const installed = results.filter(result => result?.action === 'installed').length;
    const replaced = results.filter(result => result?.action === 'replaced').length;
    const unchanged = results.filter(result => result?.action === 'already_installed').length;

    const parts = [];
    if (installed) {
        parts.push(t`${installed} installed`);
    }
    if (replaced) {
        parts.push(t`${replaced} replaced`);
    }
    if (unchanged) {
        parts.push(t`${unchanged} unchanged`);
    }

    getToastr().success(parts.join(', ') || t`Agent Skills imported`, t`Agent Skills`);
}

/**
 * @param {unknown} error
 */
function reportEmbeddedSkillError(error) {
    console.error('Agent Skill embedded import failed', error);
    const message = error instanceof Error ? error.message : String(error || t`Unknown error`);
    getToastr().error(message, t`Agent Skill import failed`);
}

/**
 * @param {{ items: any[]; sourceLabel: string; storageKey: string }} options
 */
async function previewAndPromptEmbeddedSkills({ items, sourceLabel, storageKey }) {
    const skillApi = getSkillApi();
    if (!skillApi || items.length === 0) {
        return;
    }

    if (hasSkillImportReminder(storageKey)) {
        return;
    }

    const previews = [];
    for (const item of items) {
        previews.push({
            item,
            preview: await skillApi.previewImport(item.input),
        });
    }

    const decisions = await showImportPopup(previews, sourceLabel);
    if (decisions === null) {
        setSkillImportReminder(storageKey);
        return;
    }

    if (decisions.length === 0) {
        setSkillImportReminder(storageKey);
        if (hasImportablePreviews(previews)) {
            getToastr().info(t`No Agent Skills imported`);
        }
        return;
    }

    const results = [];
    for (const decision of decisions) {
        const request = /** @type {any} */ ({
            input: decision.item.input,
        });
        if (decision.conflictStrategy !== undefined) {
            request.conflictStrategy = decision.conflictStrategy;
        }
        results.push(await skillApi.installImport(request));
    }

    reportInstallResults(results);
    setSkillImportReminder(storageKey);
}

/**
 * @param {{ apiId: string; name: string; preset: any }} options
 */
export async function maybePromptForPresetEmbeddedSkills({ apiId, name, preset }) {
    try {
        if (!getSkillApi()) {
            return;
        }

        const embedded = preset?.extensions?.tauritavern?.skills;
        const items = extractPresetEmbeddedSkills(preset, { apiId, name });
        if (items.length === 0) {
            return;
        }

        const hash = fnv1aHex(stableStringify(embedded));
        const storageKey = buildSkillImportReminderKey(['AlertSkillPreset', apiId, name, hash]);
        await previewAndPromptEmbeddedSkills({
            items,
            sourceLabel: String(name || t`Imported preset`),
            storageKey,
        });
    } catch (error) {
        reportEmbeddedSkillError(error);
    }
}

/**
 * @param {{ avatarFileName: string; label: string; loadCharacter: () => Promise<any> }} options
 */
export async function maybePromptForCharacterEmbeddedSkills({ avatarFileName, label, loadCharacter }) {
    try {
        if (!getSkillApi()) {
            return;
        }

        if (typeof loadCharacter !== 'function') {
            throw new Error('loadCharacter is required');
        }

        const character = await loadCharacter();
        const embedded = character?.data?.extensions?.tauritavern?.skills
            ?? character?.extensions?.tauritavern?.skills;
        const items = extractCharacterEmbeddedSkills(character, { label, avatarFileName });
        if (items.length === 0) {
            return;
        }

        const hash = fnv1aHex(stableStringify(embedded));
        const storageKey = buildSkillImportReminderKey(['AlertSkillCharacter', avatarFileName, hash]);
        await previewAndPromptEmbeddedSkills({
            items,
            sourceLabel: String(label || character?.name || character?.data?.name || t`Imported character`),
            storageKey,
        });
    } catch (error) {
        reportEmbeddedSkillError(error);
    }
}
