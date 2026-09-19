// @ts-check

/** @param {any} message */
function contentVersion(message) {
    return JSON.stringify([
        message.mes, message.swipe_id ?? 0, message.extra?.display_text,
        message.name, message.is_user, message.is_system, message.role,
    ]);
}

/**
 * Display results belong to messages, independently of their DOM residency.
 *
 * @param {{
 *   getMessages: () => any[];
 *   formatMessage: (message: any, messageId: number) => string;
 *   commit: (messageId: number, html: string) => void;
 *   refresh: () => Promise<unknown>;
 *   onFault: (error: Error) => void;
 * }} deps
 */
export function createContentPreparation({ getMessages, formatMessage, commit, refresh, onFault }) {
    /** @type {Array<{ id: string; prepare: (context: any, renderBase: () => Promise<string>) => string | Promise<string> }>} */
    const processors = [];
    /** @type {WeakMap<object, any>} */
    let results = new WeakMap();
    let transientMessages = new WeakSet();
    const pending = new Set();
    let started = false;
    // One queue supports stateful templates; parallelize only with isolated evaluation contexts.
    let queue = Promise.resolve();

    /** @param {any} message */
    function invalidate(message) {
        results.get(message)?.abort.abort();
        results.delete(message);
    }

    /** @param {any} message @param {boolean} transient */
    function setTransient(message, transient) {
        transient ? transientMessages.add(message) : transientMessages.delete(message);
        if (transient && results.get(message)?.html === null) {
            invalidate(message);
        }
    }

    function clearResults() {
        for (const entry of pending) {
            entry.abort.abort();
        }
        results = new WeakMap();
    }

    /** @param {any} definition */
    function register(definition) {
        if (started) {
            throw new Error('ChatSurface content processors must register before the first projection');
        }
        if (typeof definition?.id !== 'string' || !definition.id.trim() || typeof definition.prepare !== 'function') {
            throw new TypeError('ChatSurface content processor requires an id and prepare function');
        }
        if (processors.some(processor => processor.id === definition.id)) {
            throw new Error(`ChatSurface content processor already registered: ${definition.id}`);
        }
        processors.push({ id: definition.id, prepare: definition.prepare });
        return Object.freeze({
            async refresh() {
                clearResults();
                await refresh();
            },
        });
    }

    /** @param {any} message @param {number} messageId @param {string} source */
    function start(message, messageId, source) {
        invalidate(message);
        const entry = { message, messageId, version: contentVersion(message), source, html: /** @type {string | null} */ (null), abort: new AbortController(), promise: queue };
        results.set(message, entry);
        pending.add(entry);
        const current = () => !entry.abort.signal.aborted
            && results.get(message) === entry
            && getMessages()[messageId] === message;

        entry.promise = queue.then(async () => {
            if (!current()) {
                return;
            }
            const context = Object.freeze({ message, mesid: messageId, signal: entry.abort.signal });
            /** @param {number} index @returns {Promise<string>} */
            async function render(index) {
                const processor = processors[index];
                if (!processor) {
                    if (entry.version !== contentVersion(message)) {
                        const base = document.createElement('div');
                        base.innerHTML = formatMessage(message, messageId);
                        entry.source = base.innerHTML;
                        entry.version = contentVersion(message);
                    }
                    return entry.source;
                }
                try {
                    /** @type {Promise<string> | undefined} */
                    let base;
                    const html = await processor.prepare(context, () => base ??= render(index + 1));
                    if (typeof html !== 'string') {
                        throw new TypeError('prepare must return an HTML string');
                    }
                    return html;
                } catch (cause) {
                    const error = /** @type {Error & { cause?: unknown }} */ (
                        new Error(`ChatSurface content processor ${processor.id} failed for message ${messageId}`)
                    );
                    error.cause = cause;
                    throw error;
                }
            }
            const html = await render(0);
            if (current() && entry.version === contentVersion(message)) {
                entry.html = html;
                commit(messageId, html);
            }
        }).catch(error => {
            if (current()) {
                onFault(error);
                throw error;
            }
        }).finally(() => pending.delete(entry));
        // Keep the queue usable after a failed epoch; callers still receive entry.promise's rejection.
        queue = entry.promise.catch(() => {});
        return entry;
    }

    /** @param {{ message: any; messageId: number; content: HTMLElement; transient?: boolean }} input */
    function prepare({ message, messageId, content, transient }) {
        started = true;
        if (transient !== undefined) {
            setTransient(message, transient);
        }
        if (transientMessages.has(message)) {
            content.removeAttribute('aria-busy');
            return false;
        }
        if (processors.length === 0) {
            return true;
        }
        let entry = results.get(message);
        if (!entry || entry.abort.signal.aborted || entry.version !== contentVersion(message) || entry.source !== content.innerHTML
            || (entry.html === null && entry.messageId !== messageId)) {
            entry = start(message, messageId, content.innerHTML);
        }
        if (entry.html !== null) {
            content.innerHTML = entry.html;
            content.removeAttribute('aria-busy');
            return true;
        }
        content.replaceChildren();
        content.setAttribute('aria-busy', 'true');
        return false;
    }

    /** @param {number[]} messageIds */
    async function ready(messageIds) {
        await Promise.all(messageIds.map(messageId => results.get(getMessages()[messageId])?.promise));
    }

    return Object.freeze({
        register, prepare, ready, setTransient,
        /** @param {any[]} messages */
        reconcile(messages) {
            for (const entry of pending) {
                if (messages[entry.messageId] !== entry.message) {
                    entry.abort.abort();
                }
            }
        },
        reset() {
            clearResults();
            transientMessages = new WeakSet();
        },
        isTransient: /** @param {any} message */ message => transientMessages.has(message),
    });
}
