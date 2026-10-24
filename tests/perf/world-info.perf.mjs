/** World Info: the entry list, typing a title, opening the 100k-character entry, typing in its editor, and closing it. */
export default async function worldInfo({ page, measure, centerOf }) {
    // One input, as from a fast typist: the debounced work runs once, after the last key.
    const typeTenKeys = async key => {
        for (let i = 0; i < 10; i++) {
            await page.keyboard.press(key);
            await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => setTimeout(resolve))));
        }
    };
    const drawer = await centerOf(page.locator('#WIDrawerIcon'));
    await measure('world-info/open-list', async () => {
        await page.mouse.click(drawer.x, drawer.y);
        await page.evaluate(async () => (await import('/scripts/world-info.js')).showWorldEditor('Review lore'));
    });
    await page.click('#world_popup_entries_list .world_entry textarea[name="comment"] >> nth=1');
    await measure('world-info/type-title-10-keys', () => typeTenKeys('y'));
    const toggles = page.locator('#world_popup_entries_list .world_entry .inline-drawer-toggle');
    // The first open in a session loads the editor bundle; the measured open is a later one.
    await toggles.nth(1).click();
    await page.locator('#world_popup_entries_list .cm-content').waitFor();
    await toggles.nth(1).click();
    const entry = await centerOf(toggles.first());
    await measure('world-info/open-entry', () => page.mouse.click(entry.x, entry.y));
    await page.click('#world_popup_entries_list .tt-codemirror-editor');
    await measure('world-info/type-10-keys', () => typeTenKeys('x'));
    await toggles.first().scrollIntoViewIfNeeded();
    const close = await centerOf(toggles.first());
    await measure('world-info/close-entry', () => page.mouse.click(close.x, close.y));
}
