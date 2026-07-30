// @ts-check

/**
 * Keeps the mount-key, absolute-index and element views of the committed
 * residency set atomic. Replacing the set validates every index before
 * publishing any of them.
 */
export function createMessageResidencyIndex() {
    /** @type {Map<string, any>} */
    const byMountKey = new Map();
    /** @type {Map<number, any>} */
    const byMessageId = new Map();
    /** @type {WeakMap<HTMLElement, any>} */
    const byElement = new WeakMap();

    /** @param {Iterable<any>} next */
    function replace(next) {
        const nextRecords = [...next];
        const nextByMountKey = new Map();
        const nextByMessageId = new Map();
        const nextElements = new Set();
        for (const record of nextRecords) {
            if (nextByMountKey.has(record.mountKey)) {
                throw new Error(`ChatSurface residency has duplicate mount key: ${record.mountKey}`);
            }
            if (nextByMessageId.has(record.messageId)) {
                throw new Error(`ChatSurface residency has duplicate mesid: ${record.messageId}`);
            }
            if (nextElements.has(record.element)) {
                throw new Error('ChatSurface residency has a duplicate message element');
            }
            nextByMountKey.set(record.mountKey, record);
            nextByMessageId.set(record.messageId, record);
            nextElements.add(record.element);
        }

        for (const record of byMountKey.values()) {
            byElement.delete(record.element);
        }
        byMountKey.clear();
        byMessageId.clear();
        for (const record of nextRecords) {
            byMountKey.set(record.mountKey, record);
            byMessageId.set(record.messageId, record);
            byElement.set(record.element, record);
        }
    }

    /** @param {Iterable<any>} targets */
    function remove(targets) {
        const removed = [...targets];
        for (const record of removed) {
            if (
                byMountKey.get(record.mountKey) !== record
                || byMessageId.get(record.messageId) !== record
                || byElement.get(record.element) !== record
            ) {
                throw new Error('ChatSurface cannot remove an uncommitted residency');
            }
        }
        for (const record of removed) {
            byMountKey.delete(record.mountKey);
            byMessageId.delete(record.messageId);
            byElement.delete(record.element);
        }
    }

    return Object.freeze({
        get size() { return byMountKey.size; },
        getByMountKey: /** @param {string} mountKey */ mountKey => byMountKey.get(mountKey),
        getByMessageId: /** @param {number} messageId */ messageId => byMessageId.get(messageId),
        getByElement: /** @param {HTMLElement} element */ element => byElement.get(element),
        values: () => byMountKey.values(),
        mountKeys: () => byMountKey.keys(),
        messageIds: () => byMessageId.keys(),
        replace,
        remove,
        clear: () => replace([]),
    });
}
