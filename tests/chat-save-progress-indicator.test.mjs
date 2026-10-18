import assert from 'node:assert/strict';
import test from 'node:test';

import {
    createSaveProgressIndicator,
    SAVE_PROGRESS_MIN_VISIBLE_MS,
    SAVE_PROGRESS_SETTLE_MS,
} from '../src/scripts/tauri/save-progress-indicator.js';

const BASELINE = 'Type a message, or /? for help';

function fakeTextarea() {
    let placeholder = BASELINE;
    return {
        getAttribute: () => placeholder,
        setAttribute: (_name, value) => { placeholder = value; },
        removeAttribute: () => { placeholder = null; },
        read: () => placeholder,
        overwrite: value => { placeholder = value; },
    };
}

function fakeClock() {
    const clock = { t: 0, timers: [], nextId: 1 };
    clock.now = () => clock.t;
    clock.schedule = (fn, ms) => {
        const id = clock.nextId++;
        clock.timers.push({ id, fn, at: clock.t + ms });
        return id;
    };
    clock.cancel = id => { clock.timers = clock.timers.filter(timer => timer.id !== id); };
    clock.advance = ms => {
        const target = clock.t + ms;
        for (;;) {
            clock.timers.sort((a, b) => a.at - b.at);
            const next = clock.timers[0];
            if (!next || next.at > target) break;
            clock.timers.shift();
            clock.t = next.at;
            next.fn();
        }
        clock.t = target;
    };
    return clock;
}

function fakeTarget() {
    const listeners = new Map();
    return {
        addEventListener: (name, fn) => listeners.set(name, fn),
        removeEventListener: name => listeners.delete(name),
        emit: detail => listeners.get('tt-chat-commit')?.({ detail }),
        size: () => listeners.size,
    };
}

function setup() {
    const textarea = fakeTextarea();
    const clock = fakeClock();
    const target = fakeTarget();
    const indicator = createSaveProgressIndicator({
        getTextarea: () => textarea,
        target,
        now: clock.now,
        schedule: clock.schedule,
        cancel: clock.cancel,
    });
    return { textarea, clock, target, indicator };
}

test('each commit kind renders its short label with the current percent', () => {
    const { textarea, target } = setup();

    target.emit({ pending: 1, primary: { kind: 'payload', percent: 12 } });
    assert.equal(textarea.read(), 'savechat:12%');

    target.emit({ pending: 1, primary: { kind: 'payload', percent: 100, status: 'finishing' } });
    assert.equal(textarea.read(), 'savechat:100%');

    target.emit({ pending: 1, primary: { kind: 'metadata', percent: 100 } });
    assert.equal(textarea.read(), 'savemeta:100%');

    target.emit({ pending: 1, primary: { kind: 'metadataExtension', percent: 40 } });
    assert.equal(textarea.read(), 'savemeta:40%');
});

test('the line outlives the queue by the settle delay and a minimum visible time', () => {
    const { textarea, clock, target } = setup();

    target.emit({ pending: 1, primary: { kind: 'payload', percent: 100 } });
    clock.advance(50);
    target.emit({ pending: 0, primary: null });
    assert.equal(textarea.read(), 'savechat:100%', 'settle delay keeps the line up');

    clock.advance(SAVE_PROGRESS_SETTLE_MS);
    assert.equal(textarea.read(), 'savechat:100%', 'minimum visible time still holds it');

    clock.advance(SAVE_PROGRESS_MIN_VISIBLE_MS);
    assert.equal(textarea.read(), BASELINE, 'the host placeholder returns');
});

test('consecutive writes bridge instead of restoring twice', () => {
    const { textarea, clock, target } = setup();

    target.emit({ pending: 1, primary: { kind: 'payload', percent: 100 } });
    clock.advance(200);
    target.emit({ pending: 0, primary: null });
    clock.advance(SAVE_PROGRESS_SETTLE_MS / 2);
    target.emit({ pending: 1, primary: { kind: 'metadata', percent: 0 } });
    assert.equal(textarea.read(), 'savemeta:0%');

    clock.advance(10);
    target.emit({ pending: 1, primary: { kind: 'metadata', percent: 100 } });
    clock.advance(SAVE_PROGRESS_SETTLE_MS + SAVE_PROGRESS_MIN_VISIBLE_MS);
    target.emit({ pending: 0, primary: null });
    clock.advance(SAVE_PROGRESS_SETTLE_MS + SAVE_PROGRESS_MIN_VISIBLE_MS);
    assert.equal(textarea.read(), BASELINE);
});

test('an external placeholder change becomes the new baseline', () => {
    const { textarea, clock, target } = setup();

    target.emit({ pending: 1, primary: { kind: 'payload', percent: 40 } });
    // The host flips the connection state while a save is on screen.
    textarea.overwrite('Not connected to API!');
    target.emit({ pending: 1, primary: { kind: 'payload', percent: 60 } });
    assert.equal(textarea.read(), 'savechat:60%');

    clock.advance(SAVE_PROGRESS_SETTLE_MS + SAVE_PROGRESS_MIN_VISIBLE_MS);
    target.emit({ pending: 0, primary: null });
    clock.advance(SAVE_PROGRESS_SETTLE_MS + SAVE_PROGRESS_MIN_VISIBLE_MS);
    assert.equal(textarea.read(), 'Not connected to API!', 'restores the newer host text');
});

test('a missing composer never breaks the write path and dispose restores later text', () => {
    const clock = fakeClock();
    const target = fakeTarget();
    let textarea = null;
    const indicator = createSaveProgressIndicator({
        getTextarea: () => textarea,
        target,
        now: clock.now,
        schedule: clock.schedule,
        cancel: clock.cancel,
    });

    target.emit({ pending: 1, primary: { kind: 'payload', percent: 10 } });
    assert.doesNotThrow(() => target.emit({ pending: 0, primary: null }));
    clock.advance(SAVE_PROGRESS_SETTLE_MS + SAVE_PROGRESS_MIN_VISIBLE_MS);

    // The composer returns while the save is still running.
    textarea = fakeTextarea();
    target.emit({ pending: 1, primary: { kind: 'payload', percent: 80 } });
    assert.equal(textarea.read(), 'savechat:80%');

    indicator.dispose();
    assert.equal(textarea.read(), BASELINE);
    assert.equal(target.size(), 0);
});
