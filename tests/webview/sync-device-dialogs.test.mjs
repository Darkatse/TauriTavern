import { expect, onTestFinished, rs, test } from '@rstest/core';
import { createFrontend } from '../helpers/frontend.mjs';

test('device input retries command failures and preserves the first result during asynchronous closing', async () => {
    const { window, load, startHost, close, waitUntilComplete } = await createFrontend();
    onTestFinished(close);
    await startHost();
    await load('script.js');
    const { Popup, POPUP_RESULT } = await load('scripts/popup.js');
    const { connectLanAddress } = await load('scripts/tauri/setting/setting-panel/sync-device-dialogs.js');
    let resolve;
    let reject;
    const submit = rs.fn(() => new window.Promise((done, fail) => { resolve = done; reject = fail; }));
    const saved = connectLanAddress({ connectLanAddress: submit });
    // The dialog attaches its feedback in onOpen, which runs once the opening animation ends.
    await waitUntilComplete();
    const popup = Popup.util.popups.at(-1);
    popup.mainInput.value = '192.168.1.10:50000';

    const first = popup.completeAffirmative();
    await popup.completeAffirmative();
    await popup.completeCancelled();
    expect(submit).toHaveBeenCalledTimes(1);
    expect(popup.mainInput.disabled).toBe(true);
    reject({ BadRequest: 'Bad request: Device did not respond' });
    await first;
    expect(popup.dlg.open).toBe(true);
    expect(popup.mainInput.disabled).toBe(false);
    expect(popup.mainInput.value).toBe('192.168.1.10:50000');
    expect(popup.dlg.querySelector('[role="status"]')?.textContent).toBe('Device did not respond');

    const retry = popup.completeAffirmative();
    await popup.completeCancelled();
    expect(submit).toHaveBeenCalledTimes(2);
    resolve();
    await retry;
    expect(await saved).toBe(true);
    expect(Popup.util.lastResult).toMatchObject({ result: POPUP_RESULT.AFFIRMATIVE, value: '192.168.1.10:50000' });
    expect(popup.dlg.isConnected).toBe(false);
});
