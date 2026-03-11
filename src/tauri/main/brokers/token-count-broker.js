const DEFAULT_FLUSH_INTERVAL_MS = 25;
const DEFAULT_MAX_BATCH_REQUESTS = 50;

function trimOpenAiMessage(message) {
    if (!message || typeof message !== 'object' || Array.isArray(message)) {
        return message;
    }

    const trimmed = {};

    if (typeof message.role === 'string') {
        trimmed.role = message.role;
    }

    if (Object.prototype.hasOwnProperty.call(message, 'content')) {
        trimmed.content = message.content;
    }

    if (typeof message.name === 'string') {
        trimmed.name = message.name;
    }

    if (Object.prototype.hasOwnProperty.call(message, 'tool_calls')) {
        trimmed.tool_calls = message.tool_calls;
    }

    if (Object.prototype.hasOwnProperty.call(message, 'tool_call_id')) {
        trimmed.tool_call_id = message.tool_call_id;
    }

    if (Object.prototype.hasOwnProperty.call(message, 'function_call')) {
        trimmed.function_call = message.function_call;
    }

    return trimmed;
}

function normalizeModel(model) {
    return String(model || '').trim();
}

function estimateCjkCount(text) {
    const match = String(text || '').match(/[\u3040-\u30ff\u3400-\u4dbf\u4e00-\u9fff\uac00-\ud7af]/g);
    return match ? match.length : 0;
}

export function estimateTokenCount(text) {
    const input = String(text || '');
    if (!input) {
        return 0;
    }

    const cjk = estimateCjkCount(input);
    const other = input.length - cjk;
    return Math.max(0, Math.ceil(cjk + other / 4));
}

export function createTokenCountBroker(options) {
    const context = options?.context;
    if (!context || typeof context.safeInvoke !== 'function') {
        throw new Error('TokenCountBroker requires context.safeInvoke');
    }

    const flushIntervalMs = Number.isFinite(options?.flushIntervalMs)
        ? Math.max(0, Math.floor(options.flushIntervalMs))
        : DEFAULT_FLUSH_INTERVAL_MS;
    const maxBatchRequests = Number.isFinite(options?.maxBatchRequests)
        ? Math.max(1, Math.floor(options.maxBatchRequests))
        : DEFAULT_MAX_BATCH_REQUESTS;

    const states = new Map();

    function getState(model) {
        const key = normalizeModel(model);
        const existing = states.get(key);
        if (existing) {
            return existing;
        }

        const state = {
            model: key,
            queue: [],
            timer: null,
            inFlight: Promise.resolve(),
        };
        states.set(key, state);
        return state;
    }

    function scheduleFlush(state) {
        if (state.timer) {
            return;
        }

        state.timer = setTimeout(() => {
            void flush(state);
        }, flushIntervalMs);
    }

    async function runBatch(state, items) {
        const dto = {
            model: state.model,
            requests: items.map((item) => ({ messages: item.messages })),
        };

        const result = await context.safeInvoke('count_openai_tokens_batch', { dto });
        const tokenCounts = result.token_counts;

        for (let i = 0; i < items.length; i += 1) {
            items[i].resolve(tokenCounts[i]);
        }
    }

    async function flush(state) {
        if (state.timer) {
            clearTimeout(state.timer);
            state.timer = null;
        }

        if (state.queue.length === 0) {
            return;
        }

        const items = state.queue.splice(0, maxBatchRequests);
        if (state.queue.length) {
            scheduleFlush(state);
        }

        state.inFlight = state.inFlight.finally(async () => {
            try {
                await runBatch(state, items);
            } catch (error) {
                for (const item of items) {
                    item.reject(error);
                }
            }
        });

        await state.inFlight;
    }

    return {
        async count({ model, messages }) {
            const state = getState(model);
            const trimmedMessages = messages.map(trimOpenAiMessage);

            const promise = new Promise((resolve, reject) => {
                state.queue.push({ messages: trimmedMessages, resolve, reject });
            });

            if (state.queue.length >= maxBatchRequests) {
                void flush(state);
                return promise;
            }

            scheduleFlush(state);
            return promise;
        },
    };
}

