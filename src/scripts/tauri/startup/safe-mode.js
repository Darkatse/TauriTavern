import { invoke } from '../../../tauri-bridge.js';
import { t } from '../../i18n.js';
import { extractErrorText } from '../../util/command-error-utils.js';
import { openStartupDialog } from './startup-dialog.js';

const DIALOG_ID = 'tt-safe-mode';
const OFFER_SECONDS = 10;

/**
 * Answers this launch's safe mode prompt. A launch that requests safe mode never gets past it:
 * the next launch performs safe mode.
 */
export async function settleSafeModePrompt() {
    const prompt = await invoke('get_safe_mode_prompt');
    if (!prompt) return;
    const ask = prompt.kind === 'offer' ? offer : prompt.kind === 'report' ? report : null;
    if (!ask) throw new Error(`Unknown safe mode prompt: ${prompt.kind}`);
    const dialog = openStartupDialog(DIALOG_ID);
    await ask(dialog, prompt);
    dialog.remove();
    await invoke('dismiss_safe_mode_prompt');
}

export function loadUserStylesheet() {
    const link = document.createElement('link');
    link.rel = 'stylesheet';
    link.href = 'css/user.css';
    const loaded = new Promise(resolve => {
        link.addEventListener('load', resolve, { once: true });
        link.addEventListener('error', resolve, { once: true });
    });
    document.head.append(link);
    return loaded;
}

/**
 * Records the request and turns `dialog` into the request to reopen the app; a failed request
 * shows its error on `content` instead.
 * @param {import('./startup-dialog.js').StartupDialog} dialog
 * @param {import('./startup-dialog.js').StartupDialogContent} content
 */
export async function requestSafeMode(dialog, content) {
    try {
        await invoke('request_safe_mode');
    } catch (error) {
        dialog.show({ ...content, error: t`Couldn't enter safe mode: ${extractErrorText(error)}` });
        return;
    }
    dialog.show({
        title: t`Reopen TauriTavern`,
        description: t`Fully quit TauriTavern and open it again. It will start in safe mode automatically.`,
        actions: [],
    });
}

/**
 * Resolves when the user starts normally; a request leaves it pending.
 * @param {import('./startup-dialog.js').StartupDialog} dialog
 */
function offer(dialog) {
    return new Promise(resolve => {
        let stopCountdown = () => {};
        const content = {
            title: t`Start in safe mode?`,
            description: [
                t`TauriTavern doesn't seem to be starting properly.`,
                t`Safe mode turns off third-party extensions and restores default settings. Your previous settings are backed up, and characters and chats are not affected.`,
            ].join('\n\n'),
            actions: [
                {
                    name: 'normal',
                    label: t`Start normally`,
                    run: () => {
                        stopCountdown();
                        resolve();
                    },
                },
                {
                    name: 'safe-mode',
                    label: t`Enter safe mode`,
                    run: () => {
                        stopCountdown();
                        return requestSafeMode(dialog, content);
                    },
                },
            ],
        };
        const enter = dialog.show(content).at(-1);
        stopCountdown = countDown(
            seconds => { enter.textContent = t`Enter safe mode (${seconds})`; },
            () => enter.click(),
        );
    });
}

/**
 * @param {import('./startup-dialog.js').StartupDialog} dialog
 * @param {{ autoUploadPaused: boolean }} prompt
 */
function report(dialog, { autoUploadPaused }) {
    const paragraphs = [
        t`Third-party extensions are off and settings are back to their defaults. Characters and chats are not affected.`,
        t`Your previous settings were backed up. You can restore them in User Settings → Miscellaneous → Settings Snapshots.`,
        autoUploadPaused && t`Automatic sync upload is paused. Turn it back on in the sync settings when you're ready.`,
    ];
    return new Promise(resolve => dialog.show({
        title: t`Started in safe mode`,
        description: paragraphs.filter(Boolean).join('\n\n'),
        actions: [{ name: 'continue', label: t`Continue`, run: resolve }],
    }));
}

/**
 * Counts visible seconds with timers rather than animation frames, so the default still arrives
 * when the page stops painting.
 * @param {(seconds: number) => void} show
 * @param {() => void} expire
 */
function countDown(show, expire) {
    let remaining = OFFER_SECONDS;
    let timer;
    const tick = () => {
        if (document.visibilityState !== 'hidden' && --remaining === 0) {
            expire();
            return;
        }
        show(remaining);
        timer = setTimeout(tick, 1000);
    };
    show(remaining);
    timer = setTimeout(tick, 1000);
    return () => clearTimeout(timer);
}
