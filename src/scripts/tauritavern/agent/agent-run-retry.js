const SUPPORTED_AGENT_RETRY_GENERATION_TYPES = new Set(['normal', 'regenerate', 'swipe']);

export async function retryAgentRunFailure({
    run = null,
    events = [],
    terminalEvent = null,
    runtime = null,
} = {}) {
    assertRetryableTerminalEvent(terminalEvent);
    const originalType = resolveAgentRunGenerationType({ run, events });
    const retryType = retryGenerationTypeFor(originalType);
    const resolvedRuntime = runtime || await defaultRetryRuntime();
    assertRetryRuntime(resolvedRuntime);

    const options = await resolvedRuntime.getAgentGenerationOptions({
        generationType: retryType,
        mainApi: resolvedRuntime.mainApi,
        selectedGroup: resolvedRuntime.selectedGroup,
    });
    if (options?.agentMode !== true) {
        throw new Error('agent.retry_agent_mode_disabled: Agent Mode must be enabled to retry an Agent run');
    }

    return resolvedRuntime.Generate(retryType, options);
}

export function resolveAgentRunGenerationType({ run = null, events = [] } = {}) {
    const fromRun = normalizeGenerationType(run?.generationType);
    if (fromRun) {
        return fromRun;
    }

    const orderedEvents = Array.isArray(events) ? events : [];
    for (let index = orderedEvents.length - 1; index >= 0; index -= 1) {
        const event = orderedEvents[index];
        if (event?.type !== 'generation_intent_recorded') {
            continue;
        }
        const fromIntent = normalizeGenerationType(event?.payload?.generationType);
        if (fromIntent) {
            return fromIntent;
        }
    }

    throw new Error('agent.retry_generation_intent_missing: generation_intent_recorded is required to retry an Agent run');
}

export function retryGenerationTypeFor(generationType) {
    const normalized = normalizeGenerationType(generationType);
    if (!SUPPORTED_AGENT_RETRY_GENERATION_TYPES.has(normalized)) {
        throw new Error(`agent.retry_generation_type_unsupported: cannot retry Agent generation type ${generationType}`);
    }
    return normalized === 'swipe' ? 'swipe' : 'regenerate';
}

function assertRetryableTerminalEvent(event) {
    if (event?.type !== 'run_failed' || event?.payload?.userRetryable !== true) {
        throw new Error('agent.retry_not_allowed: only user-retryable run_failed events can be retried');
    }
}

function assertRetryRuntime(runtime) {
    if (typeof runtime?.Generate !== 'function') {
        throw new Error('agent.retry_generate_unavailable: Generate is unavailable');
    }
    if (typeof runtime?.getAgentGenerationOptions !== 'function') {
        throw new Error('agent.retry_generation_router_unavailable: getAgentGenerationOptions is unavailable');
    }
}

async function defaultRetryRuntime() {
    const [script, groupChats, router] = await Promise.all([
        import(/* webpackIgnore: true */ '/script.js'),
        import(/* webpackIgnore: true */ '/scripts/group-chats.js'),
        import(/* webpackIgnore: true */ '/scripts/tauritavern/agent/agent-generation-router.js'),
    ]);
    return {
        Generate: script.Generate,
        getAgentGenerationOptions: router.getAgentGenerationOptions,
        mainApi: script.main_api,
        selectedGroup: groupChats.selected_group,
    };
}

function normalizeGenerationType(value) {
    return String(value || '').trim();
}
