/** Prompt Manager: opening a message entry and a tool response, typing in the fields editor, and closing the form. */
export default async function promptManager({ page, measure, frame, centerOf }) {
    const form = page.locator('#completion_prompt_manager_popup_edit');
    const close = page.locator('#completion_prompt_manager_popup_entry_form_close');
    const edit = identifier => page.locator(`#completion_prompt_manager_list [data-pm-identifier="${identifier}"] .prompt-manager-edit-action`);
    const closeForm = async () => {
        await close.click();
        await form.locator('.tt-codemirror-editor').first().waitFor({ state: 'detached' });
    };
    await page.click('#leftNavDrawerIcon');
    // The first open in a session loads the editor bundle; the measured opens are later ones.
    await edit('main').click();
    await form.locator('.tt-codemirror-editor').waitFor();
    await closeForm();

    const main = await centerOf(edit('main'));
    await measure('prompt-manager/open-message-entry', () => page.mouse.click(main.x, main.y));
    await closeForm();
    await edit('readStateResult').scrollIntoViewIfNeeded();
    const result = await centerOf(edit('readStateResult'));
    await measure('prompt-manager/open-tool-entry', () => page.mouse.click(result.x, result.y));
    // Typing inside the call ID keeps the fields valid, so every key runs the whole check.
    await page.click('#completion_prompt_manager_popup_entry_fields_block .tt-codemirror-editor');
    for (const key of ['ControlOrMeta+Home', 'ArrowDown', 'End', 'ArrowLeft']) await page.keyboard.press(key);
    await measure('prompt-manager/type-fields-10-keys', async () => {
        for (let i = 0; i < 10; i++) {
            await page.keyboard.press('x');
            await frame();
        }
    });
    const fields = await form.locator('#completion_prompt_manager_popup_entry_fields_block .cm-content').innerText();
    const error = await page.locator('#completion_prompt_manager_popup_entry_form_fields_error').textContent();
    if (!fields.includes('"call_read_statexxxxxxxxxx"') || error) throw new Error(`The keys did not edit the call ID: ${fields} ${error}`);
    await close.scrollIntoViewIfNeeded();
    const closing = await centerOf(close);
    await measure('prompt-manager/close-entry', () => page.mouse.click(closing.x, closing.y));
    // Later scenarios measure the page as it was before this one.
    await page.locator('#completion_prompt_manager_popup').waitFor({ state: 'hidden' });
    await page.click('#leftNavDrawerIcon');
    await page.locator('#left-nav-panel').waitFor({ state: 'hidden' });
}
