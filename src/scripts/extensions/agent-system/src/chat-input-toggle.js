import { loadSettings, patchSettings, subscribeSettings } from './settings-store.js';
import { subscribeAgentRunState } from '../../../tauritavern/agent/agent-run-controller.js';
import { errorText } from './host-api.js';
import { AGENT_TOGGLE_ICON } from './agent-icon.js';
import { translateAgentSystem as tr } from './i18n.js';
import { openAgentSystemPanel } from './panel-popup.js';

const BUTTON_ID = 'ttas_agent_send_toggle';
const LONG_PRESS_MS = 550;
const LONG_PRESS_MOVE_TOLERANCE_PX = 10;
const LONG_PRESS_CLICK_SUPPRESS_MS = 800;

let settings = null;
let activeRun = null;

function reportInteractionError(error) {
    console.error('[AgentSystem]', error);
    window.toastr?.error?.(errorText(error));
}

export async function mountChatInputAgentToggle() {
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

    let longPressTimer = null;
    let longPressPointerId = null;
    let longPressStartX = 0;
    let longPressStartY = 0;
    let suppressClickUntil = 0;

    const clearLongPress = () => {
        if (longPressTimer !== null) {
            clearTimeout(longPressTimer);
            longPressTimer = null;
        }
        if (longPressPointerId !== null && button.hasPointerCapture?.(longPressPointerId)) {
            button.releasePointerCapture(longPressPointerId);
        }
        longPressPointerId = null;
    };

    const openPanel = () => {
        try {
            const result = openAgentSystemPanel();
            result?.catch?.(reportInteractionError);
        } catch (error) {
            reportInteractionError(error);
        }
    };

    const syncVisibility = () => {
        button.classList.toggle('displayNone', sendButton.classList.contains('displayNone'));
    };
    const sendButtonObserver = new MutationObserver(syncVisibility);
    sendButtonObserver.observe(sendButton, { attributes: true, attributeFilter: ['class'] });

    const render = () => {
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

    button.addEventListener('click', async (event) => {
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
            reportInteractionError(error);
            throw error;
        }
    });

    subscribeSettings((next) => {
        settings = next;
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
