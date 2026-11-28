import { dataset } from './dataset.mjs';

/**
 * A front-end card: opening its chat with the status bars of the latest replies, sending, streaming a reply through
 * the card's regex, and finishing it into a status bar.
 */
export default async function frontendCard({ page, measure, centerOf }) {
    const { reasoning, content, user } = dataset().atlasReply;
    const item = avatar => page.locator(`#rm_print_characters_block .character_select[data-avatar="${avatar}"]`);
    const showList = async () => {
        await page.click('#rightNavDrawerIcon');
        await page.click('#rm_button_characters');
        await item('Atlas.png').waitFor();
    };
    await showList();
    const atlas = await centerOf(item('Atlas.png'));
    await measure('frontend-card/open-chat', () => page.mouse.click(atlas.x, atlas.y));
    await page.click('#rightNavDrawerIcon');

    await page.evaluate(() => window.jQuery('#api_button_openai').triggerHandler('click'));
    await page.focus('#send_textarea');
    await measure('frontend-card/send', async () => {
        await page.keyboard.insertText(user);
        await page.keyboard.press('Enter');
    });
    await page.waitForFunction(() => globalThis.__perfReply);
    // About a paragraph per input.
    const chunks = text => text.match(/[\s\S]{1,320}/g);
    await measure('frontend-card/stream-reply',
        ...chunks(reasoning).map(token => () => page.evaluate(text => globalThis.__perfReply.think(text), token)),
        ...chunks(content).map(token => () => page.evaluate(text => globalThis.__perfReply.push(text), token)));
    await measure('frontend-card/finish-reply', async () => {
        await page.evaluate(() => globalThis.__perfReply.end());
        await page.evaluate(async () => (await import('/script.js')).waitForGenerationIdle());
    });
    // The steps measured a front-end card only if its regex, the reasoning and Code Render shaped the reply. A step
    // ends with the page quiet, so the reply is read without waiting.
    const reply = page.locator('#chat .mes.last_mes');
    const [styled, reasoned] = await reply.evaluate(mes => [
        mes.querySelector('.mes_text [data-day]') !== null,
        Boolean(mes.querySelector('.mes_reasoning')?.textContent.trim()),
    ]);
    const tabs = await reply.frameLocator('.mes-code-preview iframe').locator('.sb-tab').count();
    if (!styled || !reasoned || tabs === 0) {
        throw new Error(`The reply was not shaped as a front-end card: regex ${styled}, reasoning ${reasoned}, status bar tabs ${tabs}`);
    }

    // Later scenarios start on the startup card with the drawer closed.
    await showList();
    await item('Review.png').click();
    await page.locator('#chat .mes .mes-code-preview').first().waitFor({ state: 'detached' });
    await page.click('#rightNavDrawerIcon');
}
