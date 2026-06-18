// @ts-check

/**
 * @param {{ safeInvoke: (command: string, args?: any) => Promise<any> }} deps
 */
export function createAgentRunGuidanceApi({ safeInvoke }) {
    async function submitGuidance(input = {}) {
        if (!isPlainObject(input)) {
            throw new Error('Agent submitGuidance input must be an object');
        }

        const runId = requireRunId(input.runId);
        const text = String(input.text || '').trim();
        if (!text) {
            throw new Error('guidance text is required');
        }
        const clientGuidanceId = normalizeOptionalString(input.clientGuidanceId ?? input.client_guidance_id);

        return safeInvoke('submit_agent_run_guidance', {
            dto: {
                runId,
                text,
                ...(clientGuidanceId ? { clientGuidanceId } : {}),
            },
        });
    }

    return {
        submitGuidance,
    };
}

function requireRunId(value) {
    const runId = String(value || '').trim();
    if (!runId) {
        throw new Error('runId is required');
    }
    return runId;
}

function normalizeOptionalString(value) {
    if (value == null || value === '') {
        return undefined;
    }
    const text = String(value).trim();
    return text || undefined;
}

function isPlainObject(value) {
    return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}
