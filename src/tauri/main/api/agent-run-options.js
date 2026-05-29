// @ts-check

export function normalizeAgentRunOptions(value, presentationOverride = undefined) {
    if (value != null && !isPlainObject(value)) {
        throw new Error('agent.options_invalid: options must be an object');
    }

    const options = value || {};
    if (options.stream === true) {
        throw new Error('agent.phase2b_stream_unsupported: Agent Phase 2B only supports non-streaming model calls');
    }
    if (Object.prototype.hasOwnProperty.call(options, 'autoCommit')) {
        throw new Error('agent.auto_commit_removed: Agent chat commits are driven by workspace.commit');
    }
    const presentation = normalizeAgentRunPresentation(presentationOverride ?? options.presentation);

    return {
        ...options,
        stream: false,
        ...(presentation ? { presentation } : {}),
    };
}

function normalizeAgentRunPresentation(value) {
    if (value == null || value === '') {
        return undefined;
    }
    const presentation = String(value).trim();
    if (presentation !== 'foreground' && presentation !== 'background') {
        throw new Error('agent.presentation_invalid: presentation must be foreground or background');
    }
    return presentation;
}

function isPlainObject(value) {
    return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}
