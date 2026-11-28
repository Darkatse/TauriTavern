/** Chat: sending a message and streaming a short reply, then streaming a reply of real length after its reasoning. */
export default async function chat({ page, measure }) {
    await page.evaluate(() => window.jQuery('#api_button_openai').triggerHandler('click'));
    await page.focus('#send_textarea');
    const send = async text => {
        await page.keyboard.insertText(text);
        await page.keyboard.press('Enter');
    };
    // Each token is its own input.
    const push = token => () => page.evaluate(text => globalThis.__perfReply.push(text), token);
    const think = token => () => page.evaluate(text => globalThis.__perfReply.think(text), token);
    const end = async () => {
        await page.evaluate(() => globalThis.__perfReply.end());
        await page.evaluate(async () => (await import('/script.js')).waitForGenerationIdle());
    };
    await measure('chat/send-and-stream',
        () => send('Tell me about the reef.'),
        ...'The keeper lifts the lantern and counts the ships.'.split(' ').map(word => push(`${word} `)),
        end,
    );

    const line = i => `The keeper turns to page ${i} of the log and reads out the ships that passed the reef. `;
    const reasoning = Array.from({ length: 6 }, (_, i) => line(i).repeat(3)).join('\n\n');
    const paragraphs = Array.from({ length: 20 }, (_, i) => i % 2 ? `"${line(i).repeat(2)}"` : `*He nods.* ${line(i).repeat(2)}`);
    const reply = [
        '<style>.perf-status { border: 1px solid #888; padding: 4px; } .perf-status b { color: #c84; }</style>',
        '<div class="perf-status"><b>Day 3</b> · Northern reef · Fog</div>',
        ...paragraphs.slice(0, 6),
        '## The ships',
        '1. The *Heron*, three masts.\n2. The *Gull*, low in the water.\n3. A barge with no name.',
        `> ${line(40)}`,
        '```text\nlog: 03:10 fog bell\nlog: 03:40 lantern relit\n```',
        ...paragraphs.slice(6),
        `<details><summary>Notes</summary>\n\n${line(41).repeat(2)}\n\n</details>`,
    ].join('\n\n');
    // About a paragraph per input.
    const chunks = text => text.match(/[\s\S]{1,320}/g);
    await send('Go on.');
    await page.waitForFunction(() => globalThis.__perfReply);
    await measure('chat/stream-long-reply', ...chunks(reasoning).map(think), ...chunks(reply).map(push));
    await measure('chat/finish-long-reply', end);
}
