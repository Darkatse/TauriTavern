import { afterEach, expect, rs, test } from '@rstest/core';
import { Popup, POPUP_RESULT } from '../../../popup.js';
import { connectLanAddress } from './sync-device-dialogs.js';

rs.mock('../../../i18n.js', () => ({
    translate: (message: string) => message,
    t: (strings: TemplateStringsArray) => strings.join(''),
}));
rs.mock('../../../RossAscends-mods.js', () => ({ shouldSendOnEnter: () => true }));
rs.mock('../../../power-user.js', () => ({ power_user: {}, toastPositionClasses: [] }));
rs.mock('../../../utils.js', () => ({
    uuidv4: () => crypto.randomUUID(),
    clamp: (value: number, min: number, max: number) => Math.min(max, Math.max(min, value)),
    removeFromArray: (values: unknown[], value: unknown) => values.splice(values.indexOf(value), 1),
    runAfterAnimation: (_element: Element, callback: () => void) => callback(),
}));

afterEach(() => {
    rs.restoreAllMocks();
    rs.unstubAllGlobals();
    Popup.util.popups.length = 0;
    Popup.util.lastResult = null;
    document.body.replaceChildren();
});

test('device input retries command failures and preserves the first result during asynchronous closing', async () => {
    rs.stubGlobal('jQuery', class {});
    rs.stubGlobal('toastr', { options: {} });
    rs.spyOn(HTMLDialogElement.prototype, 'showModal').mockImplementation(function (this: HTMLDialogElement) {
        this.open = true;
    });
    rs.spyOn(HTMLDialogElement.prototype, 'close').mockImplementation(function (this: HTMLDialogElement) {
        this.open = false;
    });
    let resolve!: () => void;
    let reject!: (error: unknown) => void;
    const submit = rs.fn(() => new Promise<void>((done, fail) => { resolve = done; reject = fail; }));
    const saved = connectLanAddress({ connectLanAddress: submit });
    const popup = Popup.util.popups[0];
    if (!popup) throw new Error('Expected device input popup');
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
