// @ts-check

export const AGENT_MODEL_REQUIRES_CONFIGURATION = 'requiresConfiguration';
export const AGENT_PROFILE_PACKAGE_VERSION = 1;

function clone(value) {
    return JSON.parse(JSON.stringify(value));
}

/**
 * Removes local-only model connection bindings from a profile intended for sharing.
 *
 * @param {Record<string, any>} profile
 * @returns {Record<string, any>}
 */
export function sanitizePortableAgentProfile(profile) {
    const sanitized = clone(profile);
    if (sanitized?.model?.mode === 'connectionRef') {
        sanitized.model = {
            mode: AGENT_MODEL_REQUIRES_CONFIGURATION,
        };
    }
    return sanitized;
}

/**
 * Removes local-only model connection bindings from every profile in an
 * embedded Agent Profile package.
 *
 * @param {Record<string, any>} packageValue
 * @returns {Record<string, any>}
 */
export function sanitizePortableAgentProfilePackage(packageValue) {
    const sanitized = clone(packageValue);
    if (!sanitized || typeof sanitized !== 'object' || Array.isArray(sanitized)) {
        throw new Error('Embedded Agent Profile package must be an object');
    }
    if (Number(sanitized.version) !== AGENT_PROFILE_PACKAGE_VERSION) {
        throw new Error(`Unsupported embedded Agent Profile schema version: ${sanitized.version}`);
    }
    if (!Array.isArray(sanitized.items)) {
        throw new Error('Embedded Agent Profile package items must be an array');
    }
    sanitized.items = sanitized.items.map((item) => {
        if (!item || typeof item !== 'object' || Array.isArray(item)) {
            throw new Error('Embedded Agent Profile item must be an object');
        }
        if (!item.profile || typeof item.profile !== 'object' || Array.isArray(item.profile)) {
            throw new Error('Embedded Agent Profile item.profile must be an object');
        }
        return {
            ...item,
            profile: sanitizePortableAgentProfile(item.profile),
        };
    });
    return sanitized;
}
