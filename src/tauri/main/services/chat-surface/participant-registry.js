// @ts-check

export const CHAT_SURFACE_PROTOCOL_VERSION = 1;

const HOOK_NAMES = Object.freeze([
    'prepareContent',
    'didMount',
    'didCommitContent',
]);

const PARTICIPANT_KEYS = new Set(['id', 'protocolVersion', ...HOOK_NAMES]);

/** @param {unknown} value */
function toError(value) {
    return value instanceof Error ? value : new Error(String(value));
}

/** @param {any} input */
export function normalizeChatSurfaceParticipant(input) {
    if (!input || typeof input !== 'object') {
        throw new TypeError('ChatSurface participant must be an object');
    }

    for (const key of Object.keys(input)) {
        if (!PARTICIPANT_KEYS.has(key)) {
            throw new Error(`Unknown ChatSurface participant field: ${key}`);
        }
    }

    const id = String(input.id || '').trim();
    if (!id) {
        throw new Error('ChatSurface participant id is required');
    }

    const protocolVersion = input.protocolVersion;
    if (protocolVersion !== CHAT_SURFACE_PROTOCOL_VERSION) {
        throw new Error(`Unsupported ChatSurface participant protocol: ${String(protocolVersion)}`);
    }

    let hookCount = 0;
    /** @type {Record<string, Function | undefined>} */
    const hooks = {};
    for (const hookName of HOOK_NAMES) {
        const hook = input[hookName];
        if (hook !== undefined && typeof hook !== 'function') {
            throw new TypeError(`ChatSurface participant ${id}.${hookName} must be a function`);
        }
        if (hook) {
            hookCount += 1;
        }
        hooks[hookName] = hook;
    }

    if (hookCount === 0) {
        throw new Error(`ChatSurface participant ${id} must implement at least one hook`);
    }

    return Object.freeze({
        id,
        protocolVersion,
        ...hooks,
    });
}

export function createChatSurfaceParticipantRegistry() {
    /** @type {Map<string, ReturnType<typeof normalizeChatSurfaceParticipant>>} */
    const participants = new Map();
    let frozen = false;
    /** @type {Error | null} */
    let fault = null;
    /** @type {((error: unknown) => unknown) | null} */
    let onFault = null;

    /** @param {any} definition */
    function register(definition) {
        if (frozen) {
            throw new Error('ChatSurface participants must register before the first projection');
        }
        const participant = normalizeChatSurfaceParticipant(definition);
        if (participants.has(participant.id)) {
            throw new Error(`ChatSurface participant already registered: ${participant.id}`);
        }

        participants.set(participant.id, participant);

        /** @param {unknown} error */
        const reportFault = (error) => {
            if (fault) {
                return;
            }
            const failure = /** @type {Error & { cause?: unknown }} */ (
                new Error(`ChatSurface participant ${participant.id} is faulted`)
            );
            failure.cause = toError(error);
            fault = failure;
            onFault?.(failure);
        };

        return Object.freeze({ fault: reportFault });
    }

    /** @param {(error: unknown) => unknown} faultHandler */
    function freeze(faultHandler) {
        if (typeof faultHandler !== 'function') {
            throw new TypeError('ChatSurface participant registry requires a fault handler');
        }
        if (!frozen) {
            frozen = true;
            onFault = faultHandler;
        }
        if (fault) {
            throw fault;
        }
        return Object.freeze([...participants.values()]);
    }

    return Object.freeze({
        register,
        freeze,
        has: /** @param {string} id */ id => participants.has(id),
    });
}

let defaultRegistry;

export function getDefaultChatSurfaceParticipantRegistry() {
    defaultRegistry ??= createChatSurfaceParticipantRegistry();
    return defaultRegistry;
}
