import { event_types, eventSource } from '../../../script.js';

/** Events after which the live settings may no longer match the selected item. */
const WATCHED_EVENTS = [
    event_types.MAIN_API_CHANGED,
    event_types.CHATCOMPLETION_SOURCE_CHANGED,
    event_types.CHATCOMPLETION_MODEL_CHANGED,
    event_types.OAI_PRESET_CHANGED_AFTER,
    event_types.SECRET_ROTATED,
    // Catch-all for edits without their own event (server URL, post-processing, …).
    event_types.SETTINGS_UPDATED,
];

/**
 * Tracks whether the live settings drifted from the selected item since it was applied,
 * so the UI can offer "Reapply" instead of silently showing a stale name. What counts is
 * decided by the caller's fingerprint, which reads only what the item records.
 * @param {object} options
 * @param {() => boolean} options.isBusy Whether an item is being applied right now
 * @param {(dirty: boolean) => void} options.onChange Called when the state flips
 * @returns {{ track: (readFingerprint: () => Promise<string>) => Promise<void>, clear: () => void }}
 */
export function installDriftTracker({ isBusy, onChange }) {
    /** @type {{ read: () => Promise<string>, fingerprint: string } | null} */
    let baseline = null;
    /** Bumped by every track/clear, so a read that started earlier cannot land after it. */
    let generation = 0;
    /** Bumped by every check, so only the newest check decides. */
    let checks = 0;
    let dirty = false;

    const setDirty = (value) => {
        if (dirty === value) return;
        dirty = value;
        onChange(dirty);
    };

    const check = async () => {
        if (!baseline || isBusy()) return;
        const { read, fingerprint } = baseline;
        const at = generation;
        const seq = ++checks;
        const current = await read();
        if (at === generation && seq === checks) {
            setDirty(current !== fingerprint);
        }
    };

    for (const eventType of WATCHED_EVENTS) {
        eventSource.on(eventType, check);
    }

    return {
        /** Takes the current settings as matching the selected item. */
        async track(readFingerprint) {
            const at = ++generation;
            baseline = null;
            setDirty(false);
            const fingerprint = await readFingerprint();
            if (at === generation) {
                baseline = { read: readFingerprint, fingerprint };
            }
        },
        /** Nothing is selected: nothing can drift. */
        clear() {
            ++generation;
            baseline = null;
            setDirty(false);
        },
    };
}
