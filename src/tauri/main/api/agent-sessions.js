import { ensureModelTargetLlmConnectionForProfile } from '../../../scripts/tauritavern/agent/model-target-llm-connection.js';

export function createAgentSessionsApi({ safeInvoke, promptAssembly }) {
    const preparing = new Set();

    async function load() {
        return safeInvoke('load_agent_session_profile');
    }

    async function save(profile) {
        if (!profile || typeof profile !== 'object' || Array.isArray(profile)) {
            throw new Error('agent.profile_required: profile must be an object');
        }
        await ensureModelTargetLlmConnectionForProfile(profile);
        return safeInvoke('save_agent_session_profile', { dto: { profile } });
    }

    async function send({ sessionId, text } = {}) {
        sessionId = requireSessionId(sessionId);
        if (typeof text !== 'string' || !text.trim()) {
            throw new Error('agent.session_message_required: text cannot be empty');
        }
        if (preparing.has(sessionId)) throw new Error('agent.session_busy: this Session is already preparing a run');
        preparing.add(sessionId);
        try {
            const { profile } = await load();
            if (!profile) throw new Error('agent.session_profile_missing: configure the Session Profile before sending');
            await ensureModelTargetLlmConnectionForProfile(profile);
            const prepared = await safeInvoke('prepare_agent_session_run', { dto: { sessionId, text, profile } });
            const assembled = await promptAssembly.buildSnapshot(prepared.request);
            return await safeInvoke('start_agent_session_run', { dto: {
                sessionId,
                text,
                profile,
                expectedHistorySeq: prepared.expectedHistorySeq,
                promptSnapshot: assembled.promptSnapshot,
                frozenRunInputSnapshot: assembled.frozenRunInputSnapshot,
                generationIntent: { ...assembled.generationIntent, source: 'session', promptAssembly: prepared.assembly },
            } });
        } finally {
            preparing.delete(sessionId);
        }
    }

    return {
        profile: { load, save },
        create: () => safeInvoke('create_agent_session'),
        read(input) {
            const sessionId = requireSessionId(input?.sessionId);
            return safeInvoke('read_agent_session', { dto: {
                sessionId,
                ...(input.beforeSeq == null ? {} : { beforeSeq: input.beforeSeq }),
                ...(input.limit == null ? {} : { limit: input.limit }),
            } });
        },
        send,
    };
}

function requireSessionId(value) {
    if (typeof value !== 'string' || !value.trim()) throw new Error('sessionId is required');
    return value.trim();
}
