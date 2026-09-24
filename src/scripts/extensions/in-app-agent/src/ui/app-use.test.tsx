import { afterEach, beforeEach, expect, test, rstest } from '@rstest/core';
import { act, cleanup, render, screen } from '@testing-library/react';
import { useState } from 'react';
import { createObservation } from './snapshot';
import { interact } from './interact';

let hit: Element | null = null;
beforeEach(() => {
    document.body.style.visibility = 'visible';
    // Supply layout for semantic/event tests; visibility and hit testing belong to native WebView checks.
    rstest.spyOn(Element.prototype, 'getClientRects').mockImplementation(() => clientRects());
    rstest.spyOn(document, 'elementFromPoint').mockImplementation(() => hit);
});
afterEach(() => {
    cleanup();
    rstest.restoreAllMocks();
    document.body.replaceChildren();
    hit = null;
});

function clientRects(): DOMRectList {
    const rects = [new DOMRect(10, 10, 100, 30)];
    return Object.assign(rects, { item: (index: number) => rects[index] ?? null });
}

function ref(tree: string, text: string) {
    const line = tree.split('\n').find(candidate => candidate.includes(text));
    const result = line?.match(/\[ref=([^\]]+)\]/)?.[1];
    if (!result) {
        throw new Error(`Missing reference for ${text}: ${tree}`);
    }
    return result;
}
const signal = () => new AbortController().signal;

test('a paginated region stays actionable while earlier page and run refs expire', async () => {
    const buttons = Array.from({ length: 110 }, (_, index) =>
        `<button>Action ${index}</button>`,
    ).join('');
    document.body.innerHTML = `<section aria-label="Actions">${buttons}</section>`;
    const observation = createObservation();
    observation.enterRun('one');
    const overview = observation.snapshot({ depth: 1 });
    const oldRef = ref(overview.tree, 'Actions');
    const first = observation.snapshot({ root: oldRef });
    const cursor = first.nextCursor;
    if (!cursor) {
        throw new Error('Expected a continuation');
    }
    const next = observation.snapshot({ cursor });
    expect(() => observation.snapshot({ root: oldRef })).toThrow();
    const button = screen.getByRole('button', { name: 'Action 109' });
    button.onclick = () => { button.textContent = 'Done'; };
    hit = button;
    await interact({ action: 'click', ref: ref(next.tree, 'Action 109') }, observation, signal());
    expect(button.textContent).toBe('Done');
    observation.enterRun('two');
    expect(() => observation.snapshot({ root: ref(next.tree, 'Actions') })).toThrow();
});

test('short refs are not reused when the page module reloads', async () => {
    document.body.innerHTML = '<button>Before reload</button>';
    rstest.resetModules();
    const beforeReload = (await import('./snapshot')).createObservation();
    beforeReload.enterRun('run');
    const oldRef = ref(beforeReload.snapshot({}).tree, 'Before reload');

    // Reload the module as a page would, keeping browser storage and conversation history.
    rstest.resetModules();
    document.body.innerHTML = '<button>After reload</button>';
    const afterReload = (await import('./snapshot')).createObservation();
    afterReload.enterRun('run');
    const newRef = ref(afterReload.snapshot({}).tree, 'After reload');
    expect(() => afterReload.snapshot({ root: oldRef })).toThrow();
    expect(afterReload.snapshot({ root: newRef }).tree).toContain('After reload');
});

test('snapshots preserve control state while bounding text and omitting sensitive content', () => {
    document.body.innerHTML = `
        <button role="menuitemradio">Model A</button>
        <input data-tt-sensitive value="secret-should-not-appear">
        <div class="ttia-history-area">assistant-history-should-not-appear</div>
        <label><input type="checkbox">Show <span id="secret-label" data-tt-sensitive>secret-label-should-not-appear</span></label>
        <button aria-labelledby="secret-label"></button>
        <input aria-label="Readonly" readonly aria-readonly="false">
        <input type="checkbox" aria-label="Checked" checked aria-checked="false">
    `;
    const input = document.createElement('textarea');
    input.setAttribute('aria-label', 'Long value');
    input.value = '\u0001\n"'.repeat(100_000);
    document.body.append(input);
    const observation = createObservation();
    observation.enterRun('run');
    const snapshot = observation.snapshot({});
    expect(snapshot.tree.match(/Model A/g)).toHaveLength(1);
    expect(snapshot.tree).toContain('valueTruncated=true');
    expect(snapshot.tree).not.toContain('secret-should-not-appear');
    expect(snapshot.tree).not.toContain('assistant-history-should-not-appear');
    expect(snapshot.tree).not.toContain('secret-label-should-not-appear');
    expect(snapshot.tree.split('\n').find(line => line.includes('"Readonly"'))).toContain('readOnly=true');
    expect(snapshot.tree.split('\n').find(line => line.includes('"Checked"'))).toContain('checked=true');
    expect(JSON.stringify(snapshot).length).toBeLessThan(50_000);
});

test('fill updates React controlled state, not just the DOM value', async () => {
    function Form() {
        const [text, setText] = useState('before');
        return (
            <>
                <input aria-label="Text" value={text} onChange={event => setText(event.target.value)} />
                <output>{text}</output>
            </>
        );
    }
    render(<Form />);
    const observation = createObservation();
    observation.enterRun('run');
    const tree = observation.snapshot({}).tree;
    hit = screen.getByRole('textbox');
    await act(async () => {
        await interact({ action: 'fill', ref: ref(tree, '"Text"'), value: 'after' }, observation, signal());
    });
    expect(screen.getByRole('status').textContent).toBe('after');
});
