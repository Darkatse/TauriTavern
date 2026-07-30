// @ts-check

import { createBoundedProjectionLayout } from '../../kernel/chat-surface/projection-layout.js';
import { CHAT_VIRTUAL_MAX_VIEWPORT_ITEMS } from '../../kernel/chat-surface/virtualization-config.js';

/**
 * Policy service for a `V ∪ T` chat. TanStack supplies geometry; the core
 * controller remains the only owner of message roots and participant leases.
 *
 * @param {{
 *   controller: ReturnType<import('./chat-surface-controller.js').createChatSurfaceController>;
 *   domAdapter: ReturnType<import('../../adapters/chat-surface/chat-dom-adapter.js').createChatDomAdapter>;
 *   getMessages: () => any[];
 *   createVirtualAdapter: (input: { root: HTMLElement; scrollToFn: import('@tanstack/virtual-core').VirtualizerOptions<HTMLElement, HTMLElement>['scrollToFn']; onGeometryChange: (change: { scrolling: boolean; programmatic: boolean }) => void }) => ReturnType<import('../../adapters/chat-surface/tanstack-virtual-adapter.js').createTanStackVirtualAdapter>;
 *   onProjectionCommitted: (messageIds: readonly number[]) => void;
 *   onFault: (error: Error) => void;
 * }} deps
 */
export function createBoundedChatSurface({
    controller,
    domAdapter,
    getMessages,
    createVirtualAdapter,
    onProjectionCommitted,
    onFault,
}) {
    if (
        !controller
        || !domAdapter
        || typeof getMessages !== 'function'
        || typeof createVirtualAdapter !== 'function'
        || typeof onProjectionCommitted !== 'function'
        || typeof onFault !== 'function'
    ) {
        throw new TypeError('Bounded ChatSurface requires controller, DOM, messages and fault dependencies');
    }

    /** @type {'inactive' | 'bootstrapping' | 'settled' | 'gesture-scrolling' | 'jumping' | 'faulted' | 'disposed'} */
    let state = 'inactive';
    /** @type {any} */
    let layout = null;
    /** @type {{ scrolling: boolean; programmatic: boolean } | null} */
    let pendingGeometry = null;
    /** @type {number | null} */
    let scheduledCommit = null;
    let committing = false;
    let projectionHeld = false;
    let projectionDeferred = false;
    let measurementRefreshPending = false;
    let followingTail = false;
    /** @type {number[]} */
    let runtimeDemand = [];
    const virtual = createVirtualAdapter({
        root: domAdapter.root,
        scrollToFn: controller.scroll.virtualScrollTo,
        onGeometryChange(change) {
            pendingGeometry = change;
            if (committing || state === 'inactive' || state === 'bootstrapping' || state === 'jumping' || state === 'faulted' || state === 'disposed') {
                return;
            }
            if (change.scrolling && !change.programmatic) {
                state = 'gesture-scrolling';
            } else if (!change.programmatic) {
                followingTail = virtual.isAtEnd();
            }
            scheduleGeometryCommit();
        },
    });

    function assertOperational() {
        if (state === 'disposed') {
            throw new Error('Bounded ChatSurface is disposed');
        }
        const fault = controller.getFault();
        if (fault) {
            const error = /** @type {Error & { cause?: unknown }} */ (new Error('Bounded ChatSurface is faulted'));
            error.cause = fault;
            throw error;
        }
    }

    /** @param {Error} error */
    function acceptControllerFault(error) {
        const wasActive = state !== 'inactive';
        state = 'faulted';
        if (scheduledCommit !== null) {
            cancelAnimationFrame(scheduledCommit);
            scheduledCommit = null;
        }
        domAdapter.root.removeAttribute('data-tt-chat-bootstrap');
        if (wasActive) {
            onFault(error);
        }
    }

    const unsubscribeFault = controller.subscribeFault(acceptControllerFault);

    /** @param {unknown} error */
    function fail(error) {
        return controller.fault(error);
    }

    /** @param {ReturnType<typeof virtual.geometry>} geometry */
    function layoutFromGeometry(geometry) {
        return createBoundedProjectionLayout({
            count: geometry.count,
            viewportItems: geometry.viewportItems,
            projectedItems: geometry.projectedItems,
            paddingStart: geometry.metrics.paddingStart,
            gap: geometry.metrics.gap,
            maxViewportItems: CHAT_VIRTUAL_MAX_VIEWPORT_ITEMS,
        });
    }

    /** @param {any} current @param {any} next */
    function hasSameDomLayout(current, next) {
        return Boolean(current)
            && current.projection.indices.length === next.projection.indices.length
            && current.projection.indices.every(
                /** @param {number} messageId @param {number} index */
                (messageId, index) => messageId === next.projection.indices[index],
            )
            && current.topSpacer.present === next.topSpacer.present
            && current.topSpacer.height === next.topSpacer.height
            && current.middleSpacer.present === next.middleSpacer.present
            && current.middleSpacer.height === next.middleSpacer.height;
    }

    /** @param {ReturnType<typeof virtual.geometry>} geometry */
    function createRuntimeDemand(geometry) {
        const visible = geometry.visibleMessageIds.slice();
        const itemsByMessageId = new Map(geometry.viewportItems.map(item => [item.index, item]));
        const viewportCenter = geometry.scrollOffset + (domAdapter.root.clientHeight / 2);
        const distanceFromCenter = (/** @type {number} */ messageId) => {
            const item = itemsByMessageId.get(messageId);
            if (!item) {
                throw new Error(`ChatSurface runtime demand cannot resolve visible message ${messageId}`);
            }
            return Math.abs(((item.start + item.end) / 2) - viewportCenter);
        };
        visible.sort((left, right) => distanceFromCenter(left) - distanceFromCenter(right));
        const ordered = [
            ...visible,
            ...geometry.viewportItems.map(/** @param {any} item */ item => item.index),
        ];
        return [...new Set(ordered)];
    }

    /** @param {ReturnType<typeof virtual.geometry>} geometry */
    function captureMeasurementAnchor(geometry) {
        if (geometry.count === 0) {
            return null;
        }
        if (followingTail) {
            return Object.freeze({ messageId: geometry.count - 1, offset: 0, atEnd: true });
        }
        const messageId = geometry.visibleMessageIds[0] ?? geometry.viewportItems[0]?.index;
        if (messageId === undefined) {
            throw new Error('ChatSurface cannot preserve measurement position without a visible anchor');
        }
        const item = geometry.viewportItems.find(candidate => candidate.index === messageId);
        if (!item) {
            throw new Error('ChatSurface cannot preserve measurement position without a visible anchor');
        }
        return Object.freeze({
            messageId,
            offset: Math.max(0, geometry.scrollOffset - item.start),
            atEnd: false,
        });
    }

    /** @param {any} nextLayout @param {ReturnType<typeof virtual.geometry>} geometry @param {boolean} suspended */
    function finishProjection(nextLayout, geometry, suspended) {
        domAdapter.commitBoundedLayout(nextLayout);
        layout = nextLayout;
        onProjectionCommitted(nextLayout.projection.indices);
        const measuredGeometry = virtual.measure(domAdapter.directMessages());
        runtimeDemand = createRuntimeDemand(measuredGeometry.count === geometry.count ? measuredGeometry : geometry);
        controller.setRuntimeDemand({ messageIds: runtimeDemand, suspended });
    }

    /** @param {{ replaceMessageIds?: number[]; materializeOptionsByMessageId?: Map<number, any>; skipUnchanged?: boolean }} [options] */
    function commitCurrentGeometry({ replaceMessageIds = [], materializeOptionsByMessageId = new Map(), skipUnchanged = false } = {}) {
        assertOperational();
        virtual.refreshMetrics();
        const geometry = virtual.geometry();
        const nextLayout = layoutFromGeometry(geometry);
        if (skipUnchanged && hasSameDomLayout(layout, nextLayout)) {
            layout = nextLayout;
            runtimeDemand = createRuntimeDemand(geometry);
            controller.setRuntimeDemand({ messageIds: runtimeDemand, suspended: false });
            return nextLayout;
        }
        controller.project({
            indices: nextLayout.projection.indices.slice(),
            replaceMessageIds,
            materializeOptionsByMessageId,
        });
        finishProjection(nextLayout, geometry, false);
        return nextLayout;
    }

    /** @param {{ messages?: any[]; replaceMessageIds?: number[]; materializeOptionsByMessageId?: Map<number, any> }} [options] */
    function commitStructure({
        messages = getMessages(),
        replaceMessageIds = [],
        materializeOptionsByMessageId = new Map(),
    } = {}, suspended = false) {
        assertOperational();
        if (!Array.isArray(messages)) {
            throw new TypeError('Bounded ChatSurface messages must be an array');
        }
        /** @type {any} */
        let geometry;
        /** @type {any} */
        let nextLayout;
        controller.reconcilePlanned({
            messages,
            replaceMessageIds,
            materializeOptionsByMessageId,
            plan(structure) {
                virtual.setStructure(structure.keys);
                geometry = virtual.geometry();
                nextLayout = layoutFromGeometry(geometry);
                return { indices: nextLayout.projection.indices.slice() };
            },
        });
        finishProjection(nextLayout, geometry, suspended);
        return nextLayout;
    }

    function commitMeasurementRefresh() {
        const anchor = captureMeasurementAnchor(virtual.geometry());
        virtual.invalidateMeasurements();
        if (!anchor) {
            commitCurrentGeometry();
            pendingGeometry = null;
            return;
        }
        virtual.force(anchor.messageId);
        commitCurrentGeometry();
        if (anchor.atEnd) {
            virtual.scrollToEnd();
        } else {
            virtual.scrollToAnchor(anchor.messageId, anchor.offset);
        }
        virtual.setMode('normal');
        commitCurrentGeometry();
        if (anchor.atEnd) {
            virtual.scrollToEnd();
        } else {
            virtual.scrollToAnchor(anchor.messageId, anchor.offset);
        }
        followingTail = anchor.atEnd;
        // All notifications above were caused by this synchronous reflow.
        // The next real scroll callback remains authoritative.
        pendingGeometry = null;
    }

    function scheduleGeometryCommit() {
        if (scheduledCommit !== null || state === 'faulted' || state === 'disposed') {
            return;
        }
        scheduledCommit = requestAnimationFrame(() => {
            scheduledCommit = null;
            const change = pendingGeometry;
            pendingGeometry = null;
            if (!change || state === 'faulted' || state === 'disposed') {
                return;
            }
            try {
                if (change.scrolling && (state === 'gesture-scrolling' || !change.programmatic)) {
                    controller.setRuntimeDemand({ messageIds: runtimeDemand, suspended: true });
                    return;
                }
                if (projectionHeld) {
                    projectionDeferred = true;
                    controller.setRuntimeDemand({ messageIds: runtimeDemand, suspended: true });
                    return;
                }
                committing = true;
                if (measurementRefreshPending) {
                    measurementRefreshPending = false;
                    commitMeasurementRefresh();
                } else {
                    commitCurrentGeometry({ skipUnchanged: true });
                }
                state = 'settled';
            } catch (error) {
                fail(error);
            } finally {
                committing = false;
            }
            if (pendingGeometry) {
                scheduleGeometryCommit();
            }
        });
    }

    /** @param {{ messages?: any[] }} [options] */
    function open({ messages = getMessages() } = {}) {
        assertOperational();
        if (state !== 'inactive') {
            throw new Error(`Bounded ChatSurface cannot open from state ${state}`);
        }
        if (controller.getMountedMessageIds().length !== 0) {
            throw new Error('Bounded ChatSurface must open on an empty controller epoch');
        }

        try {
            state = 'bootstrapping';
            domAdapter.enableBoundedLayout();
            domAdapter.root.setAttribute('data-tt-chat-bootstrap', 'true');
            virtual.mount();
            virtual.setMode('tail');
            committing = true;
            commitStructure({ messages });
            virtual.scrollToEnd();
            followingTail = true;
            virtual.setMode('normal');
            commitCurrentGeometry();
            commitCurrentGeometry();
            pendingGeometry = null;
            domAdapter.root.removeAttribute('data-tt-chat-bootstrap');
            state = 'settled';
            return snapshot();
        } catch (error) {
            throw fail(error);
        } finally {
            committing = false;
        }
    }

    /** @param {{ messages?: any[]; replaceMessageIds?: number[]; materializeOptionsByMessageId?: Map<number, any> }} [options] */
    function reconcile(options = {}) {
        assertOperational();
        if (state === 'inactive') {
            return open(options);
        }
        const wasGestureScrolling = state === 'gesture-scrolling';
        const pendingBeforeStructure = pendingGeometry;
        try {
            committing = true;
            virtual.setMode('normal');
            commitStructure(options, wasGestureScrolling);
            if (wasGestureScrolling) {
                pendingGeometry = pendingBeforeStructure;
            }
            state = wasGestureScrolling ? 'gesture-scrolling' : 'settled';
            return snapshot();
        } catch (error) {
            throw fail(error);
        } finally {
            committing = false;
            if (pendingGeometry) {
                scheduleGeometryCommit();
            }
        }
    }

    /** @param {number} messageId @param {{ replace?: boolean; materializeOptionsByMessageId?: Map<number, any> }} [options] */
    function jumpToMessage(messageId, { replace = false, materializeOptionsByMessageId = new Map() } = {}) {
        assertOperational();
        if (state === 'inactive') {
            open();
        }
        if (projectionHeld && controller.getMessageElement(messageId) === null) {
            throw new Error('Bounded ChatSurface cannot leave an active message editor');
        }
        try {
            state = 'jumping';
            committing = true;
            virtual.force(messageId);
            commitCurrentGeometry({
                replaceMessageIds: replace ? [messageId] : [],
                materializeOptionsByMessageId,
            });
            virtual.scrollToIndex(messageId, 'center');
            virtual.setMode('normal');
            commitCurrentGeometry();
            const element = controller.getMessageElement(messageId);
            if (!(element instanceof HTMLElement) || !element.isConnected) {
                throw new Error(`Bounded ChatSurface jump did not mount message ${messageId}`);
            }
            const rootRect = domAdapter.root.getBoundingClientRect();
            const targetRect = element.getBoundingClientRect();
            if (
                rootRect.height > 0
                && (targetRect.bottom <= rootRect.top || targetRect.top >= rootRect.bottom)
            ) {
                throw new Error(`Bounded ChatSurface jump target ${messageId} is outside the viewport`);
            }
            pendingGeometry = null;
            followingTail = virtual.isAtEnd();
            state = 'settled';
            return element;
        } catch (error) {
            throw fail(error);
        } finally {
            committing = false;
        }
    }

    /** @param {{ includeAuxiliary?: boolean }} [options] */
    function resetEpoch({ includeAuxiliary = false } = {}) {
        if (state === 'disposed') {
            throw new Error('Bounded ChatSurface is disposed');
        }
        committing = true;
        try {
            if (scheduledCommit !== null) {
                cancelAnimationFrame(scheduledCommit);
                scheduledCommit = null;
            }
            pendingGeometry = null;
            virtual.dispose();
            controller.resetEpoch({ includeAuxiliary });
            domAdapter.disableBoundedLayout();
            virtual.reset();
            pendingGeometry = null;
            layout = null;
            runtimeDemand = [];
            projectionHeld = false;
            projectionDeferred = false;
            measurementRefreshPending = false;
            followingTail = false;
            state = 'inactive';
        } finally {
            committing = false;
        }
    }

    function refreshLayoutMetrics() {
        assertOperational();
        measurementRefreshPending = true;
        if (state === 'gesture-scrolling' || projectionHeld) {
            if (projectionHeld) {
                projectionDeferred = true;
            }
        } else {
            pendingGeometry = Object.freeze({ scrolling: false, programmatic: false });
            scheduleGeometryCommit();
        }
        return layout;
    }

    /** @param {boolean} held */
    function setProjectionHeld(held) {
        assertOperational();
        projectionHeld = Boolean(held);
        if (!projectionHeld && projectionDeferred) {
            projectionDeferred = false;
            pendingGeometry = Object.freeze({ scrolling: false, programmatic: false });
            scheduleGeometryCommit();
        }
    }

    function snapshot() {
        return Object.freeze({
            state,
            layout,
            geometry: state === 'inactive' || state === 'disposed' ? null : virtual.geometry(),
            runtimeDemand: Object.freeze(runtimeDemand.slice()),
            followingTail,
            projectionHeld,
            projectionDeferred,
            fault: controller.getFault(),
        });
    }

    function dispose() {
        if (state === 'disposed') {
            return;
        }
        if (state !== 'inactive') {
            resetEpoch();
        }
        virtual.dispose();
        unsubscribeFault();
        state = 'disposed';
    }

    return Object.freeze({
        open,
        reconcile,
        jumpToMessage,
        resetEpoch,
        refreshLayoutMetrics,
        setProjectionHeld,
        snapshot,
        dispose,
    });
}
