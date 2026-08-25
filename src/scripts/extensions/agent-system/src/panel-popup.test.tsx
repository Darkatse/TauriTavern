import { act, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { expect, test } from '@rstest/core';

import { openAgentSystemPanel } from './panel-popup';
import { defaultProfile } from './profile-model';

type StoredSettingsResult = { found: boolean; value?: unknown };

function installHost(tryGetJson: () => Promise<StoredSettingsResult>): void {
    const profile = defaultProfile();
    const profiles: TauriTavernAgentProfilesApi = {
        list: () => Promise.resolve({
            profiles: [{
                id: profile.id,
                displayName: profile.displayName,
                ...(profile.description !== undefined ? { description: profile.description } : {}),
                directRunnable: profile.run.directRunnable,
            }],
            issues: [],
        }),
        load: () => Promise.resolve({ profile: structuredClone(profile) }),
        diagnose: () => Promise.resolve({
            profileId: profile.id,
            previewAvailable: true,
            promptAssemblyAvailable: true,
            directRunAvailable: true,
            subAgentAvailable: true,
            diagnostics: [],
        }),
        resolveSystemPrompt: () => Promise.resolve({ agentSystemPrompt: 'Resolved prompt.' }),
        repairFile: () => Promise.resolve(),
        retargetPresetRefs: () => Promise.resolve({ updated: 0, profileIds: [] }),
        save: () => Promise.resolve(),
        delete: () => Promise.resolve(),
    };
    Object.defineProperty(window, '__TAURITAVERN__', {
        configurable: true,
        value: {
            ready: Promise.resolve(),
            api: {
                extension: {
                    store: {
                        tryGetJson,
                        setJson: () => Promise.resolve(),
                    },
                },
                agent: {
                    profiles,
                    tools: {
                        list: () => Promise.resolve({ tools: [], diagnostics: [] }),
                    },
                },
                llmConnections: {
                    save: () => Promise.resolve(),
                },
            },
        },
    });
    Object.defineProperty(window, 'SillyTavern', {
        configurable: true,
        value: {
            getContext: () => ({
                extensionSettings: { connectionManager: { modelTargets: [] } },
                getPresetManager: () => ({
                    getAllPresets: () => [],
                    findPreset: () => null,
                }),
                POPUP_RESULT: { AFFIRMATIVE: 1 },
                Popup: { show: { confirm: () => Promise.resolve(1) } },
            }),
        },
    });
}

function restoreProperty(target: object, key: PropertyKey, descriptor?: PropertyDescriptor): void {
    if (descriptor) {
        Object.defineProperty(target, key, descriptor);
    } else {
        Reflect.deleteProperty(target, key);
    }
}

test('popup opens synchronously, focuses the singleton, and cleans every close path', async () => {
    const hostDescriptor = Object.getOwnPropertyDescriptor(window, '__TAURITAVERN__');
    const sillyTavernDescriptor = Object.getOwnPropertyDescriptor(window, 'SillyTavern');
    const showModalDescriptor = Object.getOwnPropertyDescriptor(HTMLDialogElement.prototype, 'showModal');
    const closeDescriptor = Object.getOwnPropertyDescriptor(HTMLDialogElement.prototype, 'close');
    const focusDescriptor = Object.getOwnPropertyDescriptor(HTMLDialogElement.prototype, 'focus');
    let resolveSettings: ((value: StoredSettingsResult) => void) | null = null;
    const firstSettings = new Promise<StoredSettingsResult>((resolve) => {
        resolveSettings = resolve;
    });
    let settingsCalls = 0;
    let focusCalls = 0;
    installHost(() => {
        settingsCalls += 1;
        return settingsCalls === 1 ? firstSettings : Promise.resolve({ found: false });
    });
    Object.defineProperty(HTMLDialogElement.prototype, 'showModal', {
        configurable: true,
        value(this: HTMLDialogElement) {
            this.open = true;
        },
    });
    Object.defineProperty(HTMLDialogElement.prototype, 'close', {
        configurable: true,
        value(this: HTMLDialogElement) {
            this.open = false;
            this.dispatchEvent(new Event('close'));
        },
    });
    Object.defineProperty(HTMLDialogElement.prototype, 'focus', {
        configurable: true,
        value() {
            focusCalls += 1;
        },
    });

    try {
        act(() => openAgentSystemPanel());
        const firstDialog = document.querySelector<HTMLDialogElement>('dialog.ttas-dialog');
        expect(firstDialog?.open).toBe(true);

        act(() => openAgentSystemPanel());
        expect(document.querySelectorAll('dialog.ttas-dialog')).toHaveLength(1);
        expect(focusCalls).toBe(1);

        const cancel = new Event('cancel', { cancelable: true });
        act(() => {
            firstDialog?.dispatchEvent(cancel);
        });
        expect(cancel.defaultPrevented).toBe(true);
        expect(document.querySelector('dialog.ttas-dialog')).toBeNull();

        await act(async () => {
            resolveSettings?.({ found: false });
            await firstSettings;
        });

        act(() => openAgentSystemPanel());
        const secondDialog = document.querySelector<HTMLDialogElement>('dialog.ttas-dialog');
        if (!secondDialog) {
            throw new Error('expected the reopened Agent System dialog');
        }
        await waitFor(() => expect(secondDialog.querySelector('.ttas-panel-body')).not.toBeNull());
        await userEvent.setup().click(within(secondDialog).getByTitle('Close'));
        expect(document.querySelector('dialog.ttas-dialog')).toBeNull();

        Object.defineProperty(HTMLDialogElement.prototype, 'showModal', {
            configurable: true,
            value() {
                throw new Error('showModal failed');
            },
        });
        expect(() => act(() => openAgentSystemPanel())).toThrow('showModal failed');
        expect(document.querySelector('dialog.ttas-dialog')).toBeNull();
    } finally {
        document.querySelector<HTMLDialogElement>('dialog.ttas-dialog')?.close();
        restoreProperty(window, '__TAURITAVERN__', hostDescriptor);
        restoreProperty(window, 'SillyTavern', sillyTavernDescriptor);
        restoreProperty(HTMLDialogElement.prototype, 'showModal', showModalDescriptor);
        restoreProperty(HTMLDialogElement.prototype, 'close', closeDescriptor);
        restoreProperty(HTMLDialogElement.prototype, 'focus', focusDescriptor);
    }
});
