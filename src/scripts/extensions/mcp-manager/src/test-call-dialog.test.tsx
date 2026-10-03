import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, expect, test } from '@rstest/core';
import userEvent from '@testing-library/user-event';
import { createRef } from 'react';

import { tr } from './host';
import { installPopupHost, TestPopup, uninstallPopupHost } from './popup-stub';
import {
    openTestCallDialog,
    TestCallDialog,
    type TestCallDialogDeps,
} from './test-call-dialog';

const SERVER_A = '11111111-1111-4111-8111-111111111111';

function server(
    id: string,
    state: TauriTavernMcpServerState,
    displayName = 'Local tools',
): TauriTavernMcpServer {
    return {
        id,
        displayName,
        endpoint: `http://127.0.0.1:3000/mcp/${id.slice(0, 4)}`,
        headers: {},
        protocolVersion: 'auto',
        state,
        toolPermissions: {},
        toolDescriptionOverrides: {},
    };
}

function richTool(): TauriTavernMcpTool {
    return {
        id: `mcp/${SERVER_A}:search`,
        nativeName: 'search',
        title: 'Search files',
        description: 'Search local files by name.',
        inputSchema: {
            type: 'object',
            properties: {
                query: { type: 'string', description: 'Search query' },
                limit: { type: 'integer' },
                recursive: { type: 'boolean' },
                mode: { enum: ['fast', 'deep'] },
                tags: { type: 'array', items: { type: 'string' } },
                options: { type: 'object' },
            },
            required: ['query'],
        },
        annotations: {},
        permission: 'off',
    };
}

function emptyTool(): TauriTavernMcpTool {
    return {
        id: `mcp/${SERVER_A}:ping`,
        nativeName: 'ping',
        inputSchema: { type: 'object' },
        annotations: {},
        permission: 'ask',
    };
}

function discoveryFor(
    registrationId: string,
    tools: TauriTavernMcpTool[],
    diagnostics: TauriTavernMcpDiscoveryResult['diagnostics'] = [],
): TauriTavernMcpDiscoveryResult {
    return {
        registrationId,
        protocolVersion: '2026-07-28',
        tools,
        diagnostics,
        staleTools: [],
    };
}

function deps(overrides: Partial<TestCallDialogDeps> = {}): TestCallDialogDeps {
    return {
        servers: [server(SERVER_A, 'active')],
        discover: () => Promise.resolve(discoveryFor(SERVER_A, [richTool()])),
        refresh: () => Promise.resolve(discoveryFor(SERVER_A, [richTool()])),
        testCall: () => Promise.reject(new Error('unexpected testCall')),
        ...overrides,
    };
}

function renderDialog(overrides: Partial<TestCallDialogDeps> = {}) {
    return render(<TestCallDialog {...deps(overrides)} tr={tr} ref={createRef()} />);
}

afterEach(() => {
    cleanup();
    uninstallPopupHost();
});

test('uses explicit refresh when the user retries a failed catalog load', async () => {
    let refreshes = 0;
    const user = userEvent.setup();
    renderDialog({
        discover: () => Promise.reject(new Error('stored catalog is invalid')),
        refresh: () => {
            refreshes += 1;
            return Promise.resolve(discoveryFor(SERVER_A, [richTool()]));
        },
    });

    expect((await screen.findByRole('alert')).textContent).toBe('stored catalog is invalid');
    await user.click(screen.getByRole('button', { name: 'Retry' }));

    expect(await screen.findByRole('option', { name: 'Search files' })).toBeTruthy();
    expect(refreshes).toBe(1);
});

test('builds arguments from friendly fields and preserves raw number precision', async () => {
    let received: Parameters<TestCallDialogDeps['testCall']>[0] | undefined;
    const user = userEvent.setup();
    renderDialog({
        discover: () => Promise.resolve(discoveryFor(SERVER_A, [richTool()], [{
            code: 'mcp.catalog_persistence_failed',
            message: 'Catalog remains memory-only',
        }])),
        testCall: input => {
            received = input;
            return Promise.resolve({
                outcome: 'known_response',
                response: {
                    kind: 'tool_result',
                    isError: false,
                    textBlocks: [{ index: 0, text: '3 matches' }],
                    structuredJson: '{\n  "matches": 3\n}',
                    diagnostics: [],
                },
            } satisfies TauriTavernMcpTestCallOutcome);
        },
    });

    await screen.findByRole('option', { name: 'Search files' });
    expect(screen.getByText('Catalog remains memory-only')).toBeTruthy();
    await user.selectOptions(screen.getByLabelText('Tool'), 'search');

    await user.type(screen.getByLabelText(/query/), 'hello world');
    await user.type(screen.getByLabelText('limit'), '9007199254740993');
    await user.click(within(screen.getByRole('radiogroup', { name: 'recursive' })).getByRole('radio', { name: 'true' }));
    await user.selectOptions(screen.getByLabelText('mode'), '"deep"');
    await user.type(screen.getByLabelText('tags'), 'alpha{enter}beta');
    fireEvent.change(screen.getByLabelText('options'), { target: { value: '{"x":1}' } });

    await user.click(screen.getByRole('button', { name: 'Run test' }));

    await waitFor(() => expect(received).toEqual({
        registrationId: SERVER_A,
        nativeName: 'search',
        argumentsJson: '{"query":"hello world","limit":9007199254740993,"recursive":true,"mode":"deep","tags":["alpha","beta"],"options":{"x":1}}',
    }));
    expect(await screen.findByText('3 matches')).toBeTruthy();
    expect(screen.getByText('Server responded')).toBeTruthy();
    expect(document.querySelector('.tt-mcp-json-output')?.textContent).toBe('{\n  "matches": 3\n}');
});

test('blocks the call on missing required and malformed fields', async () => {
    let calls = 0;
    const user = userEvent.setup();
    renderDialog({
        testCall: () => {
            calls += 1;
            return Promise.reject(new Error('should not run'));
        },
    });

    await screen.findByRole('option', { name: 'Search files' });
    await user.selectOptions(screen.getByLabelText('Tool'), 'search');
    const query = screen.getByLabelText(/query/);
    expect(query.hasAttribute('required')).toBe(true);
    await user.type(screen.getByLabelText('limit'), '1.5');
    fireEvent.change(screen.getByLabelText('options'), { target: { value: '{broken' } });
    await user.click(screen.getByRole('button', { name: 'Run test' }));

    expect(calls).toBe(0);
    expect(screen.getByText('Required')).toBeTruthy();
    expect(screen.getByText('Enter a whole number.')).toBeTruthy();
    expect(screen.getByText('Enter valid JSON.')).toBeTruthy();
    expect(query.getAttribute('aria-invalid')).toBe('true');
    // Assistive technology announces the error together with the field's hint.
    expect(screen.getByRole('textbox', { name: /query/, description: 'Search query Required' })).toBe(query);

    // Editing a field clears only its own error.
    await user.type(query, 'anything');
    expect(screen.queryByText('Required')).toBeNull();
    expect(screen.getByText('Enter a whole number.')).toBeTruthy();
    expect(query.hasAttribute('aria-invalid')).toBe(false);
    expect(screen.getByRole('textbox', { name: /query/, description: 'Search query' })).toBe(query);
});

test('closing the popup aborts the local wait for an in-flight call', async () => {
    installPopupHost();
    let resolveCall!: (outcome: TauriTavernMcpTestCallOutcome) => void;
    let signal: AbortSignal | undefined;
    let argumentsJson: string | undefined;
    const opened = openTestCallDialog(deps({
        discover: () => Promise.resolve(discoveryFor(SERVER_A, [emptyTool()])),
        testCall: (input, options) => {
            argumentsJson = input.argumentsJson;
            signal = options?.signal;
            return new Promise<TauriTavernMcpTestCallOutcome>(resolve => {
                resolveCall = resolve;
            });
        },
    }));
    const popup = TestPopup.current;
    if (!popup) {
        throw new Error('Test-call popup was not created');
    }

    const user = userEvent.setup();
    await screen.findByRole('option', { name: 'ping' });
    await waitFor(() => expect(document.activeElement).toBe(screen.getByLabelText('Server')));
    await user.selectOptions(screen.getByLabelText('Tool'), 'ping');
    await user.click(screen.getByRole('button', { name: 'Run test' }));
    expect(await screen.findByText('Waiting for server…')).toBeTruthy();
    expect(argumentsJson).toBe('{}');

    expect(await popup.close(1)).toBe(true);
    expect(signal?.aborted).toBe(true);
    resolveCall({ outcome: 'outcome_unknown', code: 'mcp.call_cancelled', message: 'cancelled' });
    await opened;
});
