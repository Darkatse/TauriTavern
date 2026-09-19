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
import { createContentPreparation } from './content-preparation.js';
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
 *   prepareMaterializeOptions: (input: { messages: any[]; messageIds: number[] }) => Promise<Map<number, any>>;
 *   materializeMessage: (input: any) => any;
 *   formatMessageContent: (message: any, messageId: number) => string;
 *   prepareContentTransaction: (element: HTMLElement, html: string) => { content: HTMLElement; commit: () => unknown };
 *   emitEvent: (event: string, messageId: number, ...args: any[]) => Promise<void>;
 *   syncMountedViewState: (messageIds: readonly number[]) => void;
 *   onFault: (error: Error) => void;
 * }} deps
 */
export function installChatSurfaceRuntime({
    root,
    getMessages,
    prepareMaterializeOptions,
    materializeMessage,
    formatMessageContent,
    prepareContentTransaction,
    emitEvent,
    syncMountedViewState,
    onFault,
}) {
    if (!(root instanceof HTMLElement) || typeof prepareMaterializeOptions !== 'function') {
        throw new TypeError('ChatSurface install requires a root element and materialization preparer');
    }
    installFrontendSourceHandoff(root);

    /** @type {ReturnType<typeof createChatSurfaceController> | null} */
    let controller = null;
    const contentPreparation = createContentPreparation({
        getMessages,
        formatMessage: formatMessageContent,
        commit(messageId, html) {
            const element = activeController.getMessageElement(messageId);
            if (element) {
                const transaction = prepareContentTransaction(element, html);
                transaction.content.removeAttribute('aria-busy');
                activeController.commitContent(element, transaction);
            }
        },
        refresh: refreshContent,
        onFault: error => activeController.fault(error),
    });
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
        contentPreparation,
    });
    installChatSurfaceController(controller, contentPreparation);
    const activeController = controller;
    const bounded = createBoundedChatSurface({
        controller: activeController,
        domAdapter,
        getMessages,
        createVirtualAdapter: options => createTanStackVirtualAdapter({ ...options, virtualCore: libs }),
        onProjectionCommitted: syncMountedViewState,
        onFault,
    });

    /** @param {number} messageId */
    function updateContent(messageId) {
        const element = activeController.getMessageElement(messageId);
        if (element) {
            const html = formatMessageContent(getMessages()[messageId], messageId);
            activeController.updateContent(element, prepareContentTransaction(element, html));
        }
    }

    async function refreshContent() {
        const messageIds = activeController.getMountedMessageIds();
        for (const messageId of messageIds) {
            if (!contentPreparation.isTransient(getMessages()[messageId])) updateContent(messageId);
        }
        await contentPreparation.ready(messageIds);
    }

    /** @param {number} messageId @param {string | null} [event] @param {...any} args */
    async function finishContent(messageId, event = null, ...args) {
        const message = getMessages()[messageId];
        if (contentPreparation.isTransient(message)) {
            contentPreparation.setTransient(message, false);
            updateContent(messageId);
        }
        await contentPreparation.ready([messageId]);
        if (event && getMessages()[messageId] === message) await emitEvent(event, messageId, ...args);
    }

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
    async function render({
        messages = getMessages(),
        startIndex = 0,
        frontendSourceHandoffEvent = null,
    } = {}) {
        const managed = isBoundedView();
        const mountedBefore = activeController.getMountedMessageIds();
        const replaceMessageIds = mountedBefore.filter(
            messageId => messageId >= startIndex && messageId < messages.length,
        );

        if (managed) {
            const materializeOptionsByMessageId = await prepareMaterializeOptions({
                messages,
                messageIds: [...new Set([
                    ...replaceMessageIds,
                    ...bounded.materializationCandidates(messages.length),
                ])],
            });
            prepareRuntimeAdmission(managed);
            if (bounded.snapshot().state === 'inactive') {
                bounded.open({ messages, materializeOptionsByMessageId });
            } else {
                bounded.reconcile({ messages, replaceMessageIds, materializeOptionsByMessageId });
            }
            await contentPreparation.ready(activeController.getMountedMessageIds());
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
        const materializeOptionsByMessageId = await prepareMaterializeOptions({ messages, messageIds: replacements });
        prepareRuntimeAdmission(managed);
        activeController.reconcile({
            indices: nextIds,
            messages,
            replaceMessageIds: replacements,
            frontendSourceHandoffEvent,
            materializeOptionsByMessageId,
        });
        await contentPreparation.ready(activeController.getMountedMessageIds());
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
        finishContent,
        jumpToMessage,
        reconcileMounted,
        render,
        rerenderMessage,
        resetEpoch,
        scroll: activeController.scroll,
        setProjectionHeld,
        setContentTransient: contentPreparation.setTransient,
    });
}
