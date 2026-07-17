import assert from 'node:assert/strict';
import test from 'node:test';

import {
    createChatProgrammaticScrollTracker,
    createChatScrollController,
    createChatScrollIntentTracker,
    isChatViewportAtBottom,
} from '../src/scripts/tauri/perf/chat-scroll-controller.js';

function createHarness(
    viewport = { scrollHeight: 1000, clientHeight: 400, scrollTop: 600 },
    { canAutoScroll = () => true } = {},
) {
    const frames = new Map();
    const cancelled = [];
    let nextFrameId = 1;
    let scrolls = 0;
    const controller = createChatScrollController({
        readViewport: () => viewport,
        scrollToBottom: () => {
            scrolls += 1;
            viewport.scrollTop = Math.max(0, viewport.scrollHeight - viewport.clientHeight);
        },
        requestFrame: callback => {
            const id = nextFrameId++;
            frames.set(id, callback);
            return id;
        },
        cancelFrame: id => {
            cancelled.push(id);
            frames.delete(id);
        },
        canAutoScroll,
    });
    return {
        controller,
        viewport,
        cancelled,
        get scrolls() { return scrolls; },
        flushFrames() {
            // Drain nested rAF work (e.g. force navigation settles across two frames).
            while (frames.size > 0) {
                for (const [id, callback] of [...frames]) {
                    frames.delete(id);
                    callback();
                }
            }
        },
    };
}

test('viewport bottom detection tolerates only a small rounding gap', () => {
    assert.equal(isChatViewportAtBottom({ scrollHeight: 1000, clientHeight: 400, scrollTop: 598 }), true);
    assert.equal(isChatViewportAtBottom({ scrollHeight: 1000, clientHeight: 400, scrollTop: 590 }), false);
});

test('user scroll intent expires without scheduling a timer', () => {
    let now = 100;
    const tracker = createChatScrollIntentTracker({ now: () => now, timeoutMs: 1000 });

    assert.equal(tracker.isActive(), false);
    tracker.mark();
    assert.equal(tracker.isActive(), true);

    tracker.clear();
    assert.equal(tracker.isActive(), false);

    tracker.mark();

    now = 1099;
    assert.equal(tracker.isActive(), true);

    now = 1100;
    assert.equal(tracker.isActive(), false);
});

test('generation started away from bottom rejects every follow request', () => {
    const harness = createHarness({ scrollHeight: 1000, clientHeight: 400, scrollTop: 200 });
    harness.controller.beginGeneration();

    assert.equal(harness.controller.requestScroll({ waitForFrame: true }), false);
    harness.flushFrames();
    assert.equal(harness.scrolls, 0);
});

test('global auto-scroll setting rejects every request including forced and queued scrolls', () => {
    let autoScrollEnabled = false;
    const harness = createHarness(
        { scrollHeight: 1000, clientHeight: 400, scrollTop: 0 },
        { canAutoScroll: () => autoScrollEnabled },
    );

    assert.equal(harness.controller.requestScroll(), false);
    assert.equal(harness.controller.requestScroll({ force: true }), false);
    assert.equal(harness.controller.requestScroll({ waitForFrame: true }), false);
    harness.flushFrames();
    assert.equal(harness.scrolls, 0);

    harness.controller.beginGeneration();
    assert.equal(harness.controller.requestScroll(), false);
    assert.equal(harness.controller.requestScroll({ force: true }), false);
    assert.equal(harness.controller.requestScroll({ waitForFrame: true }), false);
    harness.flushFrames();
    assert.equal(harness.scrolls, 0);

    autoScrollEnabled = true;
    assert.equal(harness.controller.requestScroll({ waitForFrame: true, force: true }), true);
    autoScrollEnabled = false;
    harness.flushFrames();
    assert.equal(harness.scrolls, 0);
});

test('user scroll away cancels a queued generation scroll', () => {
    const harness = createHarness();
    harness.controller.beginGeneration();
    assert.equal(harness.controller.requestScroll({ waitForFrame: true }), true);

    harness.viewport.scrollTop = 300;
    harness.controller.onViewportChanged({ userInitiated: true });
    harness.flushFrames();

    assert.deepEqual(harness.cancelled, [1]);
    assert.equal(harness.scrolls, 0);
    assert.equal(harness.controller.requestScroll(), false);
});

test('layout shifts do not cancel generation follow that started at the bottom', () => {
    const harness = createHarness();
    harness.controller.beginGeneration();

    for (const phase of ['send', 'stream', 'completion']) {
        harness.viewport.scrollHeight += 100;
        harness.viewport.scrollTop = 0;
        harness.controller.onViewportChanged({ userInitiated: false });

        assert.equal(harness.controller.requestScroll({ waitForFrame: true }), true, `${phase} scroll should be scheduled`);
        harness.flushFrames();
        assert.equal(
            harness.viewport.scrollTop,
            harness.viewport.scrollHeight - harness.viewport.clientHeight,
            `${phase} should restore the bottom position`,
        );
    }

    harness.controller.endGeneration();
});

test('send-time follow intent survives layout shifts before generation begins', () => {
    const harness = createHarness();
    harness.controller.captureGenerationIntent();

    harness.viewport.scrollHeight += 100;
    harness.viewport.scrollTop = 0;
    harness.controller.beginGeneration();
    harness.controller.onViewportChanged({ userInitiated: false });

    assert.equal(harness.controller.requestScroll({ waitForFrame: true }), true);
    harness.flushFrames();
    assert.equal(harness.viewport.scrollTop, harness.viewport.scrollHeight - harness.viewport.clientHeight);

    harness.controller.endGeneration();
});

test('cleared send-time intent does not leak into a later generation', () => {
    const harness = createHarness();
    harness.controller.captureGenerationIntent();
    harness.controller.clearGenerationIntent();

    harness.viewport.scrollTop = 0;
    harness.controller.beginGeneration();

    assert.equal(harness.controller.requestScroll(), false);
    harness.controller.endGeneration();
});

test('viewport changes read the current controller state', () => {
    let viewportReads = 0;
    const viewport = { scrollHeight: 1000, clientHeight: 400, scrollTop: 600 };
    const controller = createChatScrollController({
        readViewport: () => {
            viewportReads += 1;
            return viewport;
        },
        scrollToBottom: () => {},
        requestFrame: () => 1,
        cancelFrame: () => {},
    });

    controller.onViewportChanged();
    assert.equal(viewportReads, 1);
    assert.equal(controller.shouldFollowOutput(), true);

    viewport.scrollTop = 300;
    controller.onViewportChanged();
    assert.equal(viewportReads, 2);
    assert.equal(controller.shouldFollowOutput(), false);
});

test('nested generation does not reset a cancelled follow session', () => {
    const harness = createHarness();
    harness.controller.beginGeneration();
    harness.viewport.scrollTop = 300;
    harness.controller.onViewportChanged();
    harness.controller.beginGeneration();
    harness.controller.endGeneration();

    assert.equal(harness.controller.shouldFollowOutput(), false);
    assert.equal(harness.controller.requestScroll(), false);
    harness.controller.endGeneration();
    assert.equal(harness.controller.shouldFollowOutput(), false);
    harness.viewport.scrollTop = 600;
    harness.controller.onViewportChanged();
    assert.equal(harness.controller.shouldFollowOutput(), true);
});

test('explicit navigation can scroll while content following is disabled', () => {
    const harness = createHarness({ scrollHeight: 1000, clientHeight: 400, scrollTop: 200 });
    harness.controller.onViewportChanged();

    assert.equal(harness.controller.requestScroll(), false);
    assert.equal(harness.controller.requestScroll({ force: true }), true);
    assert.equal(harness.scrolls, 1);
});

test('queued explicit navigation survives a rejected follow request', () => {
    const harness = createHarness({ scrollHeight: 1000, clientHeight: 400, scrollTop: 200 });
    harness.controller.beginGeneration();

    assert.equal(harness.controller.requestScroll({ waitForFrame: true, force: true }), true);
    assert.equal(harness.controller.requestScroll({ waitForFrame: true }), false);
    harness.flushFrames();

    // Force navigation settles on two frames after last_mes / content-visibility remeasure.
    assert.equal(harness.scrolls, 2);
    assert.equal(harness.viewport.scrollTop, 600);
    harness.controller.endGeneration();
});

test('queued explicit navigation is not downgraded by an allowed follow request', () => {
    const harness = createHarness();
    harness.controller.beginGeneration();

    assert.equal(harness.controller.requestScroll({ waitForFrame: true, force: true }), true);
    assert.equal(harness.controller.requestScroll({ waitForFrame: true }), true);

    harness.controller.endGeneration();
    harness.viewport.scrollTop = 200;
    harness.controller.onViewportChanged({ userInitiated: false });
    harness.flushFrames();

    assert.deepEqual(harness.cancelled, []);
    assert.equal(harness.scrolls, 2);
    assert.equal(harness.viewport.scrollTop, 600);
});

test('forced navigation resamples bottom after a collapsed intermediate height', () => {
    const frames = new Map();
    const cancelled = [];
    let nextFrameId = 1;
    let scrolls = 0;
    const viewport = { scrollHeight: 1000, clientHeight: 400, scrollTop: 200 };
    const heights = [500, 1200];
    let settleStep = 0;
    const controller = createChatScrollController({
        readViewport: () => viewport,
        scrollToBottom: () => {
            // Simulate the intermediate collapse on the first force settle frame.
            viewport.scrollHeight = heights[Math.min(settleStep, heights.length - 1)];
            settleStep += 1;
            scrolls += 1;
            viewport.scrollTop = Math.max(0, viewport.scrollHeight - viewport.clientHeight);
        },
        requestFrame: callback => {
            const id = nextFrameId++;
            frames.set(id, callback);
            return id;
        },
        cancelFrame: id => {
            cancelled.push(id);
            frames.delete(id);
        },
    });

    assert.equal(controller.requestScroll({ waitForFrame: true, force: true }), true);

    while (frames.size > 0) {
        for (const [id, callback] of [...frames]) {
            frames.delete(id);
            callback();
        }
    }

    assert.equal(scrolls, 2);
    assert.equal(viewport.scrollHeight, 1200);
    assert.equal(viewport.scrollTop, 800);
    assert.deepEqual(cancelled, []);
});

test('global auto-scroll disable cancels a queued explicit navigation', () => {
    let autoScrollEnabled = true;
    const harness = createHarness(undefined, { canAutoScroll: () => autoScrollEnabled });

    assert.equal(harness.controller.requestScroll({ waitForFrame: true, force: true }), true);
    autoScrollEnabled = false;
    assert.equal(harness.controller.requestScroll(), false);
    harness.flushFrames();

    assert.deepEqual(harness.cancelled, [1]);
    assert.equal(harness.scrolls, 0);
});

test('programmatic scroll tracker consumes only the expected asynchronous scroll event', () => {
    const frames = new Map();
    const cancelled = [];
    let nextFrameId = 1;
    const tracker = createChatProgrammaticScrollTracker({
        requestFrame: callback => {
            const id = nextFrameId++;
            frames.set(id, callback);
            return id;
        },
        cancelFrame: id => {
            cancelled.push(id);
            frames.delete(id);
        },
    });

    tracker.mark(600);
    assert.equal(tracker.consumeIfMatches(300), false);
    assert.equal(tracker.consumeIfMatches(600), true);
    assert.equal(tracker.consumeIfMatches(600), false);
    assert.deepEqual(cancelled, [1]);
});

test('programmatic scroll tracker expires an unobserved target on the next frame', () => {
    const frames = new Map();
    let nextFrameId = 1;
    const tracker = createChatProgrammaticScrollTracker({
        requestFrame: callback => {
            const id = nextFrameId++;
            frames.set(id, callback);
            return id;
        },
        cancelFrame: id => frames.delete(id),
    });

    tracker.mark(600);
    for (const [id, callback] of [...frames]) {
        frames.delete(id);
        callback();
    }

    assert.equal(tracker.consumeIfMatches(600), false);
});

test('new programmatic scroll target replaces the previous expiry frame', () => {
    const frames = new Map();
    const cancelled = [];
    let nextFrameId = 1;
    const tracker = createChatProgrammaticScrollTracker({
        requestFrame: callback => {
            const id = nextFrameId++;
            frames.set(id, callback);
            return id;
        },
        cancelFrame: id => {
            cancelled.push(id);
            frames.delete(id);
        },
    });

    tracker.mark(500);
    tracker.mark(600);

    assert.deepEqual(cancelled, [1]);
    assert.equal(tracker.consumeIfMatches(500), false);
    assert.equal(tracker.consumeIfMatches(600), true);
});

test('programmatic bottom jumps during generation do not cancel follow when marked non-user', () => {
    const harness = createHarness();
    harness.controller.beginGeneration();

    // Simulate a force scroll that temporarily leaves the viewport off-bottom before the frame lands.
    harness.viewport.scrollHeight += 120;
    harness.viewport.scrollTop = 0;
    harness.controller.onViewportChanged({ userInitiated: false });

    assert.equal(harness.controller.shouldFollowOutput(), true);
    assert.equal(harness.controller.requestScroll({ waitForFrame: true }), true);
    harness.flushFrames();
    assert.equal(
        harness.viewport.scrollTop,
        harness.viewport.scrollHeight - harness.viewport.clientHeight,
    );
    harness.controller.endGeneration();
});

test('stale user intent still cancels only when the viewport event is user initiated', () => {
    const harness = createHarness();
    harness.controller.beginGeneration();
    harness.controller.requestScroll({ waitForFrame: true });

    harness.viewport.scrollTop = 100;
    // Message insertion / programmatic scroll should pass userInitiated=false even if a prior gesture is recent.
    harness.controller.onViewportChanged({ userInitiated: false });
    harness.flushFrames();

    assert.equal(harness.controller.shouldFollowOutput(), true);
    assert.equal(harness.scrolls, 1);

    harness.viewport.scrollTop = 100;
    harness.controller.onViewportChanged({ userInitiated: true });
    assert.equal(harness.controller.shouldFollowOutput(), false);
    harness.controller.endGeneration();
});
