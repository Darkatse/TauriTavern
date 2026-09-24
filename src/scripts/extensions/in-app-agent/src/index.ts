import { createInAppAgentController } from './controller';
import { registerAssistantTools } from './tools';
import { createElement } from 'react';
import { createRoot } from 'react-dom/client';
import { AssistantApp } from './AssistantApp';
import { installAssistantDrawer } from './drawer';
import { assistantSelection, createAssistantActions, requireContext } from './host';
import { createInteractionCursor } from './ui/cursor';

const host = window.__TAURITAVERN__;
if (host?.ready == null) throw new Error('in-app-agent: Host readiness is unavailable');
await host.ready;
const api = host.api;
if (!api?.agent || !api.dev || !api.extension) throw new Error('in-app-agent: required Host APIs are unavailable');
const hostApi: TauriTavernHostApi = api;
const { agent, dev } = api;
const context = requireContext();
const controller = createAssistantController();
const cursor = createInteractionCursor(() => context.Popup.util.getTopmostModalLayer());
const unsubscribeCursor = controller.subscribe(() => {
    const { activeRun, sendingSessionId } = controller.getSnapshot();
    if (!activeRun && sendingSessionId === undefined) cursor.clear();
});
let pageReady = false;
await registerAssistantTools(api, agent.tools, dev.frontendLogs, () => pageReady, (point, toolContext) => {
    const { activeRun, sendingSessionId } = controller.getSnapshot();
    // Only the task owned by this assistant drives its cursor, including the send admission window.
    if (activeRun?.runId === toolContext.runId
        || (toolContext.target.kind === 'session' && sendingSessionId === toolContext.target.sessionId)) {
        cursor.move(point);
    }
});

export function createAssistantController() {
    return createInAppAgentController({ agent, selection: assistantSelection });
}

context.eventSource.once(context.eventTypes.APP_READY, () => {
    pageReady = true;
    void mount().catch(error => {
        console.error('[InAppAssistant] Failed to mount assistant', error);
        window.toastr?.error?.(error instanceof Error ? error.message : String(error));
    });
});
async function mount() {
    const actions = await createAssistantActions(hostApi, context);
    const drawer = installAssistantDrawer();
    const root = createRoot(drawer.mount);
    root.render(createElement(AssistantApp, { controller, actions, drawer }));
    window.addEventListener('pagehide', () => {
        unsubscribeCursor(); cursor.clear();
        root.unmount(); controller.dispose(); drawer.dispose();
    }, { once: true });
}
