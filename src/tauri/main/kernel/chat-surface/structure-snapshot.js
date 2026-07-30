// @ts-check

/**
 * @typedef {{
 *   keys: readonly string[];
 * }} ChatStructureSnapshot
 */

/** @param {unknown} messages */
function assertMessages(messages) {
    if (!Array.isArray(messages)) {
        throw new TypeError('ChatSurface structure requires a messages array');
    }
}

/**
 * Assigns process-local identities to message objects without changing the
 * SillyTavern chat payload. A factory belongs to exactly one ChatSurface.
 */
export function createStructureSnapshotFactory() {
    let chatEpoch = 0;
    let nextKey = 1;
    /** @type {WeakMap<object, string>} */
    let keyByMessage = new WeakMap();
    /** @type {ChatStructureSnapshot} */
    let current = Object.freeze({
        keys: Object.freeze([]),
    });

    /** @param {object} message */
    function getOrCreateKey(message) {
        const existing = keyByMessage.get(message);
        if (existing) {
            return existing;
        }

        const key = `tt-message-${chatEpoch}-${nextKey}`;
        nextKey += 1;
        keyByMessage.set(message, key);
        return key;
    }

    /** @param {any[]} messages */
    function buildKeys(messages) {
        const keys = messages.map((message, index) => {
            if (!message || typeof message !== 'object') {
                throw new TypeError(`ChatSurface message at index ${index} must be an object`);
            }
            return getOrCreateKey(message);
        });

        if (new Set(keys).size !== keys.length) {
            throw new Error('ChatSurface messages must not contain the same object more than once');
        }

        return keys;
    }

    /** @param {readonly string[]} left @param {readonly string[]} right */
    function keysEqual(left, right) {
        return left.length === right.length && left.every((key, index) => key === right[index]);
    }

    /**
     * Starts a new chat epoch. The epoch only namespaces process-local keys;
     * projection work is synchronous and does not expose a version token.
     *
     * @param {any[]} [messages]
     * @returns {ChatStructureSnapshot}
     */
    function beginEpoch(messages = []) {
        assertMessages(messages);
        chatEpoch += 1;
        nextKey = 1;
        keyByMessage = new WeakMap();
        current = Object.freeze({
            keys: Object.freeze(buildKeys(messages)),
        });
        return current;
    }

    /**
     * Returns the current immutable ordered-key snapshot.
     *
     * @param {any[]} messages
     * @returns {ChatStructureSnapshot}
     */
    function update(messages) {
        assertMessages(messages);
        if (chatEpoch === 0) {
            return beginEpoch(messages);
        }

        const keys = buildKeys(messages);
        if (keysEqual(current.keys, keys)) {
            return current;
        }

        current = Object.freeze({
            keys: Object.freeze(keys),
        });
        return current;
    }

    /** @param {unknown} message */
    function keyOf(message) {
        return message && typeof message === 'object' ? keyByMessage.get(message) ?? null : null;
    }

    return {
        beginEpoch,
        update,
        keyOf,
    };
}
