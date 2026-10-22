import { tr } from './i18n';

export type DrawerState = {
    isTopLevelDrawerOpen: (panel: HTMLElement) => boolean;
    subscribeDrawerState: (panel: HTMLElement, listener: () => void) => () => void;
};

export function installAssistantDrawer(drawers: DrawerState) {
    const panel = document.getElementById('AdvancedFormatting');
    const toggle = document.querySelector<HTMLElement>('#advanced-formatting-button .drawer-toggle');
    const icon = toggle?.querySelector<HTMLElement>('.drawer-icon');
    if (!panel || !toggle || !icon) throw new Error('in-app-agent: A drawer is unavailable');
    const mount = document.createElement('div');
    mount.id = 'tt-assistant';
    panel.prepend(mount);
    let mode: 'assistant' | 'advanced' = 'assistant';
    let state = { open: drawers.isTopLevelDrawerOpen(panel), assistant: true };
    const listeners = new Set<() => void>();
    panel.dataset.ttAssistant = mode;

    const update = () => {
        const open = drawers.isTopLevelDrawerOpen(panel);
        icon.setAttribute('aria-label', tr(mode === 'assistant' ? 'title' : 'advanced'));
        if (state.open === open && state.assistant === (mode === 'assistant')) return;
        state = { open, assistant: mode === 'assistant' };
        listeners.forEach(listener => listener());
    };
    const unsubscribe = drawers.subscribeDrawerState(panel, update);
    update();
    return {
        mount,
        getSnapshot: () => state,
        subscribe(listener: () => void) { listeners.add(listener); return () => { listeners.delete(listener); }; },
        setMode(next: typeof mode) { mode = next; panel.dataset.ttAssistant = mode; update(); },
        close() { if (drawers.isTopLevelDrawerOpen(panel)) toggle.click(); },
        setActivity(activity: 'working' | 'unread' | '') {
            icon.dataset.ttAssistantActivity = activity;
            icon.title = tr(activity === 'working' ? 'workingBadge' : activity === 'unread' ? 'newReply' : 'title');
        },
        dispose() {
            unsubscribe();
            listeners.clear(); mount.remove(); delete panel.dataset.ttAssistant; delete icon.dataset.ttAssistantActivity;
        },
    };
}
export type AssistantDrawer = ReturnType<typeof installAssistantDrawer>;
