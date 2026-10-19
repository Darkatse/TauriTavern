import assert from 'node:assert/strict';
import test from 'node:test';

import {
    createSaveProgressIndicator,
    SAVE_PROGRESS_MIN_VISIBLE_MS,
    SAVE_PROGRESS_SETTLE_MS,
} from '../src/scripts/tauri/save-progress-indicator.js';

function fakePill() {
    const state = { text: null, shows: 0, hides: 0 };
    return {
        state,
        show(text) { state.text = text; state.shows += 1; },
        hide() { state.text = null; state.hides += 1; },
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
    const pill = fakePill();
    const clock = fakeClock();
    const target = fakeTarget();
    const indicator = createSaveProgressIndicator({
        pill, target, now: clock.now, schedule: clock.schedule, cancel: clock.cancel,
    });
    return { pill, clock, target, indicator };
}

test('each commit kind renders its short label with the current percent', () => {
    const { pill, target } = setup();

    target.emit({ pending: 1, primary: { kind: 'payload', percent: 12 } });
    assert.equal(pill.state.text, 'savechat 12%');

    target.emit({ pending: 1, primary: { kind: 'payload', percent: 100 } });
    assert.equal(pill.state.text, 'savechat 100%');

    target.emit({ pending: 1, primary: { kind: 'metadata', percent: 100 } });
    assert.equal(pill.state.text, 'savemeta 100%');

    target.emit({ pending: 1, primary: { kind: 'metadataExtension', percent: 40 } });
    assert.equal(pill.state.text, 'savemeta 40%');
});

test('the pill outlives the queue by the settle delay and a minimum visible time', () => {
    const { pill, clock, target } = setup();

    target.emit({ pending: 1, primary: { kind: 'payload', percent: 100 } });
    clock.advance(50);
    target.emit({ pending: 0, primary: null });
    assert.equal(pill.state.text, 'savechat 100%', 'settle delay keeps the line up');

    clock.advance(SAVE_PROGRESS_SETTLE_MS);
    assert.equal(pill.state.text, 'savechat 100%', 'minimum visible time still holds it');

    clock.advance(SAVE_PROGRESS_MIN_VISIBLE_MS);
    assert.equal(pill.state.hides, 1, 'the pill is removed once both delays elapsed');
});

test('consecutive writes bridge instead of hiding the pill twice', () => {
    const { pill, clock, target } = setup();

    target.emit({ pending: 1, primary: { kind: 'payload', percent: 100 } });
    clock.advance(200);
    target.emit({ pending: 0, primary: null });
    clock.advance(SAVE_PROGRESS_SETTLE_MS / 2);
    target.emit({ pending: 1, primary: { kind: 'metadata', percent: 0 } });
    assert.equal(pill.state.text, 'savemeta 0%');
    assert.equal(pill.state.hides, 0, 'the pill never blinked out');

    clock.advance(10);
    target.emit({ pending: 1, primary: { kind: 'metadata', percent: 100 } });
    clock.advance(SAVE_PROGRESS_SETTLE_MS + SAVE_PROGRESS_MIN_VISIBLE_MS);
    target.emit({ pending: 0, primary: null });
    clock.advance(SAVE_PROGRESS_SETTLE_MS + SAVE_PROGRESS_MIN_VISIBLE_MS);
    assert.equal(pill.state.hides, 1);
});
