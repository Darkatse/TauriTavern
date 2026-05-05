import { loadSettings, patchSettings, subscribeSettings } from './settings-store.js';
import { subscribeAgentRunState } from '../../../tauritavern/agent/agent-run-controller.js';
import { errorText } from './host-api.js';
import { translateAgentSystem as tr } from './i18n.js';
import { openAgentSystemPanel } from './panel-popup.js';

const BUTTON_ID = 'ttas_agent_send_toggle';
const LONG_PRESS_MS = 550;
const LONG_PRESS_MOVE_TOLERANCE_PX = 10;
const LONG_PRESS_CLICK_SUPPRESS_MS = 800;
const AGENT_TOGGLE_ICON = `
<svg class="ttas-agent-send-toggle-icon" width="14" height="14" viewBox="0 0 14 14" fill="none" xmlns="http://www.w3.org/2000/svg" aria-hidden="true" focusable="false">
    <path d="M7.06431 5.93342C7.68763 5.93342 8.19307 6.43904 8.19322 7.06233C8.19322 7.68573 7.68772 8.19123 7.06431 8.19123C6.44099 8.19113 5.9354 7.68567 5.9354 7.06233C5.93555 6.43911 6.44108 5.93353 7.06431 5.93342Z" fill="currentColor"></path>
    <path fill-rule="evenodd" clip-rule="evenodd" d="M8.6815 0.963693C10.1169 0.447019 11.6266 0.374829 12.5633 1.31135C13.5 2.24805 13.4277 3.75776 12.911 5.19319C12.7126 5.74431 12.4386 6.31796 12.0965 6.89729C12.4969 7.54638 12.8141 8.19018 13.036 8.80647C13.5527 10.2419 13.6251 11.7516 12.6883 12.6883C11.7516 13.625 10.242 13.5527 8.8065 13.036C8.19022 12.8141 7.54641 12.4969 6.89732 12.0965C6.31797 12.4386 5.74435 12.7125 5.19322 12.911C3.75777 13.4276 2.2481 13.5 1.31138 12.5633C0.374859 11.6266 0.447049 10.1168 0.963724 8.68147C1.17185 8.10338 1.46321 7.50063 1.82896 6.8924C1.52182 6.35711 1.27235 5.82825 1.08872 5.31819C0.572068 3.88278 0.499714 2.37306 1.43638 1.43635C2.37308 0.499655 3.8828 0.572044 5.31822 1.08869C5.82828 1.27232 6.35715 1.5218 6.89243 1.82893C7.50066 1.46318 8.10341 1.17181 8.6815 0.963693ZM11.3573 8.01154C10.9083 8.62253 10.3901 9.22873 9.80943 9.8094C9.22877 10.3901 8.62255 10.9083 8.01158 11.3572C8.4257 11.5841 8.8287 11.7688 9.21275 11.9071C10.5456 12.3868 11.4246 12.2547 11.8397 11.8397C12.2548 11.4246 12.3869 10.5456 11.9071 9.21272C11.7688 8.82866 11.5841 8.42568 11.3573 8.01154ZM2.56529 8.02912C2.37344 8.39322 2.21495 8.74796 2.09263 9.08772C1.61291 10.4204 1.74512 11.2995 2.16001 11.7147C2.57505 12.1297 3.45415 12.2618 4.78697 11.7821C5.11057 11.6656 5.44786 11.5164 5.7938 11.3367C5.249 10.9223 4.70922 10.4533 4.19029 9.9344C3.57578 9.31987 3.03169 8.67633 2.56529 8.02912ZM6.90708 3.2469C6.24065 3.70479 5.5646 4.26321 4.91392 4.91389C4.26325 5.56456 3.70482 6.24063 3.24693 6.90705C3.72674 7.63325 4.32777 8.37459 5.03892 9.08576C5.64943 9.69627 6.28183 10.2265 6.90806 10.6678C7.59368 10.2025 8.2908 9.63076 8.96079 8.96076C9.6308 8.29075 10.2025 7.59366 10.6678 6.90803C10.2265 6.2818 9.69631 5.6494 9.08579 5.03889C8.37462 4.32773 7.63328 3.72672 6.90708 3.2469ZM11.7147 2.15998C11.2996 1.74509 10.4204 1.61288 9.08775 2.0926C8.74835 2.21479 8.39382 2.37271 8.03013 2.56428C8.67728 3.03065 9.31995 3.5758 9.93443 4.19026C10.4534 4.7092 10.9223 5.24896 11.3368 5.79377C11.5164 5.44785 11.6656 5.11052 11.7821 4.78694C12.2618 3.45416 12.1297 2.57502 11.7147 2.15998ZM4.91197 2.2176C3.57922 1.73788 2.70004 1.86995 2.28501 2.28498C1.87001 2.70003 1.73791 3.5792 2.21763 4.91194C2.31709 5.18822 2.44112 5.47427 2.58677 5.7674C3.01931 5.1887 3.51474 4.6158 4.06529 4.06526C4.61584 3.5147 5.18872 3.01928 5.76743 2.58674C5.47431 2.4411 5.18824 2.31706 4.91197 2.2176Z" fill="currentColor"></path>
</svg>`;

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
