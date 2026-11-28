import { act, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, test } from '@rstest/core';

import {
    mountTauriTavernWebStorageApp,
    type WebStorageHandle,
    type WebStorageMountOptions,
    type WebStorageSelection,
} from './WebStorageApp';

declare global {
    var IS_REACT_ACT_ENVIRONMENT: boolean | undefined;
}
globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const handles: WebStorageHandle[] = [];

afterEach(() => {
    for (const handle of handles.splice(0)) {
        act(() => handle.unmount());
    }
});

test('deleting passes exactly the checked entries, in list order', async () => {
    const requests: WebStorageSelection[] = [];
    const options: WebStorageMountOptions = {
        snapshot: {
            localStorage: {
                usedBytes: 4_900_000,
                quotaBytes: 5_242_880,
                nearlyFull: true,
                entries: [
                    { key: 'memory', bytes: 4_000_000 },
                    { key: 'button-position', bytes: 900_000 },
                ],
            },
            databases: ['gallery', 'tts-cache'],
        },
        actions: {
            cleanUp: selection => {
                requests.push(selection);
                return Promise.resolve();
            },
            copyText: () => Promise.resolve(),
        },
        tr: key => key,
    };
    const container = document.createElement('div');
    document.body.append(container);
    act(() => {
        handles.push(mountTauriTavernWebStorageApp(container, options));
    });
    const view = within(container);
    const user = userEvent.setup();

    expect(view.getByText('Storage is nearly used up. Extensions may fail to save data.')).toBeTruthy();
    const deleteButton = view.getByRole<HTMLButtonElement>('button', { name: 'Delete Selected' });
    expect(deleteButton.disabled, 'Nothing is deleted before the user chooses').toBe(true);

    await user.click(within(view.getByRole('region', { name: 'Local Storage' })).getByLabelText('Select All'));
    await user.click(within(view.getByRole('region', { name: 'IndexedDB' })).getByLabelText('tts-cache'));
    await user.click(deleteButton);

    expect(requests).toEqual([{ localStorageKeys: ['memory', 'button-position'], databases: ['tts-cache'] }]);
});
