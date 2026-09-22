import { tr } from './i18n';

export function installAssistantDrawer() {
    const panel = document.getElementById('AdvancedFormatting');
    const toggle = document.querySelector<HTMLElement>('#advanced-formatting-button .drawer-toggle');
    const icon = toggle?.querySelector<HTMLElement>('.drawer-icon');
    if (!panel || !toggle || !icon) throw new Error('in-app-agent: A drawer is unavailable');
    const mount = document.createElement('div');
    mount.id = 'tt-assistant';
    panel.prepend(mount);
    let mode: 'assistant' | 'advanced' = 'assistant';
    let state = { open: panel.classList.contains('openDrawer'), assistant: true };
    const listeners = new Set<() => void>();
    panel.dataset.ttAssistant = mode;

    const update = () => {
        const open = panel.classList.contains('openDrawer');
        icon.setAttribute('aria-expanded', String(open));
        icon.setAttribute('aria-label', tr(mode === 'assistant' ? 'title' : 'advanced'));
        if (state.open === open && state.assistant === (mode === 'assistant')) return;
        state = { open, assistant: mode === 'assistant' };
        listeners.forEach(listener => listener());
    };
    const observer = new MutationObserver(update);
    observer.observe(panel, { attributes: true, attributeFilter: ['class'] });
    // Enter is already handled by SillyTavern's keyboard adapter.
    const onSpace = (event: KeyboardEvent) => {
        if (event.key !== ' ') return;
        event.preventDefault(); event.stopPropagation(); toggle.click();
    };
    icon.addEventListener('keydown', onSpace);
    icon.setAttribute('aria-controls', panel.id);
    update();
    return {
        mount,
        getSnapshot: () => state,
        subscribe(listener: () => void) { listeners.add(listener); return () => { listeners.delete(listener); }; },
        setMode(next: typeof mode) { mode = next; panel.dataset.ttAssistant = mode; update(); },
        close() { if (panel.classList.contains('openDrawer')) toggle.click(); },
        setActivity(activity: 'working' | 'unread' | '') {
            icon.dataset.ttAssistantActivity = activity;
            icon.title = tr(activity === 'working' ? 'workingBadge' : activity === 'unread' ? 'newReply' : 'title');
        },
        dispose() {
            observer.disconnect(); icon.removeEventListener('keydown', onSpace);
            listeners.clear(); mount.remove(); delete panel.dataset.ttAssistant; delete icon.dataset.ttAssistantActivity;
        },
    };
}
export type AssistantDrawer = ReturnType<typeof installAssistantDrawer>;
