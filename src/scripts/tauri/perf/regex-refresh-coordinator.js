// @ts-check

import { addCopyToCodeBlocks, chat, messageFormatting } from '../../../script.js';
import { replaceMesTextHtmlWithRuntimePolicy } from '../message/mes-text-write.js';

let singleton = null;

export function getRegexRefreshCoordinator() {
    if (singleton) {
        return singleton;
    }

    singleton = createRegexRefreshCoordinator();
    return singleton;
}

function createRegexRefreshCoordinator() {
    const DEFAULT_DEBOUNCE_MS = 500;

    /** @type {ReturnType<typeof setTimeout> | null} */
    let debounceTimeoutId = null;

    /** @type {{ resolve: () => void }[]} */
    const waiters = [];

    /** @type {{ messageId: number; message: any; element: HTMLElement }[]} */
    let queue = [];
    let queueIndex = 0;

    let scheduled = false;
    let running = false;
    let cycleRequested = false;

    function requestFlush({ debounceMs = DEFAULT_DEBOUNCE_MS } = {}) {
        if (debounceTimeoutId) {
            clearTimeout(debounceTimeoutId);
        }

        debounceTimeoutId = setTimeout(() => {
            debounceTimeoutId = null;
            triggerFlush();
        }, debounceMs);
    }

    function flushNow() {
        if (debounceTimeoutId) {
            clearTimeout(debounceTimeoutId);
            debounceTimeoutId = null;
        }

        triggerFlush();

        return new Promise((resolve) => {
            waiters.push({ resolve });
        });
    }

    function triggerFlush() {
        cycleRequested = true;
        schedule();
    }

    function schedule() {
        if (scheduled) {
            return;
        }

        if (!running && !cycleRequested) {
            return;
        }

        scheduled = true;

        if (typeof requestIdleCallback === 'function') {
            requestIdleCallback(run, { timeout: 1000 });
            return;
        }

        requestAnimationFrame(() => run(null));
    }

    function collectQueue() {
        /** @type {{ messageId: number; message: any; element: HTMLElement }[]} */
        const next = [];

        for (const node of document.querySelectorAll('#chat .mes[mesid]')) {
            if (!(node instanceof HTMLElement)) {
                continue;
            }

            const rawId = node.getAttribute('mesid');
            const messageId = Number(rawId);
            if (!Number.isFinite(messageId)) {
                throw new Error(`RegexRefreshCoordinator: invalid mesid '${rawId}'`);
            }

            const message = chat[messageId];
            if (!message) {
                throw new Error(`RegexRefreshCoordinator: missing chat message for id ${messageId}`);
            }

            next.push({ messageId, message, element: node });
        }

        return next;
    }

    /**
     * @param {{ messageId: number; message: any; element: HTMLElement }} entry
     */
    function refreshMessage(entry) {
        const message = entry.message;
        const text = message?.extra?.display_text ?? message.mes;
        replaceMesTextHtmlWithRuntimePolicy(
            entry.element,
            messageFormatting(text, message.name, message.is_system, message.is_user, entry.messageId, {}, false),
        );
        addCopyToCodeBlocks(entry.element);
    }

    /**
     * @param {IdleDeadline | null} deadline
     */
    function run(deadline) {
        scheduled = false;

        if (!running) {
            if (!cycleRequested) {
                resolveWaiters();
                return;
            }

            cycleRequested = false;
            running = true;
            queue = collectQueue();
            queueIndex = 0;
        }

        if (queue.length === 0) {
            finishCycle();
            if (cycleRequested) {
                schedule();
                return;
            }
            resolveWaiters();
            return;
        }

        const start = performance.now();
        const budgetMs = 8;

        while (queueIndex < queue.length) {
            const entry = queue[queueIndex];
            queueIndex += 1;

            refreshMessage(entry);

            if (deadline && typeof deadline.timeRemaining === 'function' && deadline.timeRemaining() < 1) {
                break;
            }

            if (performance.now() - start > budgetMs) {
                break;
            }
        }

        if (queueIndex < queue.length) {
            schedule();
            return;
        }

        finishCycle();
        if (cycleRequested) {
            schedule();
            return;
        }
        resolveWaiters();
    }

    function finishCycle() {
        running = false;
        queue = [];
        queueIndex = 0;
    }

    function resolveWaiters() {
        if (running || cycleRequested) {
            return;
        }

        if (waiters.length === 0) {
            return;
        }

        const toResolve = waiters.splice(0, waiters.length);
        for (const waiter of toResolve) {
            waiter.resolve();
        }
    }

    return {
        requestFlush,
        flushNow,
    };
}
