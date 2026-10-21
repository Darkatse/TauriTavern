// @ts-check

/**
 * @param {string} runId
 * @param {TauriTavernAgentApi['subscribe']} subscribe
 * @param {number} [afterSeq]
 * @returns {TauriTavernAgentApi['subscribe']}
 */
export function createSharedRunEventSubscribe(runId, subscribe, afterSeq = 0) {
    const normalizedRunId = requireRunId(runId);
    /** @typedef {{ handler: (event: TauriTavernAgentRunEvent) => void; onError: ((error: unknown) => void) | undefined }} Subscriber */
    /** @type {Map<number, Subscriber>} */
    const subscribers = new Map();
    let nextSubscriberId = 1;
    /** @type {(() => void) | null} */
    let stop = null;

    /** @param {Subscriber} subscriber @param {unknown} error */
    const reportError = (subscriber, error) => {
        if (typeof subscriber?.onError === 'function') {
            subscriber.onError(error);
            return;
        }
        queueMicrotask(() => {
            throw error;
        });
    };

    /** @param {TauriTavernAgentRunEvent} event */
    const dispatch = (event) => {
        for (const subscriber of Array.from(subscribers.values())) {
            try {
                subscriber.handler(event);
            } catch (error) {
                reportError(subscriber, error);
            }
        }
    };

    /** @param {unknown} error */
    const dispatchError = (error) => {
        for (const subscriber of Array.from(subscribers.values())) {
            reportError(subscriber, error);
        }
    };

    return function sharedSubscribe(inputRunId, handler, options = {}) {
        if (requireRunId(inputRunId) !== normalizedRunId) {
            throw new Error(
                'agent.subscribe_run_mismatch: shared run event subscription received another runId',
            );
        }
        if (typeof handler !== 'function') {
            throw new Error('handler is required');
        }

        const subscriberId = nextSubscriberId++;
        subscribers.set(subscriberId, {
            handler,
            onError: options?.onError,
        });
        if (!stop) {
            stop = subscribe(normalizedRunId, dispatch, { afterSeq, onError: dispatchError });
        }

        return function unsubscribe() {
            subscribers.delete(subscriberId);
            if (subscribers.size === 0 && typeof stop === 'function') {
                stop();
                stop = null;
            }
        };
    };
}

/** @param {unknown} value */
function requireRunId(value) {
    const runId = String(value || '').trim();
    if (!runId) {
        throw new Error('runId is required');
    }
    return runId;
}
