import { cleanup, screen, waitFor } from '@testing-library/react';
import { afterEach, expect, test } from '@rstest/core';
import userEvent from '@testing-library/user-event';

import { installPopupHost, TestPopup, uninstallPopupHost } from './popup-stub';
import { openToolDialog } from './tool-dialog';

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

test('editing a description preserves sibling settings and removes an empty override', async () => {
    installPopupHost();
    const user = userEvent.setup();
    const properties = { query: 'Search terms.' };
    for (const { override, description, expected } of [
        {
            override: { properties },
            description: '  Only search local files.  ',
            expected: { description: '  Only search local files.  ', properties },
        },
        {
            override: { description: 'Saved text', properties },
            description: '   ',
            expected: { properties },
        },
        {
            override: { description: 'Saved text' },
            description: '',
            expected: null,
        },
    ]) {
        const saved: (TauriTavernToolDescriptionOverride | null)[] = [];
        const opened = openToolDialog({
            tool: tool(),
            override,
            save: value => {
                saved.push(value);
                return Promise.resolve();
            },
        });
        const draft = await screen.findByLabelText<HTMLTextAreaElement>('Custom description');
        await waitFor(() => expect(document.activeElement).toBe(draft));
        await user.clear(draft);
        if (description) await user.type(draft, description);

        expect(await currentPopup().close(1)).toBe(true);
        await opened;
        expect(saved).toEqual([expected]);
    }
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
