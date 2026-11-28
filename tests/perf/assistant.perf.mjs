/** In-app assistant: opening its long Session, then loading an older page while 200 messages are shown. */
export default async function assistant({ page, measure, centerOf }) {
    const toggle = await centerOf(page.locator('#advanced-formatting-button .drawer-toggle'));
    await measure('assistant/open-session', () => page.mouse.click(toggle.x, toggle.y));
    const older = page.locator('#tt-assistant .ttia-older');
    const firstSeq = () => page.evaluate(() => Number(document.querySelector('#tt-assistant [data-message-seq]').dataset.messageSeq));
    for (let i = 0; i < 3; i++) {
        const seq = await firstSeq();
        await older.click();
        await page.waitForFunction(seq => Number(document.querySelector('#tt-assistant [data-message-seq]').dataset.messageSeq) < seq, seq);
    }
    await older.scrollIntoViewIfNeeded();
    const button = await centerOf(older);
    await measure('assistant/load-older', () => page.mouse.click(button.x, button.y));
    // Later scenarios start with no conversation shown and the drawer closed.
    await page.getByRole('button', { name: 'New conversation' }).click();
    await page.locator('#tt-assistant [data-message-seq]').first().waitFor({ state: 'detached' });
    await page.mouse.click(toggle.x, toggle.y);
}
