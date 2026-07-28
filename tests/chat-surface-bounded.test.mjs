import test from 'node:test';
import assert from 'node:assert/strict';
import * as virtualCore from '@tanstack/virtual-core';

import { installFakeDom } from './helpers/fake-dom.mjs';
import { createChatDomAdapter } from '../src/tauri/main/adapters/chat-surface/chat-dom-adapter.js';
import { createChatScrollAdapter } from '../src/tauri/main/adapters/chat-surface/chat-scroll-adapter.js';
import { createTanStackVirtualAdapter } from '../src/tauri/main/adapters/chat-surface/tanstack-virtual-adapter.js';
import { createBoundedChatSurface } from '../src/tauri/main/services/chat-surface/bounded-chat-surface.js';
import { createChatSurfaceController } from '../src/tauri/main/services/chat-surface/chat-surface-controller.js';
import { createChatSurfaceParticipantRegistry } from '../src/tauri/main/services/chat-surface/participant-registry.js';

function createMessageElement(messageId, message) {
    const element = document.createElement('div');
    element.classList.add('mes');
    element.setAttribute('mesid', String(messageId));
    element._setRect({ top: 0, width: 800, height: message.height });
    const id = document.createElement('span');
    id.classList.add('mesIDDisplay');
    const content = document.createElement('div');
    content.classList.add('mes_text');
    content.textContent = message.mes;
    if (message.runtime) {
        content.append(document.createElement('pre'));
    }
    element.append(id, content);
    return element;
}

function createFixture(count, registry = createChatSurfaceParticipantRegistry()) {
    const messages = Array.from({ length: count }, (_value, index) => ({
        mes: `message-${index}`,
        height: 80 + (index % 5) * 20,
    }));
    const root = document.createElement('div');
    root.id = 'chat';
    root._setRect({ top: 0, width: 800, height: 600 });
    root.style.rowGap = '0px';
    document.body.append(root);
    let controller;
    const domAdapter = createChatDomAdapter({
        root,
        onUnauthorizedMutation: error => controller?.fault(error),
    });
    controller = createChatSurfaceController({
        getMessages: () => messages,
        materializeMessage: ({ message, messageId }) => createMessageElement(messageId, message),
        domAdapter,
        scrollAdapter: createChatScrollAdapter(root),
        participantRegistry: registry,
    });
    controller.configureRuntimeAdmission({ mode: 'managed' });
    const faults = [];
    let projectionCommitCount = 0;
    let notifyGeometryChange;
    const bounded = createBoundedChatSurface({
        controller,
        domAdapter,
        getMessages: () => messages,
        createVirtualAdapter(options) {
            notifyGeometryChange = options.onGeometryChange;
            return createTanStackVirtualAdapter({ ...options, virtualCore });
        },
        onProjectionCommitted() { projectionCommitCount += 1; },
        onFault: error => faults.push(error),
    });
    return {
        messages,
        root,
        domAdapter,
        controller,
        bounded,
        faults,
        getProjectionCommitCount: () => projectionCommitCount,
        notifyGeometryChange: change => notifyGeometryChange(change),
    };
}

test('bounded ChatSurface opens at the true tail and keeps a two-range DOM bound', () => {
    const dom = installFakeDom();
    try {
        const fixture = createFixture(10_000);
        const opened = fixture.bounded.open();
        const mounted = fixture.controller.getMountedMessageIds();
        assert.equal(opened.state, 'settled');
        assert.ok(mounted.length <= 33);
        assert.equal(mounted.at(-1), 9999);
        const topSpacer = fixture.root.querySelector('tt-chat-spacer[data-tt-chat-spacer="top"]');
        assert.ok(topSpacer);
        assert.equal(topSpacer.getAttribute('aria-hidden'), 'true');
        assert.equal(fixture.root.getAttribute('data-tt-chat-bootstrap'), null);
        assert.ok(fixture.controller.snapshot().projection.ranges.length <= 2);
        assert.deepEqual(fixture.faults, []);

        const target = fixture.bounded.jumpToMessage(10);
        assert.equal(target.getAttribute('mesid'), '10');
        assert.ok(fixture.controller.getMountedMessageIds().length <= 33);
        assert.equal(fixture.controller.getMountedMessageIds().at(-1), 9999);
        assert.ok(fixture.controller.snapshot().projection.ranges.length <= 2);
        const boundedSnapshot = fixture.bounded.snapshot();
        const viewportCenter = boundedSnapshot.geometry.scrollOffset + (fixture.root.clientHeight / 2);
        const itemsById = new Map(boundedSnapshot.geometry.viewportItems.map(item => [item.index, item]));
        const expectedVisibleDemand = boundedSnapshot.geometry.visibleMessageIds.slice().sort((left, right) => {
            const leftItem = itemsById.get(left);
            const rightItem = itemsById.get(right);
            return Math.abs(((leftItem.start + leftItem.end) / 2) - viewportCenter)
                - Math.abs(((rightItem.start + rightItem.end) / 2) - viewportCenter);
        });
        assert.deepEqual(
            boundedSnapshot.runtimeDemand.slice(0, expectedVisibleDemand.length),
            expectedVisibleDemand,
        );
        assert.equal(boundedSnapshot.runtimeDemand.includes(9999), false);

        fixture.messages.push({ mes: 'appended', height: 140 });
        fixture.bounded.reconcile();
        assert.equal(fixture.controller.getMountedMessageIds().at(-1), 10000);
        assert.ok(fixture.controller.getMountedMessageIds().length <= 33);

        for (const target of [10, 9_000, 20, 8_000, 30, 7_000]) {
            fixture.bounded.jumpToMessage(target);
            assert.ok(fixture.controller.getMountedMessageIds().includes(target));
            assert.equal(fixture.controller.getMountedMessageIds().at(-1), 10000);
            assert.ok(fixture.controller.getMountedMessageIds().length <= 33);
        }

        fixture.bounded.resetEpoch({ includeAuxiliary: true });
        dom.flushRaf();
        assert.equal(fixture.bounded.snapshot().state, 'inactive');
        assert.equal(fixture.root.querySelectorAll(':scope > .mes').length, 0);
        assert.equal(fixture.root.querySelector('tt-chat-spacer'), null);

        fixture.bounded.open();
        assert.equal(fixture.bounded.snapshot().state, 'settled');
        fixture.bounded.resetEpoch();
    } finally {
        dom.cleanup();
    }
});

test('bounded projection defers geometry until a held structure mutation reconciles', () => {
    const dom = installFakeDom();
    try {
        const fixture = createFixture(200);
        fixture.bounded.open();
        fixture.bounded.setProjectionHeld(true);
        fixture.messages.push({ mes: 'appended-before-event', height: 100 });
        fixture.notifyGeometryChange({ scrolling: false, programmatic: false });
        dom.flushRaf();
        assert.equal(fixture.bounded.snapshot().projectionHeld, true);
        assert.equal(fixture.bounded.snapshot().projectionDeferred, true);
        assert.equal(fixture.controller.snapshot().messageCount, 200);
        assert.deepEqual(fixture.faults, []);

        fixture.bounded.reconcile();
        fixture.bounded.setProjectionHeld(false);
        dom.flushRaf();
        assert.equal(fixture.controller.snapshot().messageCount, 201);
        assert.equal(fixture.bounded.snapshot().projectionDeferred, false);
        assert.equal(fixture.bounded.snapshot().state, 'settled');
        assert.deepEqual(fixture.faults, []);
        fixture.bounded.resetEpoch();
    } finally {
        dom.cleanup();
    }
});

test('global measurement refresh waits for an active gesture to settle', () => {
    const dom = installFakeDom();
    try {
        const fixture = createFixture(200);
        fixture.bounded.open();
        fixture.notifyGeometryChange({ scrolling: true, programmatic: false });
        dom.flushRaf();
        assert.equal(fixture.bounded.snapshot().state, 'gesture-scrolling');

        fixture.bounded.refreshLayoutMetrics();
        dom.flushRaf();
        assert.equal(fixture.bounded.snapshot().state, 'gesture-scrolling');

        fixture.notifyGeometryChange({ scrolling: false, programmatic: false });
        dom.flushRaf();
        assert.equal(fixture.bounded.snapshot().state, 'settled');
        fixture.bounded.resetEpoch();
    } finally {
        dom.cleanup();
    }
});

test('scrolling geometry stays suspended and an unchanged settle skips projection replay', () => {
    const dom = installFakeDom();
    try {
        const fixture = createFixture(200);
        fixture.bounded.open();
        const projectionCommitCount = fixture.getProjectionCommitCount();

        fixture.notifyGeometryChange({ scrolling: true, programmatic: false });
        dom.flushRaf();
        fixture.notifyGeometryChange({ scrolling: true, programmatic: false });
        dom.flushRaf();

        assert.equal(fixture.bounded.snapshot().state, 'gesture-scrolling');
        assert.equal(fixture.controller.snapshot().runtime.suspended, true);
        assert.equal(fixture.getProjectionCommitCount(), projectionCommitCount);

        fixture.notifyGeometryChange({ scrolling: false, programmatic: false });
        dom.flushRaf();

        assert.equal(fixture.bounded.snapshot().state, 'settled');
        assert.equal(fixture.controller.snapshot().runtime.suspended, false);
        assert.equal(fixture.getProjectionCommitCount(), projectionCommitCount);
        fixture.bounded.resetEpoch();
    } finally {
        dom.cleanup();
    }
});

test('structural reconcile keeps runtime admission suspended until the gesture settles', () => {
    const dom = installFakeDom();
    try {
        const fixture = createFixture(200);
        fixture.bounded.open();
        fixture.notifyGeometryChange({ scrolling: true, programmatic: false });
        dom.flushRaf();

        fixture.messages.push({ mes: 'appended-during-scroll', height: 100 });
        fixture.bounded.reconcile();

        assert.equal(fixture.bounded.snapshot().state, 'gesture-scrolling');
        assert.equal(fixture.controller.snapshot().runtime.suspended, true);

        fixture.notifyGeometryChange({ scrolling: true, programmatic: true });
        dom.flushRaf();
        assert.equal(fixture.bounded.snapshot().state, 'gesture-scrolling');
        assert.equal(fixture.controller.snapshot().runtime.suspended, true);

        fixture.notifyGeometryChange({ scrolling: false, programmatic: false });
        dom.flushRaf();
        assert.equal(fixture.bounded.snapshot().state, 'settled');
        assert.equal(fixture.controller.snapshot().runtime.suspended, false);
        fixture.bounded.resetEpoch();
    } finally {
        dom.cleanup();
    }
});

test('measurement refresh preserves a logical message anchor', () => {
    const dom = installFakeDom();
    try {
        const fixture = createFixture(500);
        fixture.bounded.open();
        fixture.bounded.jumpToMessage(250);
        const anchor = fixture.bounded.snapshot().geometry.visibleMessageIds[0];

        for (const element of fixture.domAdapter.directMessages()) {
            element._setRect({ height: element.getBoundingClientRect().height * 1.5 });
        }
        fixture.bounded.refreshLayoutMetrics();
        dom.flushRaf();

        assert.ok(fixture.bounded.snapshot().geometry.visibleMessageIds.includes(anchor));
        fixture.bounded.resetEpoch();
    } finally {
        dom.cleanup();
    }
});

test('measurement refresh clamps an intra-message anchor when the message shrinks', () => {
    const dom = installFakeDom();
    try {
        const fixture = createFixture(500);
        fixture.messages[250].height = 1000;
        fixture.bounded.open();
        fixture.bounded.jumpToMessage(250);
        dom.flushRaf();

        const item = fixture.bounded.snapshot().geometry.viewportItems.find(candidate => candidate.index === 250);
        fixture.root.scrollTo({ top: item.start + 800 });
        fixture.notifyGeometryChange({ scrolling: false, programmatic: false });
        dom.flushRaf();
        assert.equal(fixture.bounded.snapshot().geometry.visibleMessageIds[0], 250);

        fixture.controller.getMessageElement(250)._setRect({ height: 100 });
        fixture.bounded.refreshLayoutMetrics();
        dom.flushRaf();

        assert.ok(fixture.bounded.snapshot().geometry.visibleMessageIds.includes(250));
        fixture.bounded.resetEpoch();
    } finally {
        dom.cleanup();
    }
});

test('measurement refresh preserves follow-tail intent after reflow moves the live bottom', () => {
    const dom = installFakeDom();
    try {
        const fixture = createFixture(500);
        fixture.bounded.open();
        assert.equal(fixture.bounded.snapshot().followingTail, true);

        for (const element of fixture.domAdapter.directMessages()) {
            element._setRect({ height: element.getBoundingClientRect().height * 2 });
        }
        assert.equal(fixture.root.scrollTop < fixture.root.scrollHeight - fixture.root.clientHeight, true);

        fixture.bounded.refreshLayoutMetrics();
        dom.flushRaf();

        assert.equal(fixture.bounded.snapshot().followingTail, true);
        assert.equal(fixture.bounded.snapshot().geometry.atEnd, true);
        fixture.bounded.resetEpoch();
    } finally {
        dom.cleanup();
    }
});

test('bounded structural reconcile never turns an include hint into navigation', () => {
    const dom = installFakeDom();
    try {
        const fixture = createFixture(500);
        fixture.bounded.open();
        assert.equal(fixture.controller.getMessageElement(10), null);
        const scrollTop = fixture.root.scrollTop;

        fixture.bounded.reconcile({ includeMessageIds: [10] });

        assert.equal(fixture.controller.getMessageElement(10), null);
        assert.equal(fixture.root.scrollTop, scrollTop);
        assert.equal(fixture.bounded.snapshot().state, 'settled');
        assert.deepEqual(fixture.faults, []);
        fixture.bounded.resetEpoch();
    } finally {
        dom.cleanup();
    }
});

test('bounded runtime demand never exceeds Rmax and unmount synchronously revokes grants', () => {
    const dom = installFakeDom();
    try {
        const registry = createChatSurfaceParticipantRegistry();
        const activations = [];
        let cleanupCount = 0;
        registry.register({
            id: 'test/bounded-runtime',
            protocolVersion: 1,
            prepareContent({ mesid, content }, claims) {
                claims.claim(content.querySelector('pre'), ({ signal }) => {
                    activations.push({ mesid, signal });
                    return () => { cleanupCount += 1; };
                });
            },
        });
        const fixture = createFixture(500, registry);
        fixture.messages.forEach(message => { message.runtime = true; });
        fixture.bounded.open();
        dom.flushRaf();
        assert.ok(fixture.controller.snapshot().runtime.active > 0);
        assert.ok(fixture.controller.snapshot().runtime.active <= 8);

        const firstSignals = activations.map(entry => entry.signal);
        fixture.bounded.jumpToMessage(5);
        dom.flushRaf();
        assert.ok(cleanupCount > 0);
        assert.ok(firstSignals.some(signal => signal.aborted));
        assert.ok(fixture.controller.snapshot().runtime.active <= 8);

        fixture.bounded.resetEpoch();
        assert.equal(fixture.controller.snapshot().runtime.active, 0);
        assert.equal(fixture.controller.snapshot().runtime.candidates.length, 0);
    } finally {
        dom.cleanup();
    }
});

test('bounded DOM adapter rejects unknown direct flow ownership', () => {
    const dom = installFakeDom();
    try {
        const fixture = createFixture(20);
        const unknown = document.createElement('div');
        unknown.id = 'ecosystem-root-write';
        fixture.root.append(unknown);
        assert.throws(() => fixture.bounded.open(), /unknown direct child/);
        assert.equal(fixture.bounded.snapshot().state, 'faulted');
        assert.ok(fixture.controller.snapshot().fault instanceof Error);
    } finally {
        dom.cleanup();
    }
});

test('bounded surface immediately projects asynchronous controller faults', () => {
    const dom = installFakeDom();
    try {
        const fixture = createFixture(20);
        fixture.bounded.open();
        const fault = new Error('asynchronous participant failure');

        fixture.controller.fault(fault);

        assert.equal(fixture.controller.snapshot().fault, fault);
        assert.equal(fixture.bounded.snapshot().fault, fault);
        assert.equal(fixture.bounded.snapshot().state, 'faulted');
        assert.deepEqual(fixture.faults, [fault]);

        fixture.bounded.resetEpoch();
        assert.equal(fixture.controller.snapshot().fault, null);
        assert.equal(fixture.bounded.snapshot().state, 'inactive');
    } finally {
        dom.cleanup();
    }
});
