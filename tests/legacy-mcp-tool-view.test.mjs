import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const {
    createLegacyMcpGenerationContext,
    LegacyMcpOutcomeUnknownError,
} = await import(pathToFileURL(path.join(
    REPO_ROOT,
    'src/scripts/tauritavern/legacy-mcp-tools.js',
)));

const descriptor = {
    toolId: 'mcp/00000000-0000-0000-0000-000000000001:issue.create',
    nativeName: 'issue.create',
    serverDisplayName: 'my server',
    title: 'Create issue',
    description: 'Create one issue',
    inputSchema: {
        type: 'object',
        properties: { title: { type: 'string' } },
    },
};

function toolNames(toolData) {
    return toolData.tools.map(tool => tool.function.name);
}

async function createContext({ tools = [descriptor], invokeCommand, createExecutionCallId } = {}) {
    const command = invokeCommand ?? (async name => {
        assert.equal(name, 'list_legacy_mcp_tools');
        return { tools, diagnostics: [] };
    });
    return createLegacyMcpGenerationContext({
        invokeCommand: command,
        createExecutionCallId,
    });
}

test('one root descriptor snapshot creates fresh round aliases from the current local tool view', async () => {
    let listCalls = 0;
    const context = await createContext({
        tools: [
            descriptor,
            {
                ...descriptor,
                toolId: 'mcp/00000000-0000-0000-0000-000000000002:issue_create',
                nativeName: 'issue_create',
                serverDisplayName: 'my.server',
            },
        ],
        invokeCommand: async command => {
            assert.equal(command, 'list_legacy_mcp_tools');
            listCalls += 1;
            return {
                tools: [
                    descriptor,
                    {
                        ...descriptor,
                        toolId: 'mcp/00000000-0000-0000-0000-000000000002:issue_create',
                        nativeName: 'issue_create',
                        serverDisplayName: 'my.server',
                    },
                ],
                diagnostics: [],
            };
        },
    });

    const firstRound = context.createRound();
    const firstToolData = {
        tools: [{ type: 'function', function: { name: 'mcp__my_server__issue_create' } }],
    };
    firstRound.mergeIntoToolData(firstToolData);
    assert.deepEqual(toolNames(firstToolData), [
        'mcp__my_server__issue_create',
        'mcp__my_server__issue_create__2',
        'mcp__my_server__issue_create__3',
    ]);
    assert.equal(firstToolData.tool_choice, 'auto');
    firstToolData.tools[1].function.parameters.properties.title.type = 'number';

    const recursiveRound = context.createRound();
    const recursiveToolData = {};
    recursiveRound.mergeIntoToolData(recursiveToolData);
    assert.deepEqual(toolNames(recursiveToolData), [
        'mcp__my_server__issue_create',
        'mcp__my_server__issue_create__2',
    ]);
    assert.equal(recursiveToolData.tools[0].function.parameters.properties.title.type, 'string');
    assert.equal(listCalls, 1);

    const longContext = await createContext({
        tools: [{
            ...descriptor,
            serverDisplayName: 'server'.repeat(30),
            nativeName: 'tool'.repeat(40),
        }],
    });
    const longRound = longContext.createRound();
    const longToolData = {};
    longRound.mergeIntoToolData(longToolData);
    assert.ok(toolNames(longToolData)[0].length <= 64);
});

test('post-hook binding uses the initial alias/final unique-name intersection', async () => {
    const context = await createContext();

    const cloneRound = context.createRound();
    const cloneData = {};
    cloneRound.mergeIntoToolData(cloneData);
    const alias = toolNames(cloneData)[0];
    const sameNameClone = structuredClone(cloneData.tools[0]);
    cloneRound.finalizeAdvertisedTools({ tools: [sameNameClone] });
    assert.equal(cloneRound.resolveTool(alias).displayName, 'Create issue (my server)');
    assert.equal(cloneRound.resolveTool('ordinary_local_tool'), null);

    const renameRound = context.createRound();
    const renameData = {};
    renameRound.mergeIntoToolData(renameData);
    renameData.tools[0].function.name = `${alias}_renamed`;
    renameRound.finalizeAdvertisedTools(renameData);
    assert.equal(renameRound.resolveTool(`${alias}_renamed`), null);
    const renamedOriginalAlias = await renameRound.resolveTool(alias).invoke({ argumentsJson: '{}' });
    assert.equal(renamedOriginalAlias.error, true);
    assert.match(renamedOriginalAlias.result, /mcp\.legacy_tool_not_advertised/);

    const collisionRound = context.createRound();
    const collisionData = {};
    collisionRound.mergeIntoToolData(collisionData);
    collisionRound.finalizeAdvertisedTools({
        tools: [collisionData.tools[0], structuredClone(collisionData.tools[0])],
    });
    const collision = await collisionRound.resolveTool(alias).invoke({ argumentsJson: '{}' });
    assert.equal(collision.error, true);
    assert.match(collision.result, /post-hook frontend payload/);

    const removedRound = context.createRound();
    removedRound.mergeIntoToolData({});
    removedRound.finalizeAdvertisedTools({});
    assert.notEqual(removedRound.resolveTool(alias), null);
});

test('MCP execution uses a fresh executionCallId and preserves complete known/not-sent outcomes', async () => {
    const calls = [];
    let ordinal = 0;
    let outcome = {
        outcome: 'known_response',
        response: {
            kind: 'tool_result',
            isError: false,
            textBlocks: [{ index: 0, text: 'created' }],
            structuredJson: '{"id":1}',
            diagnostics: [],
        },
    };
    const context = await createContext({
        createExecutionCallId: () => `execution-${++ordinal}`,
        invokeCommand: async (command, args) => {
            calls.push({ command, args });
            if (command === 'list_legacy_mcp_tools') {
                return { tools: [descriptor], diagnostics: [] };
            }
            if (command === 'call_legacy_mcp_tool') {
                return outcome;
            }
        },
    });
    const round = context.createRound();
    const toolData = {};
    round.mergeIntoToolData(toolData);
    round.finalizeAdvertisedTools(toolData);
    const requestTool = round.resolveTool(toolNames(toolData)[0]);
    calls.length = 0;

    const success = await requestTool.invoke({
        argumentsJson: '{"value":42}',
    });
    assert.deepEqual(success, { result: JSON.stringify(outcome, null, 2), error: false });
    assert.deepEqual(calls, [
        {
            command: 'start_legacy_mcp_tool_call',
            args: { dto: { executionCallId: 'execution-1' } },
        },
        {
            command: 'call_legacy_mcp_tool',
            args: {
                dto: {
                    executionCallId: 'execution-1',
                    toolId: descriptor.toolId,
                    argumentsJson: '{"value":42}',
                },
            },
        },
    ]);
    for (const errorOutcome of [
        {
            outcome: 'known_response',
            response: { kind: 'tool_result', isError: true, textBlocks: [], diagnostics: [] },
        },
        {
            outcome: 'known_response',
            response: { kind: 'server_error', code: -32602, message: 'bad arguments' },
        },
        {
            outcome: 'not_sent',
            code: 'mcp.call_permission_off',
            message: 'The tool is Off',
        },
    ]) {
        outcome = errorOutcome;
        const result = await requestTool.invoke({ argumentsJson: '' });
        assert.deepEqual(result, { result: JSON.stringify(errorOutcome, null, 2), error: true });
    }
});

test('Abort and OutcomeUnknown terminate without fabricating a tool result', async () => {
    const calls = [];
    let resolveRemote;
    let remoteOutcome = new Promise(resolve => {
        resolveRemote = resolve;
    });
    let ordinal = 0;
    const context = await createContext({
        createExecutionCallId: () => `execution-${++ordinal}`,
        invokeCommand: async (command, args) => {
            calls.push({ command, args });
            if (command === 'list_legacy_mcp_tools') {
                return { tools: [descriptor], diagnostics: [] };
            }
            if (command === 'call_legacy_mcp_tool') {
                return remoteOutcome;
            }
        },
    });
    const round = context.createRound();
    const toolData = {};
    round.mergeIntoToolData(toolData);
    round.finalizeAdvertisedTools(toolData);
    const requestTool = round.resolveTool(toolNames(toolData)[0]);
    calls.length = 0;

    const preAborted = new AbortController();
    preAborted.abort('stop');
    await assert.rejects(
        requestTool.invoke({ argumentsJson: '{}', signal: preAborted.signal }),
        error => error.name === 'AbortError',
    );
    assert.equal(calls.length, 0);

    resolveRemote({
        outcome: 'outcome_unknown',
        code: 'mcp.call_outcome_unknown',
        message: 'The tool may have executed',
    });
    await assert.rejects(
        requestTool.invoke({ argumentsJson: '{}' }),
        error => error instanceof LegacyMcpOutcomeUnknownError
            && error.code === 'mcp.call_outcome_unknown',
    );

    calls.length = 0;
    remoteOutcome = new Promise(resolve => {
        resolveRemote = resolve;
    });
    const cancelled = new AbortController();
    const cancelledCall = requestTool.invoke({
        argumentsJson: '{}',
        signal: cancelled.signal,
    });
    while (!calls.some(call => call.command === 'call_legacy_mcp_tool')) {
        await Promise.resolve();
    }
    cancelled.abort('stop');
    resolveRemote({ outcome: 'not_sent', code: 'mcp.call_cancelled_before_send', message: 'cancelled' });
    await assert.rejects(cancelledCall, error => error.name === 'AbortError');
    assert.deepEqual(calls.map(call => call.command), [
        'start_legacy_mcp_tool_call',
        'call_legacy_mcp_tool',
        'cancel_legacy_mcp_tool_call',
    ]);

    calls.length = 0;
    remoteOutcome = new Promise(resolve => {
        resolveRemote = resolve;
    });
    const knownAfterAbort = new AbortController();
    const knownCall = requestTool.invoke({
        argumentsJson: '{}',
        signal: knownAfterAbort.signal,
    });
    while (!calls.some(call => call.command === 'call_legacy_mcp_tool')) {
        await Promise.resolve();
    }
    knownAfterAbort.abort('stop');
    const knownOutcome = {
        outcome: 'known_response',
        response: { kind: 'tool_result', isError: false, textBlocks: [], diagnostics: [] },
    };
    resolveRemote(knownOutcome);
    assert.deepEqual(await knownCall, { result: JSON.stringify(knownOutcome, null, 2), error: false });
});
