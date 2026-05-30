// @ts-check

/** @param {any} value */
function isPlainObject(value) {
    return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}

/**
 * @param {any} value
 * @returns {any}
 */
function cloneStructuredValue(value) {
    if (Array.isArray(value)) {
        return value.map(cloneStructuredValue);
    }

    if (isPlainObject(value)) {
        return mergeObjects({}, value);
    }

    return value;
}

/**
 * @param {Record<string, any>} target
 * @param  {...any} sources
 * @returns {Record<string, any>}
 */
export function mergeObjects(target, ...sources) {
    for (const source of sources) {
        if (!isPlainObject(source)) {
            continue;
        }

        for (const [key, value] of Object.entries(source)) {
            if (isPlainObject(value) && isPlainObject(target[key])) {
                target[key] = mergeObjects({ ...target[key] }, value);
            } else {
                target[key] = cloneStructuredValue(value);
            }
        }
    }

    return target;
}

/**
 * @param {string} path
 * @returns {string[]}
 */
function pathParts(path) {
    return String(path || '').split('.').filter(Boolean);
}

/**
 * @param {Record<string, any>} target
 * @param {string} path
 * @param {any} value
 */
export function setByPath(target, path, value) {
    const parts = pathParts(path);
    if (parts.length === 0) {
        return;
    }

    let cursor = target;
    for (const part of parts.slice(0, -1)) {
        if (!isPlainObject(cursor[part])) {
            cursor[part] = {};
        }
        cursor = cursor[part];
    }

    const lastPart = parts[parts.length - 1];
    if (lastPart === undefined) {
        return;
    }

    cursor[lastPart] = value;
}

/**
 * @param {Record<string, any>} target
 * @param {string} path
 */
export function unsetByPath(target, path) {
    const parts = pathParts(path);
    if (parts.length === 0) {
        return;
    }

    let cursor = target;
    for (const part of parts.slice(0, -1)) {
        if (!isPlainObject(cursor[part])) {
            return;
        }
        cursor = cursor[part];
    }

    const lastPart = parts[parts.length - 1];
    if (lastPart === undefined) {
        return;
    }

    delete cursor[lastPart];
}

/**
 * @param {Record<string, any>} target
 * @param {string} path
 * @param {any} fallback
 * @returns {any}
 */
export function getByPath(target, path, fallback) {
    let cursor = target;
    for (const part of pathParts(path)) {
        if (!isPlainObject(cursor) || !Object.prototype.hasOwnProperty.call(cursor, part)) {
            return fallback;
        }
        cursor = cursor[part];
    }
    return cursor;
}
