// @ts-check

import { accountStorage } from '../../util/AccountStorage.js';

export const SKILL_IMPORT_REMINDER_VALUE = '1';

/**
 * @param {unknown[]} parts
 */
export function buildSkillImportReminderKey(parts) {
    const sanitizedParts = parts
        .map(part => String(part || 'unknown').replace(/[^\w.-]+/g, '_').slice(0, 96))
        .filter(Boolean);
    return sanitizedParts.join('_');
}

/**
 * @param {string} key
 */
export function hasSkillImportReminder(key) {
    return accountStorage.getItem(key) === SKILL_IMPORT_REMINDER_VALUE;
}

/**
 * @param {string} key
 */
export function setSkillImportReminder(key) {
    accountStorage.setItem(key, SKILL_IMPORT_REMINDER_VALUE);
}

/**
 * @param {unknown[]} parts
 */
function clearSkillImportReminderPrefix(parts) {
    const prefix = buildSkillImportReminderKey(parts);
    const state = accountStorage.getState();
    for (const key of Object.keys(state)) {
        if (key === prefix || key.startsWith(`${prefix}_`)) {
            accountStorage.removeItem(key);
        }
    }
}

/**
 * @param {unknown} apiId
 * @param {unknown} name
 */
export function clearPresetSkillImportReminders(apiId, name) {
    clearSkillImportReminderPrefix(['AlertSkillPreset', apiId, name]);
}

/**
 * @param {unknown} avatarFileName
 */
export function clearCharacterSkillImportReminders(avatarFileName) {
    clearSkillImportReminderPrefix(['AlertSkillCharacter', avatarFileName]);
}
