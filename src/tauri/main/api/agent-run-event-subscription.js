// @ts-check

export function createSharedRunEventSubscribe(runId, subscribe) {
    const normalizedRunId = requireRunId(runId);
    const subscribers = new Map();
    let nextSubscriberId = 1;
    let stop = null;

    const reportError = (subscriber, error) => {
        if (typeof subscriber?.onError === 'function') {
            subscriber.onError(error);
            return;
        }
        queueMicrotask(() => {
            throw error;
        });
    };

    const dispatch = (event) => {
        for (const subscriber of Array.from(subscribers.values())) {
            try {
                subscriber.handler(event);
            } catch (error) {
                reportError(subscriber, error);
            }
        }
    };

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
            stop = subscribe(normalizedRunId, dispatch, { onError: dispatchError });
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

function requireRunId(value) {
    const runId = String(value || '').trim();
    if (!runId) {
        throw new Error('runId is required');
    }
    return runId;
}
