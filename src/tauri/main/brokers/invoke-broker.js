function normalizeCommand(command) {
    return String(command || '').trim();
}

function withTimeout(promise, timeoutMs, createError) {
    const timeout = Math.max(0, Math.floor(Number(timeoutMs) || 0));
    if (!timeout) {
        return promise;
    }

    let timer = null;
    const timeoutPromise = new Promise((_, reject) => {
        timer = setTimeout(() => {
            reject(typeof createError === 'function' ? createError() : new Error('InvokeBroker: timed out'));
        }, timeout);
    });

    return Promise.race([
        Promise.resolve(promise).finally(() => {
            if (timer) {
                clearTimeout(timer);
                timer = null;
            }
        }),
        timeoutPromise,
    ]);
}

function createConcurrencyLimiter(maxConcurrent) {
    const max = Math.max(1, Number(maxConcurrent) || 1);
    let active = 0;
    const queue = [];

    const acquire = () => new Promise((resolve) => {
        if (active < max) {
            active += 1;
            resolve();
            return;
        }
        queue.push(resolve);
    });

    const release = () => {
        active = Math.max(0, active - 1);
        const next = queue.shift();
        if (next) {
            active += 1;
            next();
        }
    };

    const run = async (fn) => {
        await acquire();
        try {
            return await fn();
        } finally {
            release();
        }
    };

    const stats = () => ({
        max,
        active,
        queued: queue.length,
    });

    return { run, stats };
}

function createDedupeState({ key, cacheTtlMs = 0, cacheLimit = 0, maxConcurrent = 0 } = {}, now) {
    const inFlight = new Map();
    const cache = new Map();
    const epochs = new Map();
    const limiter = maxConcurrent > 0 ? createConcurrencyLimiter(maxConcurrent) : null;

    const ttl = Math.max(0, Math.floor(cacheTtlMs || 0));
    const limit = Math.max(0, Math.floor(cacheLimit || 0));

    const keyFn = typeof key === 'function' ? key : () => '';

    const bumpEpoch = (k) => {
        epochs.set(k, (epochs.get(k) || 0) + 1);
    };

    const getEpoch = (k) => epochs.get(k) || 0;

    const readCache = (k) => {
        if (ttl <= 0) {
            return null;
        }
        const entry = cache.get(k);
        if (!entry) {
            return null;
        }
        if (entry.expiresAt <= now()) {
            cache.delete(k);
            return null;
        }
        cache.delete(k);
        cache.set(k, entry);
        return entry.value;
    };

    const writeCache = (k, value, epoch) => {
        if (ttl <= 0) {
            return;
        }
        if (getEpoch(k) !== epoch) {
            return;
        }
        cache.set(k, { value, expiresAt: now() + ttl });
        if (limit > 0 && cache.size > limit) {
            const oldestKey = cache.keys().next().value;
            cache.delete(oldestKey);
        }
    };

    const invalidate = (k) => {
        bumpEpoch(k);
        cache.delete(k);
        inFlight.delete(k);
    };

    const invalidateAll = () => {
        const keys = new Set([...cache.keys(), ...inFlight.keys()]);
        for (const k of keys) {
            bumpEpoch(k);
        }
        cache.clear();
        inFlight.clear();
    };

    return {
        keyFn,
        inFlight,
        readCache,
        writeCache,
        invalidate,
        invalidateAll,
        getEpoch,
        limiter,
    };
}

function createWriteBehindState({ key, delayMs = 0, merge, maxConcurrent = 0 } = {}, now) {
    const limiter = maxConcurrent > 0 ? createConcurrencyLimiter(maxConcurrent) : null;
    const keyFn = typeof key === 'function' ? key : () => '';
    const mergeFn = typeof merge === 'function' ? merge : (_, next) => next;
    const delay = Math.max(0, Math.floor(delayMs || 0));

    const stateByKey = new Map();

    const getKeyState = (k) => {
        const existing = stateByKey.get(k);
        if (existing) {
            return existing;
        }
        const created = {
            timer: null,
            pendingArgs: null,
            waiters: [],
            tail: Promise.resolve(),
        };
        stateByKey.set(k, created);
        return created;
    };

    const scheduleFlush = (key, flush) => {
        const entry = getKeyState(key);
        if (entry.timer) {
            clearTimeout(entry.timer);
        }
        entry.timer = setTimeout(() => {
            entry.timer = null;
            flush(key);
        }, delay);
    };

    const enqueue = (key, args, flush) => {
        const entry = getKeyState(key);
        entry.pendingArgs = entry.pendingArgs ? mergeFn(entry.pendingArgs, args) : args;

        const promise = new Promise((resolve, reject) => {
            entry.waiters.push({ resolve, reject });
        });

        scheduleFlush(key, flush);
        return promise;
    };

    const flush = async (key, transport, command) => {
        const entry = getKeyState(key);
        if (entry.timer) {
            clearTimeout(entry.timer);
            entry.timer = null;
        }
        const args = entry.pendingArgs;
        if (!args) {
            return;
        }

        entry.pendingArgs = null;
        const waiters = entry.waiters;
        entry.waiters = [];

        const run = async () => transport(command, args);
        entry.tail = entry.tail.finally(async () => {
            const task = limiter ? () => limiter.run(run) : run;
            try {
                const result = await task();
                for (const waiter of waiters) {
                    waiter.resolve(result);
                }
            } catch (error) {
                for (const waiter of waiters) {
                    waiter.reject(error);
                }
            }
        });

        await entry.tail;
    };

    const flushAll = async (transport, command) => {
        const keys = [...stateByKey.keys()];
        for (const key of keys) {
            await flush(key, transport, command);
        }
    };

    return {
        keyFn,
        enqueue,
        flush,
        flushAll,
    };
}

/**
 * @typedef {(command: string, args?: any) => Promise<any>} InvokeTransport
 * @typedef {{
 *   kind: 'dedupe';
 *   key?: ((args: any) => string) | undefined;
 *   cacheTtlMs?: number | undefined;
 *   cacheLimit?: number | undefined;
 *   maxConcurrent?: number | undefined;
 *   timeoutMs?: number | undefined;
 * }} DedupePolicy
 * @typedef {{
 *   kind: 'writeBehind';
 *   key?: ((args: any) => string) | undefined;
 *   delayMs?: number | undefined;
 *   merge?: ((previousArgs: any, nextArgs: any) => any) | undefined;
 *   maxConcurrent?: number | undefined;
 *   timeoutMs?: number | undefined;
 * }} WriteBehindPolicy
 * @typedef {DedupePolicy | WriteBehindPolicy} InvokePolicy
 */

/**
 * @param {{
 *   transport: InvokeTransport;
 *   policies?: Record<string, InvokePolicy> | undefined;
 *   now?: (() => number) | undefined;
 * }} options
 */
export function createInvokeBroker({ transport, policies = {}, now = () => Date.now() } = {}) {
    if (typeof transport !== 'function') {
        throw new Error('InvokeBroker requires a transport(command, args) function');
    }

    const policyByCommand = new Map(Object.entries(policies).map(([command, policy]) => [
        normalizeCommand(command),
        policy,
    ]));
    const stateByCommand = new Map();
    const statsByCommand = new Map();

    const getPolicy = (command) => policyByCommand.get(normalizeCommand(command)) || null;

    const getStatsEntry = (command) => {
        const key = normalizeCommand(command);
        const existing = statsByCommand.get(key);
        if (existing) {
            return existing;
        }
        const created = {
            invokes: 0,
            transportInvokes: 0,
            cacheHits: 0,
            dedupeHits: 0,
        };
        statsByCommand.set(key, created);
        return created;
    };

    const transportWithStats = async (command, args) => {
        getStatsEntry(command).transportInvokes += 1;
        return transport(command, args);
    };

    const getState = (command, policy) => {
        const key = normalizeCommand(command);
        const existing = stateByCommand.get(key);
        if (existing) {
            return existing;
        }

        let state;
        if (policy?.kind === 'dedupe') {
            state = createDedupeState(policy, now);
        } else if (policy?.kind === 'writeBehind') {
            state = createWriteBehindState(policy, now);
        } else {
            state = null;
        }

        stateByCommand.set(key, state);
        return state;
    };

    const invoke = async (command, args = {}) => {
        const normalizedCommand = normalizeCommand(command);
        if (!normalizedCommand) {
            throw new Error('InvokeBroker: command is required');
        }

        getStatsEntry(normalizedCommand).invokes += 1;
        const policy = getPolicy(normalizedCommand);
        if (!policy) {
            return transportWithStats(normalizedCommand, args);
        }

        if (policy.kind === 'dedupe') {
            const state = getState(normalizedCommand, policy);
            const key = state.keyFn(args);

            const cached = state.readCache(key);
            if (cached !== null) {
                getStatsEntry(normalizedCommand).cacheHits += 1;
                return cached;
            }

            const inflight = state.inFlight.get(key);
            if (inflight) {
                getStatsEntry(normalizedCommand).dedupeHits += 1;
                return withTimeout(inflight, policy.timeoutMs, () => {
                    const error = new Error(`InvokeBroker timed out: ${normalizedCommand}`);
                    error.name = 'InvokeBrokerTimeoutError';
                    return error;
                });
            }

            const epoch = state.getEpoch(key);
            const run = async () => transportWithStats(normalizedCommand, args);
            const task = state.limiter ? () => state.limiter.run(run) : run;
            const promise = task()
                .then((result) => {
                    state.writeCache(key, result, epoch);
                    return result;
                })
                .finally(() => {
                    state.inFlight.delete(key);
                });

            state.inFlight.set(key, promise);
            return withTimeout(promise, policy.timeoutMs, () => {
                const error = new Error(`InvokeBroker timed out: ${normalizedCommand}`);
                error.name = 'InvokeBrokerTimeoutError';
                return error;
            });
        }

        if (policy.kind === 'writeBehind') {
            const state = getState(normalizedCommand, policy);
            const key = state.keyFn(args);
            const promise = state.enqueue(key, args, (k) => state.flush(k, transportWithStats, normalizedCommand));
            return withTimeout(promise, policy.timeoutMs, () => {
                const error = new Error(`InvokeBroker timed out: ${normalizedCommand}`);
                error.name = 'InvokeBrokerTimeoutError';
                return error;
            });
        }

        return transportWithStats(normalizedCommand, args);
    };

    const invalidate = (command, args = {}) => {
        const normalizedCommand = normalizeCommand(command);
        const policy = getPolicy(normalizedCommand);
        if (!policy || policy.kind !== 'dedupe') {
            return;
        }
        const state = getState(normalizedCommand, policy);
        const key = state.keyFn(args);
        state.invalidate(key);
    };

    const invalidateAll = (command) => {
        const normalizedCommand = normalizeCommand(command);
        const policy = getPolicy(normalizedCommand);
        if (!policy || policy.kind !== 'dedupe') {
            return;
        }
        const state = getState(normalizedCommand, policy);
        state.invalidateAll();
    };

    const flush = async (command) => {
        const normalizedCommand = normalizeCommand(command);
        const policy = getPolicy(normalizedCommand);
        if (!policy || policy.kind !== 'writeBehind') {
            return;
        }

        const state = getState(normalizedCommand, policy);
        await state.flushAll(transportWithStats, normalizedCommand);
    };

    const flushAll = async () => {
        const tasks = [];
        for (const [command, policy] of policyByCommand.entries()) {
            if (policy?.kind !== 'writeBehind') {
                continue;
            }
            tasks.push(flush(command));
        }
        await Promise.all(tasks);
    };

    const getStats = () => Object.fromEntries([...statsByCommand.entries()].map(([key, entry]) => [
        key,
        { ...entry },
    ]));

    return {
        invoke,
        invalidate,
        invalidateAll,
        flush,
        flushAll,
        getStats,
    };
}
