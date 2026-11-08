import { expect, test } from '@rstest/core';
import { createMarkdownRenderer } from './markdown';

test('a growing text converts only its changed blocks, and an element open across blocks keeps them whole', () => {
    const converted: string[] = [];
    const convert = (block: string) => { converted.push(block); return `[${block}]`; };
    const render = createMarkdownRenderer(convert);
    render('One.\n\nTwo');
    expect(render('One.\n\nTwo three')).toBe('[One.\n][Two three]');
    expect(converted).toEqual(['One.\n', 'Two', 'Two three']);
    const details = 'A <details><summary>Info</summary>\n\n**B**\n\nC</details>';
    expect(render(details)).toBe(`[${details}]`);
});
