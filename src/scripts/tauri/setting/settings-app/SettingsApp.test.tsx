import { act, fireEvent, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, test } from '@rstest/core';

import { mountTauriTavernSettingsApp } from './SettingsApp';
import type {
    SettingsActions,
    SettingsHandle,
    SettingsMountOptions,
    SettingsValues,
} from './SettingsContract';

declare global {
    // The mount under test creates its React root directly instead of going
    // through Testing Library's render(), so act() needs the explicit opt-in.
    var IS_REACT_ACT_ENVIRONMENT: boolean | undefined;
}
globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const tr = (key: string) => key;

const handles: SettingsHandle[] = [];
const containers: HTMLElement[] = [];

afterEach(() => {
    for (const handle of handles.splice(0)) {
        act(() => handle.unmount());
    }
    for (const container of containers.splice(0)) {
        container.remove();
    }
});

function close(handle: SettingsHandle): void {
    act(() => handle.unmount());
    handles.splice(handles.indexOf(handle), 1);
}

type ValuesOverrides = {
    chatBackups?: Partial<SettingsValues['chatBackups']>;
    requestProxy?: Partial<SettingsValues['requestProxy']>;
    dynamicTheme?: Partial<SettingsValues['dynamicTheme']>;
};

/** Mirrors the normalized shape produced by setting-panel/settings-state.js. */
function createValues(overrides: ValuesOverrides = {}): SettingsValues {
    return {
        panelRuntimeProfile: 'off',
        embeddedRuntimeProfile: 'off',
        chatVirtualizationEnabled: false,
        codeMirrorEditorEnabled: false,
        chatBackups: {
            automaticEnabled: true,
            zstdCompressionEnabled: false,
            maxFilesPerPrefix: 20,
            maxTotalFiles: 500,
            maxTotalBytes: 1536 * 1024 * 1024,
            ...overrides.chatBackups,
        },
        closeToTrayOnClose: false,
        requestProxy: { enabled: false, url: '', bypass: [], ...overrides.requestProxy },
        allowKeysExposure: false,
        avatarPersonaOriginalImagesEnabled: false,
        nativeRegexBackendEnabled: true,
        dynamicTheme: {
            themeEnabled: false,
            dayTheme: '',
            nightTheme: '',
            wallpaperEnabled: false,
            dayWallpaper: '',
            nightWallpaper: '',
            ...overrides.dynamicTheme,
        },
        promptCacheTtl: 'off',
    };
}

function createActions(overrides: Partial<SettingsActions> = {}): SettingsActions {
    return {
        chooseDataRoot: () => Promise.resolve(null),
        chooseWallpaper: () => Promise.resolve(null),
        showHelp: () => Promise.resolve(),
        manageQuickAccess: () => Promise.resolve(),
        reloadFrontend: () => Promise.resolve(),
        openFrontendLogs: () => Promise.resolve(),
        openBackendLogs: () => Promise.resolve(),
        openLlmApiLogs: () => Promise.resolve(),
        openSync: () => Promise.resolve(),
        ...overrides,
    };
}

function createOptions(overrides: Partial<SettingsMountOptions> = {}): SettingsMountOptions {
    return {
        viewModel: {
            capabilities: {
                requestProxyAllowed: true,
                lanSyncAllowed: true,
                supportsCloseToTrayOnClose: true,
                supportsDataRootSelection: true,
            },
            values: createValues(),
            dataRoot: {
                currentDataRoot: '/data/root',
                configuredDataRoot: '/data/root',
                migrationPending: false,
                migrationError: '',
            },
            chatBackupStorageStats: null,
        },
        themeOptions: [
            { value: 'Default', label: 'Default' },
            { value: 'Dark', label: 'Dark' },
        ],
        backgroundOptions: [
            { value: 'bedroom.png', label: 'bedroom.png', thumbnailUrl: '/thumb/bedroom.png', isAnimated: false },
        ],
        currentBackground: 'bedroom.png',
        actions: createActions(),
        tr,
        ...overrides,
    };
}

function mountApp(options: SettingsMountOptions = createOptions()): { container: HTMLElement; handle: SettingsHandle } {
    const container = document.createElement('div');
    document.body.append(container);
    containers.push(container);
    let handle!: SettingsHandle;
    act(() => {
        handle = mountTauriTavernSettingsApp(container, options);
    });
    handles.push(handle);
    return { container, handle };
}

function disclosure(container: HTMLElement, summaryText: string): HTMLDetailsElement {
    const summary = Array.from(container.querySelectorAll('summary'))
        .find(element => element.textContent?.includes(summaryText));
    const details = summary?.closest('details');
    if (!details) {
        throw new Error(`disclosure not found: ${summaryText}`);
    }
    return details;
}

test('settings mount enforces its public boundary and unmounts the root', () => {
    expect(() => mountTauriTavernSettingsApp(null, createOptions()))
        .toThrow('TauriTavern settings mount element is required');

    const options = createOptions();
    const partialActions: Partial<SettingsActions> = createActions();
    delete partialActions.chooseWallpaper;
    expect(() => mountTauriTavernSettingsApp(document.createElement('div'), {
        viewModel: options.viewModel,
        actions: partialActions,
        tr,
    })).toThrow('TauriTavern settings action is unavailable: chooseWallpaper');

    const { container, handle } = mountApp();
    expect(container.innerHTML).not.toBe('');

    close(handle);
    expect(container.innerHTML).toBe('');
});

test('draft preserves stored emptiness and exposes synchronous isolated edits', () => {
    const options = createOptions();
    options.viewModel.values = createValues({
        dynamicTheme: { dayWallpaper: ' Day.png' },
    });
    const { container, handle } = mountApp(options);
    let snapshot = handle.getDraft();
    expect(snapshot.dynamicTheme.dayTheme).toBe('');
    expect(snapshot.dynamicTheme.nightTheme).toBe('');
    expect(snapshot.dynamicTheme.dayWallpaper).toBe(' Day.png');

    act(() => {
        fireEvent.change(within(container).getByRole('combobox', { name: 'Panel Runtime' }), {
            target: { value: 'aggressive' },
        });
        fireEvent.click(within(container).getByRole('checkbox', { name: 'Automatic Chat Backups' }));
        snapshot = handle.getDraft();
    });
    expect(snapshot.panelRuntimeProfile).toBe('aggressive');
    expect(snapshot.chatBackups.automaticEnabled).toBe(false);

    snapshot.chatBackups.maxTotalFiles = 'mutated';
    expect(handle.getDraft().chatBackups.maxTotalFiles).toBe('500');
});

test('number inputs keep raw edit strings and the unit switch stays byte-equivalent', async () => {
    const user = userEvent.setup();
    const { container, handle } = mountApp();
    const view = within(container);

    // 1536 MiB renders as 1.5 GiB.
    expect(handle.getDraft().chatBackups.maxTotalUnit).toBe('GiB');
    expect(handle.getDraft().chatBackups.maxTotalValue).toBe('1.5');
    await user.selectOptions(view.getByRole('combobox', { name: 'Storage unit' }), 'MiB');
    expect(handle.getDraft().chatBackups.maxTotalValue).toBe('1536');

    // Clearing a limit keeps the empty string; it must never collapse to 0.
    const totalFiles = view.getByRole('spinbutton', { name: 'Total backup files' });
    await user.clear(totalFiles);
    expect(handle.getDraft().chatBackups.maxTotalFiles).toBe('');
});

test('injected storage stats update the compression hint; late stats after unmount are ignored', () => {
    const options = createOptions();
    options.viewModel.values = createValues({ chatBackups: { zstdCompressionEnabled: true } });
    const { container, handle } = mountApp(options);
    const hint = () => container.querySelector('.tt-settings-hint-accent');
    expect(hint()).toBeNull();

    act(() => handle.setChatBackupStorageStats({ originalBytes: 2048 * 1024, storedBytes: 512 * 1024 }));
    expect(hint()?.textContent).toBe('1.5 MB');
    expect(container.textContent).toContain('25%');

    close(handle);
    act(() => handle.setChatBackupStorageStats({ originalBytes: 1, storedBytes: 0 }));
    expect(container.innerHTML).toBe('');
});

test('dynamic appearance keeps independent summary state and enabling theme focuses Day Theme', async () => {
    const user = userEvent.setup();
    const { container, handle } = mountApp();
    const details = disclosure(container, 'Dynamic Theme & Wallpaper');
    // The summary meta shows live state instead of a click hint.
    expect(details.querySelector('.tt-settings-summary-meta small')?.textContent).toBe('Off · Off');

    await user.click(within(container).getByRole('checkbox', { name: 'Enable Theme Switching' }));
    expect(details.open).toBe(true);
    expect(document.activeElement).toBe(within(container).getByRole('combobox', { name: 'Day Theme' }));
    expect(handle.getDraft().dynamicTheme.dayTheme).toBe('Default');
    expect(handle.getDraft().dynamicTheme.nightTheme).toBe('Default');
    expect(details.querySelector('.tt-settings-summary-meta small')?.textContent).toBe('Default / Default · Off');

    await user.click(within(container).getByRole('checkbox', { name: 'Enable Wallpaper Switching' }));
    expect(details.querySelector('.tt-settings-summary-meta small')?.textContent).toBe('Default / Default · Enabled');
});

test('iOS proxy repair path: an enabled proxy can be disabled but not re-enabled', async () => {
    const user = userEvent.setup();
    const options = createOptions();
    options.viewModel.capabilities = {
        requestProxyAllowed: false,
        lanSyncAllowed: false,
        supportsCloseToTrayOnClose: false,
        supportsDataRootSelection: false,
    };
    options.viewModel.values = createValues({
        requestProxy: { enabled: true, url: 'http://127.0.0.1:7890', bypass: ['localhost'] },
    });
    const { container, handle } = mountApp(options);

    // The summary meta shows the live proxy URL instead of a click hint.
    const details = disclosure(container, 'Request Proxy');
    expect(details.querySelector('.tt-settings-summary-meta small')?.textContent).toBe('http://127.0.0.1:7890');

    const toggle = within(container).getByRole<HTMLInputElement>('checkbox', { name: 'Enable Request Proxy' });
    expect(toggle.disabled).toBe(false);
    expect(within(container).getByRole<HTMLInputElement>('textbox', { name: 'Request Proxy URL' }).disabled).toBe(true);

    // After disabling, the disclosure disappears entirely — the proxy cannot
    // be re-enabled because there is no toggle left to click.
    await user.click(toggle);
    expect(handle.getDraft().requestProxy.enabled).toBe(false);
    expect(within(container).queryByRole('checkbox', { name: 'Enable Request Proxy' })).toBeNull();
    expect(container.textContent).not.toContain('Request Proxy');
});

test('unknown stored theme stays selectable and wallpaper choosing preserves raw filenames', async () => {
    const user = userEvent.setup();
    let requested = '';
    const options = createOptions({
        actions: createActions({
            chooseWallpaper: ({ currentValue }: { currentValue: string }) => {
                requested = currentValue;
                return Promise.resolve('new bg.png');
            },
        }),
    });
    options.viewModel.values = createValues({
        dynamicTheme: {
            themeEnabled: true,
            dayTheme: 'Custom Unknown Theme',
            nightTheme: 'Dark',
            wallpaperEnabled: true,
            dayWallpaper: ' Day.png',
        },
    });
    const { container, handle } = mountApp(options);

    const dayTheme = within(container).getByRole<HTMLSelectElement>('combobox', { name: 'Day Theme' });
    expect(dayTheme.querySelector('option[value="Custom Unknown Theme"]')).toBeTruthy();
    expect(dayTheme.value).toBe('Custom Unknown Theme');

    await user.click(within(container).getByRole('button', { name: 'Day.png' }));
    expect(requested).toBe(' Day.png');
    expect(handle.getDraft().dynamicTheme.dayWallpaper).toBe('new bg.png');
});

test('choosing a data root projects busy state and the pending migration result', async () => {
    const user = userEvent.setup();
    let resolvePick!: (value: string | null) => void;
    const options = createOptions({
        actions: createActions({
            chooseDataRoot: () => new Promise<string | null>(resolve => {
                resolvePick = resolve;
            }),
        }),
    });
    const { container } = mountApp(options);
    const dataRootDisclosure = disclosure(container, 'Data Directory');
    dataRootDisclosure.open = true;
    fireEvent(dataRootDisclosure, new Event('toggle'));

    const chooseButton = within(container).getByRole<HTMLButtonElement>('button', { name: 'Choose...' });
    await user.click(chooseButton);
    expect(chooseButton.disabled).toBe(true);

    await act(async () => {
        resolvePick('/new/root');
        await Promise.resolve();
    });
    expect(container.querySelector('.tt-settings-status')?.textContent)
        .toContain('Configured data directory: /new/root');
    expect(chooseButton.disabled).toBe(false);
    expect(container.textContent).toContain('Data directory migration is pending.');
});
