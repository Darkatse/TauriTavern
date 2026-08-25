import { cleanup, render, screen, waitFor } from '@testing-library/react';
import { afterEach, expect, test } from '@rstest/core';
import userEvent from '@testing-library/user-event';
import { createRef } from 'react';

import { tr } from './host';
import { installPopupHost, TestPopup, uninstallPopupHost } from './popup-stub';
import {
    openToolDialog,
    ToolDescriptionDialog,
    withDescription,
} from './tool-dialog';

const SERVER_ID = '11111111-1111-4111-8111-111111111111';

function tool(): TauriTavernMcpTool {
    return {
        id: `mcp/${SERVER_ID}:search`,
        nativeName: 'search',
        title: 'Search files',
        description: 'Search local files by name.',
        inputSchema: { type: 'object', properties: { query: { type: 'string' } }, required: ['query'] },
        annotations: {},
        permission: 'off',
    };
}

function unexpectedSave() {
    return () => Promise.reject(new Error('Unexpected save'));
}

function currentPopup(): TestPopup {
    const popup = TestPopup.current;
    if (!popup) {
        throw new Error('Tool dialog popup was not created');
    }
    return popup;
}

afterEach(() => {
    cleanup();
    uninstallPopupHost();
});

test('folds an edited description into the saved override', () => {
    expect(withDescription(undefined, '  Only search local files.  ')).toEqual({
        description: '  Only search local files.  ',
    });
    expect(withDescription({ properties: { query: 'Search terms.' } }, 'Custom text')).toEqual({
        description: 'Custom text',
        properties: { query: 'Search terms.' },
    });
    // Emptying the draft drops only the description; sibling settings survive.
    expect(withDescription({ description: 'Custom', properties: { query: 'Search terms.' } }, '   ')).toEqual({
        properties: { query: 'Search terms.' },
    });
    expect(withDescription({ description: 'Custom' }, '')).toBeNull();
    expect(withDescription(undefined, '')).toBeNull();
});

test('shows the server description as the reference and seeds the draft from the override', () => {
    render(
        <ToolDescriptionDialog
            tool={tool()}
            override={{ description: 'My custom text.' }}
            save={unexpectedSave()}
            tr={tr}
            ref={createRef()}
        />,
    );

    expect(screen.getByText('Server description')).toBeTruthy();
    expect(screen.getByText('Search local files by name.')).toBeTruthy();
    expect(screen.getByLabelText<HTMLTextAreaElement>('Custom description').value).toBe('My custom text.');
});

test('omits the reference block when the server offers no description', () => {
    const descriptionless = { ...tool() };
    delete descriptionless.description;
    render(
        <ToolDescriptionDialog
            tool={descriptionless}
            override={undefined}
            save={unexpectedSave()}
            tr={tr}
            ref={createRef()}
        />,
    );

    expect(screen.queryByText('Server description')).toBeNull();
    expect(screen.getByLabelText<HTMLTextAreaElement>('Custom description').value).toBe('');
});

test('saves through the popup affirmative and closes only after success', async () => {
    installPopupHost();
    const saved: (TauriTavernToolDescriptionOverride | null)[] = [];
    const opened = openToolDialog({
        tool: tool(),
        override: { properties: { query: 'Search terms.' } },
        save: override => {
            saved.push(override);
            return Promise.resolve();
        },
    });
    const popup = currentPopup();

    const user = userEvent.setup();
    const draft = await screen.findByLabelText<HTMLTextAreaElement>('Custom description');
    await waitFor(() => expect(document.activeElement).toBe(draft));
    await user.type(draft, 'Use only for local filename searches.');

    expect(await popup.close(1)).toBe(true);
    await opened;
    expect(saved).toEqual([{
        description: 'Use only for local filename searches.',
        properties: { query: 'Search terms.' },
    }]);
});

test('resets the complete override through the popup custom action', async () => {
    installPopupHost();
    const saved: (TauriTavernToolDescriptionOverride | null)[] = [];
    const opened = openToolDialog({
        tool: tool(),
        override: {
            description: 'Custom text.',
            properties: { query: 'Search terms.' },
        },
        save: override => {
            saved.push(override);
            return Promise.resolve();
        },
    });
    const popup = currentPopup();

    await screen.findByLabelText('Custom description');
    expect(await popup.close(2)).toBe(true);
    await opened;
    expect(saved).toEqual([null]);
});

test('vetoes another close while saving', async () => {
    installPopupHost();
    let finishSave: (() => void) | undefined;
    const pendingSave = new Promise<void>(resolve => {
        finishSave = resolve;
    });
    let calls = 0;
    const opened = openToolDialog({
        tool: tool(),
        override: undefined,
        save: () => {
            calls += 1;
            return pendingSave;
        },
    });
    const popup = currentPopup();

    const user = userEvent.setup();
    const draft = await screen.findByLabelText<HTMLTextAreaElement>('Custom description');
    await user.type(draft, 'Custom text');

    const saving = popup.close(1);
    await waitFor(() => expect(calls).toBe(1));
    expect(await popup.close(0)).toBe(false);
    finishSave?.();
    expect(await saving).toBe(true);
    await opened;
});

test('keeps the dialog open with the error in place when saving fails', async () => {
    installPopupHost();
    let attempts = 0;
    const opened = openToolDialog({
        tool: tool(),
        override: undefined,
        save: () => {
            attempts += 1;
            return Promise.reject(new Error('storage is read-only'));
        },
    });
    const popup = currentPopup();

    const user = userEvent.setup();
    const draft = await screen.findByLabelText<HTMLTextAreaElement>('Custom description');
    await user.type(draft, 'Custom text');

    expect(await popup.close(1)).toBe(false);
    expect((await screen.findByRole('alert')).textContent).toBe('storage is read-only');
    expect(attempts).toBe(1);

    // A later cancel still discards the draft without another save attempt.
    expect(await popup.close(0)).toBe(true);
    await opened;
    expect(attempts).toBe(1);
});

test('discards the draft on cancel without saving', async () => {
    installPopupHost();
    let calls = 0;
    const opened = openToolDialog({
        tool: tool(),
        override: { description: 'Saved text.' },
        save: () => {
            calls += 1;
            return Promise.resolve();
        },
    });
    const popup = currentPopup();

    const user = userEvent.setup();
    const draft = await screen.findByLabelText<HTMLTextAreaElement>('Custom description');
    await user.clear(draft);
    await user.type(draft, 'Unsaved edits');

    expect(await popup.close(0)).toBe(true);
    await opened;
    expect(calls).toBe(0);
});
