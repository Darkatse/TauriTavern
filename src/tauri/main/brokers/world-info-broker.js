const DEFAULT_FLUSH_INTERVAL_MS = 5;
const DEFAULT_MAX_BATCH_NAMES = 50;

function normalizeName(name) {
    return String(name || '').trim();
}

export function createWorldInfoBroker(options) {
    const context = options?.context;
    if (!context || typeof context.safeInvoke !== 'function') {
        throw new Error('WorldInfoBroker requires context.safeInvoke');
    }

    const flushIntervalMs = Number.isFinite(options?.flushIntervalMs)
        ? Math.max(0, Math.floor(options.flushIntervalMs))
        : DEFAULT_FLUSH_INTERVAL_MS;
    const maxBatchNames = Number.isFinite(options?.maxBatchNames)
        ? Math.max(1, Math.floor(options.maxBatchNames))
        : DEFAULT_MAX_BATCH_NAMES;

    const inFlightByName = new Map();
    const queue = [];
    let timer = null;
    let tail = Promise.resolve();

    function scheduleFlush() {
        if (timer) {
            return;
        }

        timer = setTimeout(() => {
            void flush();
        }, flushIntervalMs);
    }

    async function runBatch(names) {
        const dto = { names };
        const result = await context.safeInvoke('get_world_infos_batch', { dto });
        const items = Array.isArray(result?.items) ? result.items : [];

        const dataByName = new Map();
        for (const item of items) {
            const name = normalizeName(item?.name);
            if (!name) {
                continue;
            }
            dataByName.set(name, item?.data);
        }

        for (const name of names) {
            const entry = inFlightByName.get(name);
            if (!entry) {
                continue;
            }

            if (!dataByName.has(name)) {
                throw new Error(`WorldInfoBroker missing response item: ${name}`);
            }

            entry.resolve(dataByName.get(name));
            inFlightByName.delete(name);
        }
    }

    async function flush() {
        if (timer) {
            clearTimeout(timer);
            timer = null;
        }

        if (queue.length === 0) {
            return;
        }

        const names = queue.splice(0, maxBatchNames);
        if (queue.length) {
            scheduleFlush();
        }

        tail = tail.finally(async () => {
            try {
                await runBatch(names);
            } catch (error) {
                for (const name of names) {
                    const entry = inFlightByName.get(name);
                    if (!entry) {
                        continue;
                    }

                    entry.reject(error);
                    inFlightByName.delete(name);
                }
            }
        });

        await tail;
    }

    function get(name) {
        const key = normalizeName(name);
        if (!key) {
            throw new Error('WorldInfoBroker name is required');
        }

        const existing = inFlightByName.get(key);
        if (existing) {
            return existing.promise;
        }

        let resolve;
        let reject;
        const promise = new Promise((res, rej) => {
            resolve = res;
            reject = rej;
        });

        inFlightByName.set(key, { promise, resolve, reject });
        queue.push(key);

        if (queue.length >= maxBatchNames) {
            void flush();
            return promise;
        }

        scheduleFlush();
        return promise;
    }

    return {
        get,
    };
}

