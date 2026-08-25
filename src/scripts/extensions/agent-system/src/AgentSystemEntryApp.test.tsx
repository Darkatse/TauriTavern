import { StrictMode } from 'react';
import { act, cleanup, render, screen, waitFor } from '@testing-library/react';
import { afterEach, expect, test } from '@rstest/core';
import userEvent from '@testing-library/user-event';

import { AgentSystemEntryApp } from './AgentSystemEntryApp';
import {
    createAgentSystemEntryController,
    type AgentSystemEntryControllerDeps,
    type AgentSystemSettings,
} from './AgentSystemEntryController';

const tr = (key: string): string => key;

function settings(overrides: Partial<AgentSystemSettings> = {}): AgentSystemSettings {
    return {
        agentModeEnabled: false,
        chatInputToggleHidden: false,
        activeProfileId: 'default-writer',
        editingProfileId: 'default-writer',
        activeTab: 'profiles',
        runTimelineHeightPx: null,
        ...overrides,
    };
}

function profile(id: string, overrides: Partial<TauriTavernAgentProfileSummary> = {}): TauriTavernAgentProfileSummary {
    return { id, displayName: id, directRunnable: true, ...overrides };
}

function createDeps(options: {
    settings?: AgentSystemSettings;
    profiles?: TauriTavernAgentProfileSummary[];
    failLoad?: boolean;
} = {}) {
    const state = {
        settings: options.settings ?? settings(),
        profiles: options.profiles ?? [profile('default-writer')],
        patches: [] as Array<Partial<AgentSystemSettings>>,
        errors: [] as unknown[],
        warnings: [] as string[],
        loadCount: 0,
        listCount: 0,
        settingsSubscribers: 0,
        profilesSubscribers: 0,
        settingsListener: null as ((next: AgentSystemSettings) => void) | null,
        profilesListener: null as (() => void) | null,
    };
    const deps: AgentSystemEntryControllerDeps = {
        loadSettings: () => {
            state.loadCount += 1;
            return options.failLoad
                ? Promise.reject(new Error('load failed'))
                : Promise.resolve(state.settings);
        },
        patchSettings: (current, patch) => {
            state.patches.push(patch);
            state.settings = { ...current, ...patch };
            return Promise.resolve(state.settings);
        },
        subscribeSettings: (listener) => {
            state.settingsSubscribers += 1;
            state.settingsListener = listener;
            return () => {
                state.settingsSubscribers -= 1;
            };
        },
        listProfiles: () => {
            state.listCount += 1;
            return Promise.resolve(state.profiles);
        },
        subscribeProfilesChanged: (listener) => {
            state.profilesSubscribers += 1;
            state.profilesListener = listener;
            return () => {
                state.profilesSubscribers -= 1;
            };
        },
        notifyError: (error) => {
            state.errors.push(error);
        },
        notifyWarning: (message) => {
            state.warnings.push(message);
        },
        tr,
    };
    return { deps, state };
}

afterEach(() => {
    cleanup();
});

test('loads settings and lists only direct-runnable profiles', async () => {
    const { deps } = createDeps({
        settings: settings({ agentModeEnabled: true }),
        profiles: [
            profile('default-writer', { displayName: 'Writer' }),
            profile('sub-agent', { directRunnable: false }),
        ],
    });
    const controller = createAgentSystemEntryController(deps);
    render(<AgentSystemEntryApp controller={controller} tr={tr} onOpenPanel={() => undefined} />);
    await act(async () => {
        await controller.init();
    });

    expect(screen.getByRole('button', { name: 'agentModeOn' }).className).toContain('active');
    const select = screen.getByRole<HTMLSelectElement>('combobox');
    expect(select.disabled).toBe(false);
    expect(select.value).toBe('default-writer');
    const options = select.querySelectorAll('option');
    expect(options).toHaveLength(1);
    expect(options.item(0).textContent).toBe('Writer');
});

test('toggles agent mode and chat input visibility through the settings store', async () => {
    const { deps, state } = createDeps();
    const controller = createAgentSystemEntryController(deps);
    const user = userEvent.setup();
    render(<AgentSystemEntryApp controller={controller} tr={tr} onOpenPanel={() => undefined} />);
    await act(async () => {
        await controller.init();
    });

    await user.click(screen.getByRole('button', { name: 'agentModeOff' }));
    await waitFor(() => expect(screen.getByRole('button', { name: 'agentModeOn' })).toBeDefined());
    expect(state.patches[0]).toEqual({ agentModeEnabled: true });

    await user.click(screen.getByRole('button', { name: 'hideChatInputToggle' }));
    await waitFor(() => expect(screen.getByRole('button', { name: 'showChatInputToggle' })).toBeDefined());
    expect(state.patches[1]).toEqual({ chatInputToggleHidden: true });
});

test('persists a newly selected direct-runnable active profile', async () => {
    const { deps, state } = createDeps({
        profiles: [profile('default-writer'), profile('reviewer')],
    });
    const controller = createAgentSystemEntryController(deps);
    const user = userEvent.setup();
    render(<AgentSystemEntryApp controller={controller} tr={tr} onOpenPanel={() => undefined} />);
    await act(async () => {
        await controller.init();
    });

    await user.selectOptions(screen.getByRole('combobox'), 'reviewer');
    await waitFor(() => expect(state.patches).toEqual([{ activeProfileId: 'reviewer' }]));
});

test('resets a missing active profile to the default with a warning', async () => {
    const { deps, state } = createDeps({
        settings: settings({ activeProfileId: 'ghost' }),
    });
    const controller = createAgentSystemEntryController(deps);
    render(<AgentSystemEntryApp controller={controller} tr={tr} onOpenPanel={() => undefined} />);
    await act(async () => {
        await controller.init();
    });

    expect(state.patches).toEqual([{ activeProfileId: 'default-writer' }]);
    expect(state.warnings).toEqual(['activeProfileResetToDefault']);
});

test('rejects an unknown or non-runnable active profile selection', async () => {
    const { deps } = createDeps({
        profiles: [profile('default-writer'), profile('sub-agent', { directRunnable: false })],
    });
    const controller = createAgentSystemEntryController(deps);
    await controller.init();

    await expect(controller.setActiveProfile('ghost')).rejects.toThrow('agentProfileNotFound');
    await expect(controller.setActiveProfile('sub-agent')).rejects.toThrow('agentProfileNotDirectRunnable');
});

test('settings and profile subscriptions update the view exactly once under StrictMode', async () => {
    const { deps, state } = createDeps();
    const controller = createAgentSystemEntryController(deps);
    render(
        <StrictMode>
            <AgentSystemEntryApp controller={controller} tr={tr} onOpenPanel={() => undefined} />
        </StrictMode>,
    );
    await act(async () => {
        await controller.init();
    });

    expect(state.loadCount).toBe(1);
    expect(state.settingsSubscribers).toBe(1);
    expect(state.profilesSubscribers).toBe(1);

    await act(async () => {
        state.settingsListener?.(settings({ agentModeEnabled: true }));
        await Promise.resolve();
    });
    expect(screen.getByRole('button', { name: 'agentModeOn' })).toBeDefined();

    await act(async () => {
        state.profilesListener?.();
        await Promise.resolve();
    });
    await waitFor(() => expect(state.listCount).toBe(2));
});

test('init failure rejects without committing and dispose blocks late work', async () => {
    const { deps, state } = createDeps({ failLoad: true });
    const controller = createAgentSystemEntryController(deps);
    render(<AgentSystemEntryApp controller={controller} tr={tr} onOpenPanel={() => undefined} />);

    let failure: unknown = null;
    await act(async () => {
        failure = await controller.init().then(
            () => null,
            (caught: unknown) => caught,
        );
    });
    expect(failure).toBeInstanceOf(Error);
    expect((failure as Error).message).toBe('load failed');
    expect(state.settingsSubscribers).toBe(0);
    expect(controller.getSnapshot().loading).toBe(false);

    const deferredProfiles: { resolve: ((profiles: TauriTavernAgentProfileSummary[]) => void) | null } = { resolve: null };
    const pendingController = createAgentSystemEntryController({
        ...deps,
        loadSettings: () => Promise.resolve(settings()),
        listProfiles: () => new Promise((resolve) => {
            deferredProfiles.resolve = resolve;
        }),
    });
    const initPromise = pendingController.init();
    await waitFor(() => expect(deferredProfiles.resolve).not.toBeNull());
    pendingController.dispose();
    deferredProfiles.resolve?.([profile('default-writer')]);
    await initPromise;

    expect(pendingController.getSnapshot().profiles).toEqual([]);
    expect(state.profilesSubscribers).toBe(0);
});

test('subscription setup failure rolls back earlier subscriptions', async () => {
    const { deps, state } = createDeps();
    deps.subscribeProfilesChanged = () => {
        throw new Error('profile subscription failed');
    };
    const controller = createAgentSystemEntryController(deps);

    await expect(controller.init()).rejects.toThrow('profile subscription failed');

    expect(state.settingsSubscribers).toBe(0);
    expect(state.profilesSubscribers).toBe(0);
});
