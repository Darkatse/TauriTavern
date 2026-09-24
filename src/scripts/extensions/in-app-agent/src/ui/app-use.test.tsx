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
    document.body.innerHTML = `<section aria-label="Actions"><div class="mes_text">
        <p>${'Opening text '.repeat(200)}</p>${buttons}<p>omitted-body-tail</p>
    </div></section>`;
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
    expect(next.tree).not.toContain('omitted-body-tail');
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

test('long formatted messages leave embedded controls and following swipes actionable without text pagination', async () => {
    const paragraph = '<p>故事正文' + '内容'.repeat(350) + '<strong>强调</strong></p>';
    document.body.innerHTML = `<section aria-label="Chat">
        <div class="mes_text">
            <span style="display:none">hidden-body-text</span><span data-tt-sensitive>sensitive-body-text</span>
            <h2>故事开头</h2>${paragraph.repeat(120)}
            <ul>${'<li>正文列表项</li>'.repeat(100)}</ul>
            <p>end-of-long-body</p>
            <a href="#details">Details</a><button>Embedded action</button>
        </div>
        <button>Previous swipe</button><span>11/11</span>
        <div class="mes_text"><p>第二条消息</p><p>仍然可读</p></div>
    </section>`;
    const observation = createObservation();
    observation.enterRun('run');
    const overview = observation.snapshot({});
    const snapshot = observation.snapshot({ root: ref(overview.tree, 'Chat') });
    expect(snapshot.nextCursor).toBeUndefined();
    expect(snapshot.truncated).toBe(true);
    expect(snapshot.tree).toContain('textTruncated=true');
    expect(snapshot.tree).toContain('故事开头');
    expect(snapshot.tree).not.toContain('end-of-long-body');
    expect(snapshot.tree).not.toContain('hidden-body-text');
    expect(snapshot.tree).not.toContain('sensitive-body-text');
    expect(snapshot.tree).toContain('11/11');
    expect(snapshot.tree).toContain('第二条消息 仍然可读');
    expect(snapshot.tree.length).toBeLessThan(3_000);
    expect(observation.resolve(ref(snapshot.tree, 'Details'))).toBe(screen.getByRole('link'));
    for (const name of ['Embedded action', 'Previous swipe']) {
        const button = screen.getByRole('button', { name });
        button.onclick = () => { button.textContent = 'Done'; };
        hit = button;
        await interact({ action: 'click', ref: ref(snapshot.tree, name) }, observation, signal());
        expect(button.textContent).toBe('Done');
    }
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
    for (let index = 0; index < 8; index++) {
        const body = document.createElement('div');
        body.className = 'mes_text';
        body.textContent = 'Long message '.repeat(1_000);
        document.body.append(body);
    }
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
    expect(JSON.stringify(snapshot).length).toBeLessThan(8_000);
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
