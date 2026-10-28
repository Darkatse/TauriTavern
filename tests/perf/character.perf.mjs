/** Characters: opening a heavy card from the character list, then returning to the startup card. */
export default async function character({ page, measure, centerOf }) {
    const item = avatar => page.locator(`#rm_print_characters_block .character_select[data-avatar="${avatar}"]`);
    const showList = async () => {
        await page.click('#rm_button_characters');
        await item('Review.png').waitFor();
    };
    await page.click('#rightNavDrawerIcon');
    await showList();
    const ledger = await centerOf(item('Ledger.png'));
    await measure('character/open-heavy-card', () => page.mouse.click(ledger.x, ledger.y));
    await showList();
    const review = await centerOf(item('Review.png'));
    await measure('character/open-back', () => page.mouse.click(review.x, review.y));
    // Later scenarios start with the drawer closed, as after startup.
    await page.click('#rightNavDrawerIcon');
}
