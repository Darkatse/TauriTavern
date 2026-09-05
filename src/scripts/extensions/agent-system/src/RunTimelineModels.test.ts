import { expect, test } from '@rstest/core';

import { createTimelineDetailState } from './run-timeline-detail-state';
import { createRunTimelineEventStore } from './run-timeline-event-store';
import { createRunTimelineSession } from './run-timeline-session';
import {
    heightFromTopEdgeDrag,
    runTimelineHeightBounds,
} from './run-timeline-resize';
import {
    canStartRunTimelineViewGesture,
    createRunTimelineViewGesture,
    resolveRunTimelineViewGesture,
    RUN_TIMELINE_VIEW_GESTURE_ACTION_DETAILS,
    shouldCancelRunTimelineViewGesture,
} from './run-timeline-view-gesture';
import { virtualizeTimelineItems } from './run-timeline-virtual-list';
import {
    captureTimelineScrollAnchor,
    restoreTimelineScrollAnchor,
    scrollTimelineToBottom,
} from './RunTimelineDom';
import type { TimelineDetailReadInput, TimelineDetailSection } from './RunTimelineContract';

function event(seq: number, runId = 'run-1', type = 'tool_call_completed'): TauriTavernAgentRunEvent {
    return {
        seq,
        id: `event-${runId}-${seq}`,
        runId,
        timestamp: '2026-01-01T00:00:00Z',
        level: 'info',
        type,
        payload: {},
    };
}

test('event store orders and deduplicates the complete loaded history', () => {
    const store = createRunTimelineEventStore();
    expect(store.addMany([event(3), event(1), event(2), event(2)])).toBe(true);
    expect(store.addMany([event(2)])).toBe(false);
    expect(store.events().map(item => item.seq)).toEqual([1, 2, 3]);
    expect(() => store.addMany([event(0)])).toThrow('positive integer');
    expect(() => store.addMany([{ ...event(4), id: '' }])).toThrow('id is required');
});

test('paging keeps all pages and stale reads cannot replace a reset session', async () => {
    const session = createRunTimelineSession({ runId: 'run-1' });
    const requests: number[] = [];
    const read = (input: { beforeSeq?: number }) => {
        requests.push(input.beforeSeq ?? 0);
        const end = input.beforeSeq === Number.MAX_SAFE_INTEGER ? 480 : 240;
        return Promise.resolve({ events: Array.from({ length: 240 }, (_, index) => event(end - index)) });
    };
    expect(await session.loadInitial(read)).toBe(true);
    expect(session.events).toHaveLength(240);
    expect(session.hasMoreBefore).toBe(true);
    expect(await session.loadOlder(read)).toBe(true);
    expect(session.events).toHaveLength(480);
    expect(session.events[0]?.seq).toBe(1);
    expect(requests).toEqual([Number.MAX_SAFE_INTEGER, 241]);

    const pending: Array<(value: { events: TauriTavernAgentRunEvent[] }) => void> = [];
    session.reset({ runId: 'old-run' });
    const oldRead = session.loadInitial(() => new Promise(resolve => pending.push(resolve)));
    session.reset({ runId: 'new-run' });
    const newRead = session.loadInitial(() => new Promise(resolve => pending.push(resolve)));
    pending[1]?.({ events: [event(1, 'new-run')] });
    expect(await newRead).toBe(true);
    pending[0]?.({ events: [event(1, 'old-run')] });
    expect(await oldRead).toBe(false);
    expect(session.events.map(item => item.runId)).toEqual(['new-run']);
});

test('detail state ignores stale async loads', async () => {
    const pending: Array<{
        input: TimelineDetailReadInput;
        resolve: (sections: TimelineDetailSection[]) => void;
    }> = [];
    const state = createTimelineDetailState({
        readSections: input => new Promise(resolve => pending.push({ input, resolve })),
    });
    const first = state.load({ runId: 'run-1', targets: [{ type: 'file', labelKey: 'timelineWorkspaceFile', path: 'first' }], readOnly: false });
    const second = state.load({ runId: 'run-1', targets: [{ type: 'file', labelKey: 'timelineWorkspaceFile', path: 'second' }], readOnly: true });
    expect(pending[1]?.input.readOnly).toBe(true);
    pending[1]?.resolve([{ labelKey: 'timelineResultText' }]);
    expect(await second).toBe(true);
    pending[0]?.resolve([{ labelKey: 'timelineContent' }]);
    expect(await first).toBe(false);
    expect(state.sections).toEqual([{ labelKey: 'timelineResultText' }]);
});

test('virtualizer limits DOM rows without dropping model entries', () => {
    const items = Array.from({ length: 120 }, (_, index) => ({ id: `item-${index + 1}`, rowSpan: 1 }));
    const top = virtualizeTimelineItems(items, 0, 174);
    expect(top.items).toHaveLength(19);
    expect(top.items[0]?.id).toBe('item-1');
    const middle = virtualizeTimelineItems(items, 58 * 50, 174);
    expect(middle.items[0]?.id).toBe('item-43');
    expect(middle.topPadding).toBe(42 * 58);
    expect(items).toHaveLength(120);
});

test('scroll anchor, follow-tail, resize, and touch gesture preserve their native semantics', () => {
    const scroller = document.createElement('div');
    Object.defineProperties(scroller, {
        scrollHeight: { configurable: true, value: 500 },
        clientHeight: { configurable: true, value: 100 },
    });
    scroller.scrollTop = 120;
    const anchor = captureTimelineScrollAnchor(scroller);
    Object.defineProperty(scroller, 'scrollHeight', { configurable: true, value: 720 });
    let nearBottom = false;
    restoreTimelineScrollAnchor(scroller, anchor, viewport => { nearBottom = viewport.nearBottom; });
    expect(scroller.scrollTop).toBe(340);
    expect(nearBottom).toBe(false);
    scrollTimelineToBottom(scroller, viewport => { nearBottom = viewport.nearBottom; });
    expect(scroller.scrollTop).toBe(720);
    expect(nearBottom).toBe(true);

    const bounds = runTimelineHeightBounds({ panelBottom: 800, topBoundary: 100, chromeHeight: 40 });
    expect(bounds).toEqual({ min: 132, max: 648 });
    expect(heightFromTopEdgeDrag({ startHeight: 300, startY: 500, currentY: 440, bounds })).toBe(360);

    const target = document.createElement('div');
    const pointer = (x: number, y: number, currentTarget = target) => ({
        pointerId: 7,
        clientX: x,
        clientY: y,
        isPrimary: true,
        pointerType: 'touch',
        target: currentTarget,
    });
    expect(canStartRunTimelineViewGesture({
        event: pointer(100, 100),
        collapsed: false,
        resizing: false,
        detailsOpen: false,
        selectedHasDetails: true,
    })).toBe(true);
    const gesture = createRunTimelineViewGesture(pointer(100, 100), false);
    expect(resolveRunTimelineViewGesture(gesture, pointer(20, 104), {
        detailsOpen: false,
        selectedHasDetails: true,
    })).toBe(RUN_TIMELINE_VIEW_GESTURE_ACTION_DETAILS);
    expect(shouldCancelRunTimelineViewGesture(gesture, pointer(98, 140))).toBe(true);
    const input = document.createElement('input');
    expect(canStartRunTimelineViewGesture({
        event: pointer(100, 100, input),
        collapsed: false,
        resizing: false,
        detailsOpen: false,
        selectedHasDetails: true,
    })).toBe(false);
});
