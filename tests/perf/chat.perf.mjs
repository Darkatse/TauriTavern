/** Chat: sending a message, then rendering its reply token by token. */
export default async function chat({ page, measure }) {
    await page.evaluate(() => window.jQuery('#api_button_openai').triggerHandler('click'));
    await page.focus('#send_textarea');
    const tokens = 'The keeper lifts the lantern and counts the ships.'.split(' ');
    // Each token is its own input, so the streaming frame-rate limit, timed by the wall clock, renders every one.
    await measure('chat/send-and-stream',
        async () => {
            await page.keyboard.insertText('Tell me about the reef.');
            await page.keyboard.press('Enter');
        },
        ...tokens.map(token => () => page.evaluate(text => globalThis.__perfReply.push(text), `${token} `)),
        async () => {
            await page.evaluate(() => globalThis.__perfReply.end());
            await page.evaluate(async () => (await import('/script.js')).waitForGenerationIdle());
        },
    );
}
