// @ts-check

export function normalizeAgentRunOptions(value, presentationOverride = undefined) {
    if (value != null && !isPlainObject(value)) {
        throw new Error('agent.options_invalid: options must be an object');
    }

    const options = value || {};
    if (Object.prototype.hasOwnProperty.call(options, 'stream') && typeof options.stream !== 'boolean') {
        throw new Error('agent.stream_invalid: stream must be a boolean');
    }
    if (Object.prototype.hasOwnProperty.call(options, 'autoCommit')) {
        throw new Error('agent.auto_commit_removed: Agent chat commits are driven by workspace.commit');
    }
    const presentation = normalizeAgentRunPresentation(presentationOverride ?? options.presentation);

    return {
        ...options,
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
