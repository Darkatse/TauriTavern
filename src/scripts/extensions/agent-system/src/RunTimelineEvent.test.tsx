import '../style.css';
import { render } from '@testing-library/react';
import { expect, test } from '@rstest/core';

import type { TimelineItem } from './RunTimelineContract';
import { RunTimelineEvent } from './RunTimelineEvent';

const tr = (key: string): string => key;

test('a collapsed live preview shows the end of a tail that wraps past three lines', () => {
    const tail = `first ${'word '.repeat(60)}end`;
    const item: TimelineItem = {
        id: 'live:inv_root:0',
        seq: 1_000_000_000,
        runId: 'run-1',
        type: 'live_tool_call',
        level: 'info',
        timestamp: '',
        icon: 'fa-file-lines',
        tone: 'active',
        kind: 'write',
        titleKey: 'timelineLiveWriting',
        titleParams: { path: 'reply.md' },
        summary: '',
        rowSpan: 2,
        live: {
            toolId: 'builtin:workspace.write_file',
            tail,
            truncated: false,
            streamTone: 'neutral',
            expanded: false,
            blocks: [{ text: tail, streamTone: 'neutral' }],
            addedWords: 62,
            removedWords: 0,
        },
    };
    render(
        <ol className="ttas-run-events" style={{ width: 240 }}>
            <RunTimelineEvent item={item} tr={tr} selected={false} latest active={false} onActivate={() => undefined} />
        </ol>,
    );
    const preview = document.querySelector('.ttas-run-event-live-stream');
    const text = preview?.querySelector('.ttas-run-event-live-text')?.firstChild;
    if (!(preview instanceof HTMLElement) || !(text instanceof Text)) throw new Error('expected live preview text');
    const charBox = (offset: number) => {
        const range = document.createRange();
        range.setStart(text, offset);
        range.setEnd(text, offset + 1);
        return range.getBoundingClientRect();
    };
    const box = preview.getBoundingClientRect();
    const last = charBox(text.length - 1);
    expect(last.top).toBeGreaterThanOrEqual(box.top);
    expect(last.bottom).toBeLessThanOrEqual(box.bottom + 1);
    expect(charBox(0).bottom).toBeLessThanOrEqual(box.top);
});
