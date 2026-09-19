// @ts-check

export const AGENT_MODEL_REQUIRES_CONFIGURATION = 'requiresConfiguration';
const AGENT_PROFILE_PACKAGE_VERSION = 1;

/**
 * @typedef {Record<string, unknown> & { profile: Record<string, unknown> }} PortableAgentProfilePackageItem
 * @typedef {Record<string, unknown> & {
 *   version: 1;
 *   items: PortableAgentProfilePackageItem[];
 * }} PortableAgentProfilePackage
 */

/**
 * @template T
 * @param {T} value
 * @returns {T}
 */
function clone(value) {
    return JSON.parse(JSON.stringify(value));
}

/**
 * Removes local-only model connection bindings from a profile intended for sharing.
 *
 * @param {Record<string, unknown>} profile
 * @returns {Record<string, unknown>}
 */
export function sanitizePortableAgentProfile(profile) {
    const sanitized = clone(profile);
    const model = sanitized.model;
    const modelRecord = model && typeof model === 'object' && !Array.isArray(model)
        ? /** @type {Record<string, unknown>} */ (model)
        : null;
    if (modelRecord?.mode === 'connectionRef') {
        sanitized.model = {
            mode: AGENT_MODEL_REQUIRES_CONFIGURATION,
        };
    }
    return sanitized;
}

/**
 * @param {unknown} item
 * @returns {PortableAgentProfilePackageItem}
 */
function sanitizePortableAgentProfilePackageItem(item) {
    const itemRecord = requirePlainObject(item, 'Embedded Agent Profile item');
    const profile = requirePlainObject(itemRecord.profile, 'Embedded Agent Profile item.profile');
    return {
        ...itemRecord,
        profile: sanitizePortableAgentProfile(profile),
    };
}

/**
 * Removes local-only model connection bindings from every profile in an
 * embedded Agent Profile package.
 *
 * @param {unknown} packageValue
 * @returns {PortableAgentProfilePackage}
 */
export function sanitizePortableAgentProfilePackage(packageValue) {
    const record = requirePlainObject(clone(packageValue), 'Embedded Agent Profile package');
    if (Number(record.version) !== AGENT_PROFILE_PACKAGE_VERSION) {
        throw new Error(`Unsupported embedded Agent Profile schema version: ${record.version}`);
    }
    if (!Array.isArray(record.items)) {
        throw new Error('Embedded Agent Profile package items must be an array');
    }
    return {
        ...record,
        version: AGENT_PROFILE_PACKAGE_VERSION,
        items: record.items.map(sanitizePortableAgentProfilePackageItem),
    };
}

/**
 * @param {unknown} value
 * @param {string} label
 * @returns {Record<string, unknown>}
 */
function requirePlainObject(value, label) {
    if (!value || typeof value !== 'object' || Array.isArray(value)) {
        throw new Error(`${label} must be an object`);
    }
    return /** @type {Record<string, unknown>} */ (value);
}
