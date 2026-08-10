const MAX_MODEL_ALIAS_BYTES = 64;

function hostSafeInvoke(command, args) {
    const safeInvoke = globalThis.window?.__TAURITAVERN__?.invoke?.safeInvoke;
    if (typeof safeInvoke !== 'function') {
        throw new Error('Host ABI safeInvoke is missing');
    }
    return safeInvoke(command, args);
}

export class LegacyMcpOutcomeUnknownError extends Error {
    constructor(code, message) {
        super(`${code}: ${message}`);
        this.name = 'LegacyMcpOutcomeUnknownError';
        this.code = code;
    }
}

function abortError(reason) {
    const error = new Error(typeof reason === 'string' ? reason : 'Generation was aborted.');
    error.name = 'AbortError';
    return error;
}

function normalizeAliasSegment(value, fallback) {
    const normalized = value.replaceAll(/[^A-Za-z0-9-]+/g, '_').replaceAll(/^_+|_+$/g, '');
    return normalized || fallback;
}

function fitMcpAlias(server, tool, suffix) {
    const available = MAX_MODEL_ALIAS_BYTES - 'mcp__'.length - '__'.length - suffix.length;
    let serverLength = server.length;
    let toolLength = tool.length;
    if (serverLength + toolLength > available) {
        serverLength = Math.min(serverLength, 20, Math.floor(available / 2));
        toolLength = Math.min(toolLength, available - serverLength);
        serverLength = Math.min(server.length, available - toolLength);
    }
    return `mcp__${server.slice(0, serverLength)}__${tool.slice(0, toolLength)}${suffix}`;
}

function allocateMcpAlias(serverName, toolName, used) {
    const server = normalizeAliasSegment(serverName, 'server');
    const tool = normalizeAliasSegment(toolName, 'tool');
    for (let ordinal = 1; ; ordinal += 1) {
        const suffix = ordinal === 1 ? '' : `__${ordinal}`;
        const alias = fitMcpAlias(server, tool, suffix);
        if (!used.has(alias)) {
            return alias;
        }
    }
}

function serializeOutcome(outcome) {
    return JSON.stringify(outcome, null, 2);
}

function knownOutcomeError(outcome) {
    if (outcome.outcome === 'not_sent') {
        return true;
    }
    if (outcome.outcome !== 'known_response') {
        throw new Error(`Unexpected Legacy MCP call outcome: ${String(outcome.outcome)}`);
    }
    return outcome.response.kind !== 'tool_result' || outcome.response.isError === true;
}

async function callLegacyMcpTool(binding, argumentsJson, signal, { invokeCommand, createExecutionCallId }) {
    if (signal?.aborted) {
        throw abortError(signal.reason);
    }

    const executionCallId = createExecutionCallId();
    const cancel = () => {
        void invokeCommand('cancel_legacy_mcp_tool_call', { dto: { executionCallId } })
            .catch(error => console.debug('Failed to stop Legacy MCP tool call:', error));
    };

    await invokeCommand('start_legacy_mcp_tool_call', { dto: { executionCallId } });
    if (signal?.aborted) {
        cancel();
        throw abortError(signal.reason);
    }

    if (signal) {
        signal.addEventListener('abort', cancel, { once: true });
    }
    let outcome;
    try {
        outcome = await invokeCommand('call_legacy_mcp_tool', {
            dto: {
                executionCallId,
                toolId: binding.toolId,
                argumentsJson,
            },
        });
    } catch (error) {
        cancel();
        throw error;
    } finally {
        signal?.removeEventListener('abort', cancel);
    }

    if (outcome?.outcome === 'outcome_unknown') {
        throw new LegacyMcpOutcomeUnknownError(outcome.code, outcome.message);
    }
    if (outcome?.outcome === 'not_sent' && signal?.aborted) {
        throw abortError(signal.reason);
    }
    return {
        result: serializeOutcome(outcome),
        error: knownOutcomeError(outcome),
    };
}

class LegacyMcpToolRound {
    #context;
    #initialBindings = new Map();
    #advertisedBindings = new Map();
    #merged = false;
    #finalized = false;

    constructor(context) {
        this.#context = context;
    }

    mergeIntoToolData(toolData) {
        if (this.#merged) {
            throw new Error('Legacy MCP round was merged more than once');
        }
        this.#merged = true;

        const tools = toolData.tools ?? [];
        const used = new Set(tools.map(tool => tool.function.name));
        for (const binding of this.#context.tools) {
            const alias = allocateMcpAlias(binding.serverDisplayName, binding.nativeName, used);
            used.add(alias);
            this.#initialBindings.set(alias, binding);
            tools.push({
                type: 'function',
                function: {
                    name: alias,
                    description: binding.description || binding.title || binding.nativeName,
                    parameters: structuredClone(binding.inputSchema),
                },
            });
        }
        if (this.#initialBindings.size > 0) {
            toolData.tools = tools;
            toolData.tool_choice = 'auto';
        }
    }

    finalizeAdvertisedTools(generateData) {
        if (!this.#merged || this.#finalized) {
            throw new Error('Legacy MCP round must be merged once and finalized once');
        }
        this.#finalized = true;

        const nameCounts = new Map();
        for (const tool of Array.isArray(generateData.tools) ? generateData.tools : []) {
            const name = tool?.function?.name;
            if (typeof name === 'string') {
                nameCounts.set(name, (nameCounts.get(name) ?? 0) + 1);
            }
        }
        for (const [alias, binding] of this.#initialBindings) {
            if (nameCounts.get(alias) === 1) {
                this.#advertisedBindings.set(alias, binding);
            }
        }
    }

    resolveTool(name) {
        if (!this.#finalized) {
            throw new Error('Legacy MCP tools cannot be resolved before the final payload hook');
        }
        const binding = this.#initialBindings.get(name);
        if (!binding) {
            return null;
        }
        const displayName = `${binding.title || binding.nativeName} (${binding.serverDisplayName})`;
        const advertised = this.#advertisedBindings.has(name);
        return {
            displayName,
            formatMessage: () => `Invoking MCP tool: ${displayName}`,
            invoke: advertised
                ? ({ argumentsJson, signal }) => callLegacyMcpTool(binding, argumentsJson, signal, this.#context)
                : async () => ({
                    result: serializeOutcome({
                        outcome: 'not_sent',
                        code: 'mcp.legacy_tool_not_advertised',
                        message: `MCP tool alias "${name}" was not uniquely advertised in the post-hook frontend payload`,
                    }),
                    error: true,
                }),
        };
    }
}

class LegacyMcpGenerationContext {
    constructor(tools, diagnostics, invokeCommand, createExecutionCallId) {
        this.tools = tools;
        this.diagnostics = diagnostics;
        this.invokeCommand = invokeCommand;
        this.createExecutionCallId = createExecutionCallId;
    }

    createRound() {
        return new LegacyMcpToolRound(this);
    }
}

export function createEmptyLegacyMcpGenerationContext() {
    return new LegacyMcpGenerationContext(
        [],
        [],
        hostSafeInvoke,
        () => globalThis.crypto.randomUUID(),
    );
}

export async function createLegacyMcpGenerationContext({
    invokeCommand = hostSafeInvoke,
    createExecutionCallId = () => globalThis.crypto.randomUUID(),
} = {}) {
    const resolution = await invokeCommand('list_legacy_mcp_tools');
    if (!Array.isArray(resolution?.tools) || !Array.isArray(resolution?.diagnostics)) {
        throw new Error('list_legacy_mcp_tools returned an invalid resolution');
    }
    for (const diagnostic of resolution.diagnostics) {
        console.warn(`[Legacy MCP] ${diagnostic.code}: ${diagnostic.message}`);
    }
    return new LegacyMcpGenerationContext(
        resolution.tools,
        resolution.diagnostics,
        invokeCommand,
        createExecutionCallId,
    );
}
