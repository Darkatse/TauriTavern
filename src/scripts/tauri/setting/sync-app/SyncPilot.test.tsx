import { act, fireEvent, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, test } from '@rstest/core';

import {
    mountTauriTavernSyncProgressApp,
    type SyncProgressHandle,
} from './SyncProgressApp';
import {
    mountTauriTavernSyncScopeApp,
    type SyncScopeDatasetCatalog,
    type SyncScopeHandle,
} from './SyncScopeApp';

declare global {
    // The mounts under test create React roots directly instead of going
    // through Testing Library's render(), so act() needs the explicit opt-in.
    var IS_REACT_ACT_ENVIRONMENT: boolean | undefined;
}
globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const tr = (key: string) => key;

const handles: Array<SyncProgressHandle | SyncScopeHandle> = [];
const containers: HTMLElement[] = [];

function createContainer(): HTMLElement {
    const container = document.createElement('div');
    document.body.append(container);
    containers.push(container);
    return container;
}

function track<T extends SyncProgressHandle | SyncScopeHandle>(handle: T): T {
    handles.push(handle);
    return handle;
}

function close(handle: SyncProgressHandle | SyncScopeHandle): void {
    act(() => handle.unmount());
    handles.splice(handles.indexOf(handle), 1);
}

afterEach(() => {
    for (const handle of handles.splice(0)) {
        close(handle);
    }
    for (const container of containers.splice(0)) {
        container.remove();
    }
});

const PROGRESS_PAYLOAD = {
    direction: 'Push',
    phase: 'Uploading',
    files_done: 12,
    files_total: 40,
    bytes_done: 1536,
    bytes_total: 10485760,
    current_path: 'characters/Alice/main.png',
};

const SCOPE_CATALOG: SyncScopeDatasetCatalog = {
    policyVersion: 3,
    supportedDatasetIds: [
        'settings.core',
        'chat.character.history',
        'chat.group.metadata',
        'agent.profiles',
        'secrets.api_keys',
        'plugin.future',
    ],
    defaultDatasetIds: ['settings.core', 'chat.character.history'],
};

function mountScope(selection?: { policy_version: number; dataset_ids: string[] } | null): {
    container: HTMLElement;
    handle: SyncScopeHandle;
} {
    const container = createContainer();
    let handle!: SyncScopeHandle;
    act(() => {
        handle = mountTauriTavernSyncScopeApp(container, {
            catalog: SCOPE_CATALOG,
            selection,
            tr,
        });
    });
    track(handle);
    return { container, handle };
}

function scopeCheckbox(container: HTMLElement, name: RegExp): HTMLInputElement {
    return within(container).getByRole<HTMLInputElement>('checkbox', { name });
}

test('sync progress mount validates its boundary arguments', () => {
    expect(() => mountTauriTavernSyncProgressApp(null, { tr }))
        .toThrow('TauriTavern Sync progress mount element is required');
    expect(() => mountTauriTavernSyncProgressApp(createContainer(), {} as never))
        .toThrow('TauriTavern Sync progress translator is required');
});

test('sync progress renders the initial state and applies partial updates', () => {
    const container = createContainer();
    let handle!: SyncProgressHandle;
    act(() => {
        handle = mountTauriTavernSyncProgressApp(container, {
            title: 'TT-Sync progress',
            payload: PROGRESS_PAYLOAD,
            tr,
        });
    });
    track(handle);

    const view = () => within(container);
    expect(container.querySelector('b')?.textContent).toBe('TT-Sync progress');
    expect(view().getByText('Phase: Push / Uploading')).toBeTruthy();
    expect(view().getByText('Files: 12/40')).toBeTruthy();
    expect(view().getByText('Bytes: 1.5 KB/10.0 MB')).toBeTruthy();
    expect(container.querySelector('.tt-sync-progress-current')?.textContent)
        .toBe('Current: characters/Alice/main.png');

    // A payload update replaces the payload only; the title persists.
    act(() => handle.update({
        payload: { phase: 'Verifying', files_done: 40, files_total: 40 },
    }));
    expect(container.querySelector('b')?.textContent).toBe('TT-Sync progress');
    expect(view().getByText('Phase: Verifying')).toBeTruthy();
    expect(view().getByText('Files: 40/40')).toBeTruthy();
    expect(view().getByText('Bytes: 0 B/0 B')).toBeTruthy();
    expect(container.querySelector('.tt-sync-progress-current')?.textContent).toBe('');

    // A title update leaves the payload untouched.
    act(() => handle.update({ title: 'LAN Sync progress' }));
    expect(container.querySelector('b')?.textContent).toBe('LAN Sync progress');
    expect(view().getByText('Phase: Verifying')).toBeTruthy();
});

test('sync progress renders defaults for an empty snapshot', () => {
    const container = createContainer();
    let handle!: SyncProgressHandle;
    act(() => {
        handle = mountTauriTavernSyncProgressApp(container, { tr });
    });
    track(handle);

    expect(container.querySelector('b')?.textContent).toBe('Sync progress');
    expect(within(container).getByText('Phase: Starting')).toBeTruthy();
    expect(within(container).getByText('Files: 0/0')).toBeTruthy();
    expect(within(container).getByText('Bytes: 0 B/0 B')).toBeTruthy();
    expect(container.querySelector('.tt-sync-progress-current')?.textContent).toBe('');
});

test('sync progress unmount clears the mount element', () => {
    const container = createContainer();
    let handle!: SyncProgressHandle;
    act(() => {
        handle = mountTauriTavernSyncProgressApp(container, { tr });
    });
    track(handle);
    expect(container.innerHTML).not.toBe('');

    close(handle);
    expect(container.innerHTML).toBe('');
});

test('sync scope mount validates its boundary arguments', () => {
    expect(() => mountTauriTavernSyncScopeApp(null, { catalog: SCOPE_CATALOG, tr }))
        .toThrow('TauriTavern Sync scope mount element is required');
    expect(() => mountTauriTavernSyncScopeApp(createContainer(), { catalog: SCOPE_CATALOG } as never))
        .toThrow('TauriTavern Sync translator is required');
});

test('sync scope renders the normalized selection, summary and the Other group', () => {
    const { container, handle } = mountScope({
        policy_version: 3,
        dataset_ids: ['agent.profiles', 'settings.core', 'agent.profiles', 'bogus'],
    });

    // Duplicates and unsupported ids are dropped; the stored order survives.
    expect(handle.getSelection()).toEqual({
        policy_version: 3,
        dataset_ids: ['agent.profiles', 'settings.core'],
    });
    expect(container.querySelector('.tt-sync-scope-summary b')?.textContent).toBe('2 / 6');

    expect(scopeCheckbox(container, /Core settings/).checked).toBe(true);
    expect(scopeCheckbox(container, /Agent profiles/).checked).toBe(true);
    expect(scopeCheckbox(container, /Group metadata/).checked).toBe(false);

    // A supported dataset unknown to the built-in groups lands in Other.
    const other = within(container).getByRole('button', { name: /^Other/ }).closest('section');
    expect(other?.textContent).toContain('plugin.future');

    // Sensitive datasets are marked, but the summary badge only appears when one is selected.
    expect(scopeCheckbox(container, /API keys/).closest('label')?.textContent).toContain('Sensitive');
    expect(container.querySelector('.tt-sync-scope-summary code')).toBeNull();
});

test('sync scope falls back to defaults when nothing stored is supported', () => {
    const { handle } = mountScope({ policy_version: 3, dataset_ids: ['bogus'] });
    expect(handle.getSelection().dataset_ids).toEqual(['settings.core', 'chat.character.history']);
});

test('sync scope presets replace the selection through supported filtering', async () => {
    const user = userEvent.setup();
    const { container, handle } = mountScope(null);

    await user.click(within(container).getByRole('button', { name: 'Chats' }));
    expect(handle.getSelection().dataset_ids).toEqual(['chat.character.history', 'chat.group.metadata']);

    await user.click(within(container).getByRole('button', { name: 'Full' }));
    expect(handle.getSelection().dataset_ids).toEqual(SCOPE_CATALOG.supportedDatasetIds);
    expect(container.querySelector('.tt-sync-scope-summary b')?.textContent).toBe('6 / 6');

    await user.click(within(container).getByRole('button', { name: 'Recommended' }));
    expect(handle.getSelection().dataset_ids).toEqual(SCOPE_CATALOG.defaultDatasetIds);
});

test('sync scope never deselects the last dataset', async () => {
    const user = userEvent.setup();
    const { container, handle } = mountScope({
        policy_version: 3,
        dataset_ids: ['agent.profiles', 'settings.core'],
    });

    await user.click(scopeCheckbox(container, /Core settings/));
    expect(handle.getSelection().dataset_ids).toEqual(['agent.profiles']);

    const last = scopeCheckbox(container, /Agent profiles/);
    await user.click(last);
    expect(handle.getSelection().dataset_ids).toEqual(['agent.profiles']);
    expect(last.checked).toBe(true);
});

test('sync scope group toggle adds missing ids in group order and removes a full group', async () => {
    const user = userEvent.setup();
    const { container, handle } = mountScope({
        policy_version: 3,
        dataset_ids: ['agent.profiles', 'settings.core'],
    });

    const coreHeader = within(container).getByRole('button', { name: /^Core/ });
    await user.click(coreHeader);
    expect(handle.getSelection().dataset_ids).toEqual([
        'agent.profiles',
        'settings.core',
        'chat.character.history',
        'chat.group.metadata',
    ]);

    await user.click(coreHeader);
    expect(handle.getSelection().dataset_ids).toEqual(['agent.profiles']);
});

test('sync scope getSelection reads the latest value and returns a fresh copy', () => {
    const { container, handle } = mountScope(null);

    // The popup reads the selection after its dialog may already be detached,
    // so the getter must reflect the user event without waiting for a commit.
    let snapshot: ReturnType<SyncScopeHandle['getSelection']> | undefined;
    act(() => {
        fireEvent.click(within(container).getByRole('button', { name: /^Agent continuity/ }));
        snapshot = handle.getSelection();
    });
    expect(snapshot).toEqual({
        policy_version: 3,
        dataset_ids: ['settings.core', 'chat.character.history', 'agent.profiles'],
    });

    snapshot?.dataset_ids.push('mutated');
    expect(handle.getSelection().dataset_ids).toHaveLength(3);
});

test('sync scope summary shows the sensitive badge once a sensitive dataset is selected', async () => {
    const user = userEvent.setup();
    const { container } = mountScope(null);
    const summary = container.querySelector('.tt-sync-scope-summary');
    expect(summary?.querySelector('code')).toBeNull();

    await user.click(scopeCheckbox(container, /API keys/));
    expect(summary?.querySelector('code')?.textContent).toBe('Sensitive');
});

test('sync scope unmount clears the mount element', () => {
    const { container, handle } = mountScope(null);
    expect(container.innerHTML).not.toBe('');

    close(handle);
    expect(container.innerHTML).toBe('');
});
