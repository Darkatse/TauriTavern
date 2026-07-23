// @ts-check

/** @typedef {{ messageId: number; element: HTMLElement }} DomProjectionEntry */

/** @param {HTMLElement} root */
function directMessages(root) {
    return /** @type {HTMLElement[]} */ ([...root.querySelectorAll(':scope > .mes')]);
}

/**
 * Owns only direct `.mes` children. Auxiliary children such as style pins,
 * welcome content and the static Show More control keep their existing owners.
 *
 * The mutation guard remains opt-in because unmodified ecosystem plugins still
 * write message roots. Production enables it only after the matching participant
 * adapters are active.
 *
 * @param {{
 *   root: HTMLElement;
 *   onUnauthorizedMutation: (error: Error) => void;
 *   onExternalRemoval?: (elements: HTMLElement[]) => void;
 *   guardUnauthorizedMutations?: boolean;
 * }} deps
 */
export function createChatDomAdapter({
    root,
    onUnauthorizedMutation,
    onExternalRemoval,
    guardUnauthorizedMutations = false,
}) {
    if (!(root instanceof HTMLElement)) {
        throw new TypeError('ChatSurface root must be an HTMLElement');
    }
    if (typeof onUnauthorizedMutation !== 'function') {
        throw new TypeError('ChatSurface onUnauthorizedMutation must be a function');
    }
    if (onExternalRemoval !== undefined && typeof onExternalRemoval !== 'function') {
        throw new TypeError('ChatSurface onExternalRemoval must be a function');
    }

    /** @type {DomProjectionEntry[]} */
    let committed = [];
    let mutationGuardEnabled = guardUnauthorizedMutations;
    /** @type {HTMLElement | null} */
    let topSpacer = null;
    /** @type {HTMLElement | null} */
    let middleSpacer = null;
    let bounded = false;

    /** @param {'top' | 'middle'} kind */
    function requireSpacer(kind) {
        const current = kind === 'top' ? topSpacer : middleSpacer;
        if (current) {
            return current;
        }
        const element = document.createElement('tt-chat-spacer');
        element.setAttribute('data-tt-chat-spacer', kind);
        element.setAttribute('aria-hidden', 'true');
        if (kind === 'top') {
            topSpacer = element;
        } else {
            middleSpacer = element;
        }
        return element;
    }

    const observer = new MutationObserver((records) => {
        const projectionChanged = records.some(record => (
            record.type === 'attributes'
            && record.target instanceof HTMLElement
            && record.target.parentElement === root
            && record.target.matches('.mes')
        ) || (
            record.target === root
            && [...(record.addedNodes ?? []), ...(record.removedNodes ?? [])]
                .some(node => node instanceof HTMLElement && node.matches('.mes'))
        ));
        if (mutationGuardEnabled) {
            if (!projectionChanged) {
                return;
            }
            try {
                assertCommitted(committed);
            } catch (error) {
                onUnauthorizedMutation(error instanceof Error ? error : new Error(String(error)));
            }
            return;
        }

        /** @type {HTMLElement[]} */
        const externalRemovals = [];
        for (const record of records) {
            if (record.target !== root) {
                continue;
            }
            for (const node of record.removedNodes ?? []) {
                if (node instanceof HTMLElement && node.matches('.mes')) {
                    externalRemovals.push(node);
                }
            }
        }
        if (externalRemovals.length > 0) {
            const removed = new Set(externalRemovals);
            committed = committed.filter(entry => !removed.has(entry.element));
            onExternalRemoval?.(externalRemovals);
        }
    });

    function observe() {
        observer.observe(root, mutationGuardEnabled
            ? { childList: true, attributes: true, attributeFilter: ['mesid'], subtree: true }
            : { childList: true });
    }
    observe();

    /** @param {DomProjectionEntry[]} entries */
    function remove(entries) {
        for (const { element } of entries) {
            if (element.parentElement !== root) {
                continue;
            }
            element.remove();
        }
    }

    /** @param {HTMLElement} element @param {number} messageId */
    function setMessageId(element, messageId) {
        element.setAttribute('mesid', String(messageId));
        const display = element.querySelector('.mesIDDisplay');
        if (display instanceof HTMLElement) {
            display.textContent = `#${messageId}`;
        }
    }

    /** @param {HTMLElement} element */
    function nextDirectMessage(element) {
        let sibling = element.nextElementSibling;
        while (sibling) {
            if (sibling.matches('.mes')) {
                return sibling;
            }
            sibling = sibling.nextElementSibling;
        }
        return null;
    }

    /** @param {DomProjectionEntry[]} desired */
    function reorderRetained(desired) {
        /** @type {HTMLElement | null} */
        let next = null;
        for (let index = desired.length - 1; index >= 0; index -= 1) {
            const entry = desired[index];
            if (!entry) {
                throw new Error(`ChatSurface DOM projection is missing entry ${index}`);
            }
            const element = entry.element;
            if (element.parentElement !== root) {
                continue;
            }
            if (nextDirectMessage(element) !== next) {
                if (next) {
                    next.before(element);
                } else {
                    root.append(element);
                }
            }
            next = element;
        }
    }

    /** @param {DomProjectionEntry[]} desired */
    function insertDetachedRuns(desired) {
        let cursor = 0;
        while (cursor < desired.length) {
            const entry = desired[cursor];
            if (!entry) {
                throw new Error(`ChatSurface DOM projection is missing entry ${cursor}`);
            }
            if (entry.element.parentElement === root) {
                cursor += 1;
                continue;
            }

            /** @type {HTMLElement[]} */
            const run = [];
            while (cursor < desired.length) {
                const candidate = desired[cursor];
                if (!candidate || candidate.element.parentElement === root) {
                    break;
                }
                run.push(candidate.element);
                cursor += 1;
            }

            const nextEntry = desired[cursor];
            const nextRetained = nextEntry?.element.parentElement === root
                ? nextEntry.element
                : null;
            if (nextRetained) {
                nextRetained.before(...run);
            } else {
                root.append(...run);
            }
        }
    }

    /** @param {{ removed: DomProjectionEntry[]; desired: DomProjectionEntry[] }} change */
    function validateCommit({ removed, desired }) {
        const desiredElements = desired.map(entry => entry.element);
        const removedElements = removed.map(entry => entry.element);
        const desiredSet = new Set(desiredElements);
        if (desiredSet.size !== desiredElements.length) {
            throw new Error('ChatSurface DOM projection contains a duplicate element');
        }
        if (new Set(desired.map(entry => entry.messageId)).size !== desired.length) {
            throw new Error('ChatSurface DOM projection contains a duplicate mesid');
        }
        if (removedElements.some(element => desiredSet.has(element))) {
            throw new Error('ChatSurface DOM projection removes and retains the same element');
        }
        for (const element of removedElements) {
            if (element.parentElement !== root) {
                throw new Error('ChatSurface cannot remove a message outside its root');
            }
        }
        for (const element of desiredElements) {
            if (element.isConnected && element.parentElement !== root) {
                throw new Error('ChatSurface desired message is connected outside its root');
            }
        }

        const expectedCurrent = new Set([
            ...removedElements,
            ...desiredElements.filter(element => element.parentElement === root),
        ]);
        const current = directMessages(root);
        if (current.length !== expectedCurrent.size || current.some(element => !expectedCurrent.has(element))) {
            throw new Error('ChatSurface DOM root diverged from the committed residency set');
        }
    }

    /** @param {DomProjectionEntry[]} desired */
    function assertCommitted(desired) {
        const current = directMessages(root);
        if (
            current.length !== desired.length
            || desired.some((entry, index) => current[index] !== entry.element)
            || desired.some(entry => entry.element.getAttribute('mesid') !== String(entry.messageId))
        ) {
            throw new Error('ChatSurface committed DOM projection is inconsistent');
        }
    }

    /**
     * Commits the complete message projection while leaving auxiliary children
     * untouched. Walking backwards keeps already-correct retained runs stable.
     *
     * @param {{ removed: DomProjectionEntry[]; desired: DomProjectionEntry[] }} change
     */
    function commit({ removed, desired }) {
        validateCommit({ removed, desired });

        remove(removed);
        for (const entry of desired) {
            setMessageId(entry.element, entry.messageId);
        }

        reorderRetained(desired);
        insertDetachedRuns(desired);
        committed = desired.slice();
    }

    /** @param {number} finalMessageId */
    function syncLastMessage(finalMessageId) {
        for (const element of directMessages(root)) {
            element.classList.toggle('last_mes', element.getAttribute('mesid') === String(finalMessageId));
        }
    }

    /** @param {Iterable<HTMLElement>} elements */
    function discard(elements) {
        const discarded = new Set(elements);
        for (const element of discarded) {
            element.remove();
        }
        committed = committed.filter(entry => !discarded.has(entry.element));
    }

    function clearMessages() {
        discard(directMessages(root));
    }

    function enableBoundedLayout() {
        if (bounded) {
            return;
        }
        if (root.querySelector('#show_more_messages')) {
            throw new Error('Bounded ChatSurface cannot adopt the static Show More control');
        }
        bounded = true;
        root.setAttribute('data-tt-chat-surface', 'bounded');
    }

    /** @param {any} layout */
    function commitBoundedLayout(layout) {
        if (!bounded) {
            throw new Error('ChatSurface bounded layout is not enabled');
        }
        if (!layout?.projection || !Array.isArray(layout.projection.indices)) {
            throw new TypeError('ChatSurface bounded layout is malformed');
        }
        const messages = directMessages(root);
        const byMessageId = new Map(messages.map(element => [
            Number(element.getAttribute('mesid')),
            element,
        ]));
        if (
            messages.length !== layout.projection.indices.length
            || layout.projection.indices.some(/** @param {number} messageId */ messageId => !byMessageId.has(messageId))
        ) {
            throw new Error('ChatSurface bounded layout and committed messages diverged');
        }
        for (const child of root.children) {
            if (
                child.matches('.mes')
                || child.matches('tt-chat-spacer[data-tt-chat-spacer]')
                || child.matches('.style-pins')
            ) {
                continue;
            }
            throw new Error(`Bounded ChatSurface contains an unknown direct child: ${child.tagName.toLowerCase()}`);
        }

        for (const messageId of layout.projection.indices) {
            const element = byMessageId.get(messageId);
            if (!element) {
                throw new Error(`ChatSurface bounded layout is missing message ${messageId}`);
            }
            element.setAttribute('data-tt-virtual-index', String(messageId));
        }

        const firstMessage = byMessageId.get(layout.projection.indices[0]);
        /** @param {'top' | 'middle'} kind @param {{ present: boolean; height: number }} definition @param {HTMLElement | null} before */
        const commitSpacer = (kind, definition, before) => {
            const existing = kind === 'top' ? topSpacer : middleSpacer;
            if (!definition.present) {
                existing?.remove();
                return;
            }
            const element = requireSpacer(kind);
            element.style.height = `${definition.height}px`;
            root.insertBefore(element, before);
        };
        commitSpacer('top', layout.topSpacer, firstMessage ?? null);

        const tail = layout.tailMessageId === null ? null : byMessageId.get(layout.tailMessageId);
        commitSpacer('middle', layout.middleSpacer, tail ?? null);
        assertBoundedLayout(layout);
    }

    /** @param {any} layout */
    function assertBoundedLayout(layout) {
        const flow = [...root.children].filter(child => !child.matches('.style-pins'));
        const expected = [];
        if (layout.topSpacer.present) {
            expected.push(topSpacer);
        }
        const viewportIds = new Set(layout.viewportMessageIds);
        for (const messageId of layout.projection.indices) {
            if (layout.middleSpacer.present && messageId === layout.tailMessageId && !viewportIds.has(messageId)) {
                expected.push(middleSpacer);
            }
            expected.push(root.querySelector(`:scope > .mes[mesid="${messageId}"]`));
        }
        if (
            flow.length !== expected.length
            || expected.some((element, index) => element === null || flow[index] !== element)
        ) {
            /** @param {Element | null | undefined} element */
            const describe = element => element
                ? `${element.tagName.toLowerCase()}:${element.getAttribute('mesid') ?? element.getAttribute('data-tt-chat-spacer') ?? ''}`
                : 'null';
            throw new Error(
                `ChatSurface bounded DOM order is inconsistent; actual=[${flow.map(describe).join(',')}], `
                + `expected=[${expected.map(describe).join(',')}]`,
            );
        }
        if (topSpacer && topSpacer.isConnected !== layout.topSpacer.present) {
            throw new Error('ChatSurface top spacer presence diverged');
        }
        if (middleSpacer && middleSpacer.isConnected !== layout.middleSpacer.present) {
            throw new Error('ChatSurface middle spacer presence diverged');
        }
    }

    function disableBoundedLayout() {
        topSpacer?.remove();
        middleSpacer?.remove();
        topSpacer = null;
        middleSpacer = null;
        bounded = false;
        root.removeAttribute('data-tt-chat-surface');
        for (const element of directMessages(root)) {
            element.removeAttribute('data-tt-virtual-index');
        }
    }

    function clearAll() {
        clearMessages();
        disableBoundedLayout();
        root.replaceChildren();
    }

    return Object.freeze({
        root,
        validateCommit,
        assertCommitted,
        commit,
        clearMessages,
        clearAll,
        discard,
        directMessages: () => directMessages(root),
        syncLastMessage,
        enableBoundedLayout,
        commitBoundedLayout,
        disableBoundedLayout,
        isMutationGuardEnabled: () => mutationGuardEnabled,
        /** @param {boolean} enabled */
        setMutationGuardEnabled(enabled) {
            mutationGuardEnabled = Boolean(enabled);
            observer.disconnect();
            if (mutationGuardEnabled) {
                assertCommitted(committed);
            }
            observe();
        },
        dispose: () => observer.disconnect(),
    });
}
