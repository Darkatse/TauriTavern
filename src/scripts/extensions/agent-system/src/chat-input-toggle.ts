import { loadSettings, patchSettings, subscribeSettings } from './settings-store';
import { subscribeAgentRunState } from '../../../tauritavern/agent/agent-run-controller.js';
import { reportAgentSystemError } from './host-api';
import { AGENT_TOGGLE_ICON } from './agent-icon';
import { translateAgentSystem as tr } from './i18n';
import { openAgentSystemPanel } from './panel-popup';
import type { AgentSystemSettings } from './settings-store';

const BUTTON_ID = 'ttas_agent_send_toggle';
const LONG_PRESS_MS = 550;
const LONG_PRESS_MOVE_TOLERANCE_PX = 10;
const LONG_PRESS_CLICK_SUPPRESS_MS = 800;

let settings: AgentSystemSettings | null = null;
let activeRun: TauriTavernAgentRunHandle | null = null;

export async function mountChatInputAgentToggle(): Promise<void> {
    const rightSendForm = document.getElementById('rightSendForm');
    const sendButton = document.getElementById('send_but');
    if (!(rightSendForm instanceof HTMLElement) || !(sendButton instanceof HTMLElement)) {
        throw new Error(tr('sendFormNotFound'));
    }

    const button = document.createElement('button');
    button.id = BUTTON_ID;
    button.type = 'button';
    button.className = 'ttas-agent-send-toggle interactable displayNone';
    button.innerHTML = `${AGENT_TOGGLE_ICON}<span class="ttas-agent-send-toggle-status" aria-hidden="true"></span>`;
    rightSendForm.insertBefore(button, sendButton);

    let longPressTimer: ReturnType<typeof setTimeout> | null = null;
    let longPressPointerId: number | null = null;
    let longPressStartX = 0;
    let longPressStartY = 0;
    let suppressClickUntil = 0;

    const clearLongPress = (): void => {
        if (longPressTimer !== null) {
            clearTimeout(longPressTimer);
            longPressTimer = null;
        }
        if (longPressPointerId !== null && button.hasPointerCapture?.(longPressPointerId)) {
            button.releasePointerCapture(longPressPointerId);
        }
        longPressPointerId = null;
    };

    const openPanel = (): void => {
        try {
            openAgentSystemPanel();
        } catch (error) {
            reportAgentSystemError(error);
        }
    };

    const syncVisibility = (): void => {
        button.classList.toggle('displayNone', sendButton.classList.contains('displayNone') || Boolean(settings?.chatInputToggleHidden));
    };
    const sendButtonObserver = new MutationObserver(syncVisibility);
    sendButtonObserver.observe(sendButton, { attributes: true, attributeFilter: ['class'] });

    const render = (): void => {
        const enabled = Boolean(settings?.agentModeEnabled);
        const label = activeRun
            ? tr('agentRunActive')
            : (enabled ? tr('agentModeOn') : tr('agentModeOff'));
        button.classList.toggle('active', enabled);
        button.classList.toggle('running', Boolean(activeRun));
        button.setAttribute('aria-pressed', String(enabled));
        button.setAttribute('aria-label', label);
        button.dataset.ttasState = activeRun ? 'running' : (enabled ? 'on' : 'off');
        button.dataset.ttasLabel = label;
        button.title = label;
    };

    button.addEventListener('pointerdown', (event) => {
        if (event.button !== 0) {
            return;
        }

        clearLongPress();
        longPressPointerId = event.pointerId;
        longPressStartX = event.clientX;
        longPressStartY = event.clientY;
        button.setPointerCapture?.(event.pointerId);
        longPressTimer = setTimeout(() => {
            longPressTimer = null;
            suppressClickUntil = Date.now() + LONG_PRESS_CLICK_SUPPRESS_MS;
            openPanel();
        }, LONG_PRESS_MS);
    });

    button.addEventListener('pointermove', (event) => {
        if (longPressPointerId !== event.pointerId || longPressTimer === null) {
            return;
        }

        const deltaX = event.clientX - longPressStartX;
        const deltaY = event.clientY - longPressStartY;
        if (Math.hypot(deltaX, deltaY) > LONG_PRESS_MOVE_TOLERANCE_PX) {
            clearLongPress();
        }
    });

    button.addEventListener('pointerup', clearLongPress);
    button.addEventListener('pointercancel', clearLongPress);
    button.addEventListener('lostpointercapture', clearLongPress);

    const handleClick = async (event: MouseEvent): Promise<void> => {
        if (Date.now() < suppressClickUntil) {
            suppressClickUntil = 0;
            event.preventDefault();
            event.stopPropagation();
            return;
        }

        try {
            settings = await patchSettings(settings || await loadSettings(), {
                agentModeEnabled: !settings?.agentModeEnabled,
            });
            render();
        } catch (error) {
            reportAgentSystemError(error);
            throw error;
        }
    };
    button.addEventListener('click', event => { void handleClick(event); });

    subscribeSettings((next) => {
        settings = next;
        syncVisibility();
        render();
    });

    subscribeAgentRunState((state) => {
        activeRun = state.activeRun;
        render();
    });

    settings = await loadSettings();
    syncVisibility();
    render();
}
