// @ts-check

import libs from '../../../../lib.js';
import { createChatDomAdapter } from '../../adapters/chat-surface/chat-dom-adapter.js';
import { createChatScrollAdapter } from '../../adapters/chat-surface/chat-scroll-adapter.js';
import { installFrontendSourceHandoff } from '../../adapters/chat-surface/frontend-source-handoff.js';
import { createTanStackVirtualAdapter } from '../../adapters/chat-surface/tanstack-virtual-adapter.js';
import { DYNAMIC_THEME_CHANGED_EVENT } from '../dynamic-theme/constants.js';
import { createBoundedChatSurface } from './bounded-chat-surface.js';
import { createChatSurfaceController } from './chat-surface-controller.js';
import { isChatVirtualizationEnabled } from './chat-virtualization-state.js';
import {
    getChatSurfaceParticipantRegistry,
    installChatSurfaceController,
} from './runtime.js';

export const CHAT_LAYOUT_CHANGED_EVENT = 'sillytavern:chat-layout-changed';

/**
 * Concrete ChatSurface composition root for the SillyTavern frontend.
 *
 * @param {{
 *   root: HTMLElement;
 *   getMessages: () => any[];
 *   materializeMessage: (input: any) => any;
 *   syncMountedViewState: (messageIds: readonly number[]) => void;
 *   onFault: (error: Error) => void;
 * }} deps
 */
export function installChatSurfaceRuntime({
    root,
    getMessages,
    materializeMessage,
    syncMountedViewState,
    onFault,
}) {
    if (!(root instanceof HTMLElement)) {
        throw new TypeError('ChatSurface install requires a root element');
    }
    installFrontendSourceHandoff(root);

    /** @type {ReturnType<typeof createChatSurfaceController> | null} */
    let controller = null;
    const domAdapter = createChatDomAdapter({
        root,
        guardUnauthorizedMutations: false,
        onUnauthorizedMutation: error => controller?.fault(error),
        onExternalRemoval: elements => controller?.reconcileExternalRemovals(elements),
    });
    const scrollAdapter = createChatScrollAdapter(root, {
        animateTop(top, duration) {
            const host = /** @type {any} */ (globalThis);
            const jquery = host.jQuery ?? host.$;
            if (typeof jquery !== 'function') {
                throw new Error('ChatSurface animated scroll requires jQuery');
            }
            jquery(root).animate({ scrollTop: top }, duration);
        },
    });
    controller = createChatSurfaceController({
        getMessages,
        materializeMessage,
        domAdapter,
        scrollAdapter,
        participantRegistry: getChatSurfaceParticipantRegistry(),
    });
    installChatSurfaceController(controller);
    const activeController = controller;
    const bounded = createBoundedChatSurface({
        controller: activeController,
        domAdapter,
        getMessages,
        createVirtualAdapter: options => createTanStackVirtualAdapter({ ...options, virtualCore: libs }),
        onProjectionCommitted: syncMountedViewState,
        onFault,
    });

    let lastLayoutWidth = root.clientWidth;
    let layoutFrame = 0;
    const scheduleLayoutRefresh = () => {
        if (layoutFrame) {
            return;
        }
        layoutFrame = requestAnimationFrame(() => {
            layoutFrame = 0;
            lastLayoutWidth = root.clientWidth;
            const { state } = bounded.snapshot();
            if (state === 'settled' || state === 'gesture-scrolling') {
                bounded.refreshLayoutMetrics();
            }
        });
    };
    window.addEventListener(CHAT_LAYOUT_CHANGED_EVENT, scheduleLayoutRefresh);
    window.addEventListener(DYNAMIC_THEME_CHANGED_EVENT, scheduleLayoutRefresh);
    window.addEventListener('resize', () => {
        if (root.clientWidth !== lastLayoutWidth) {
            scheduleLayoutRefresh();
        }
    }, { passive: true });
    document.fonts?.addEventListener?.('loadingdone', scheduleLayoutRefresh);

    function isBoundedView() {
        return isChatVirtualizationEnabled()
            && root.querySelector(':scope > .welcomePanel') === null;
    }

    /** @param {boolean} managed */
    function prepareRuntimeAdmission(managed) {
        if (activeController.getMountedMessageIds().length === 0) {
            activeController.configureRuntimeAdmission({ mode: managed ? 'managed' : 'eager' });
        }
    }

    /**
     * @param {{ messages?: any[]; startIndex?: number; frontendSourceHandoffEvent?: string | null }} [options]
     */
    function render({
        messages = getMessages(),
        startIndex = 0,
        frontendSourceHandoffEvent = null,
    } = {}) {
        const managed = isBoundedView();
        prepareRuntimeAdmission(managed);
        const mountedBefore = activeController.getMountedMessageIds();
        const replaceMessageIds = mountedBefore.filter(messageId => messageId >= startIndex);

        if (managed) {
            if (bounded.snapshot().state === 'inactive') {
                bounded.open({ messages });
            } else {
                bounded.reconcile({ messages, replaceMessageIds });
            }
            return Object.freeze({
                bounded: true,
                mountedCount: activeController.getMountedMessageIds().length,
                replaceMessageIds,
            });
        }

        const mountedIds = activeController.getMountedMessageIds();
        const nextIds = mountedIds.length === 0
            ? Array.from({ length: Math.max(0, messages.length - startIndex) }, (_value, index) => startIndex + index)
            : mountedIds.filter(messageId => messageId < messages.length);
        const replacements = nextIds.filter(messageId => messageId >= startIndex);
        activeController.reconcile({
            indices: nextIds,
            messages,
            replaceMessageIds: replacements,
            frontendSourceHandoffEvent,
        });
        return Object.freeze({
            bounded: false,
            replaceMessageIds: replacements,
        });
    }

    function reconcileMounted(options = {}) {
        const managed = isBoundedView();
        prepareRuntimeAdmission(managed);
        return managed ? bounded.reconcile(options) : activeController.reconcileMounted(options);
    }

    /** @param {{ includeAuxiliary?: boolean }} [options] */
    function resetEpoch({ includeAuxiliary = false } = {}) {
        return isChatVirtualizationEnabled()
            ? bounded.resetEpoch({ includeAuxiliary })
            : activeController.resetEpoch({ includeAuxiliary });
    }

    /** @param {number} messageId */
    function rerenderMessage(messageId) {
        if (!activeController.getMessageElement(messageId)) {
            return null;
        }
        if (isBoundedView()) {
            bounded.reconcile({ replaceMessageIds: [messageId] });
        } else {
            activeController.remountMessage(messageId);
        }
        return activeController.getMessageElement(messageId);
    }

    /** @param {number} messageId */
    function jumpToMessage(messageId) {
        if (!isBoundedView()) {
            throw new Error('Bounded ChatSurface is not active');
        }
        return bounded.jumpToMessage(messageId);
    }

    /** @param {boolean} held */
    function setProjectionHeld(held) {
        if (isBoundedView() && bounded.snapshot().state !== 'inactive') {
            bounded.setProjectionHeld(held);
        }
    }

    return Object.freeze({
        enableMutationGuard: () => activeController.setMutationGuardEnabled(true),
        getMessageElement: activeController.getMessageElement,
        getMountedMessageIds: activeController.getMountedMessageIds,
        isBoundedView,
        jumpToMessage,
        reconcileMounted,
        render,
        rerenderMessage,
        resetEpoch,
        scroll: activeController.scroll,
        setProjectionHeld,
    });
}
