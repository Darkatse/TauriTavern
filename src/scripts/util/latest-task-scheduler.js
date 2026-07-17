/**
 * Runs at most one task at a time and coalesces requests received while it is
 * running into one final rerun against the latest state.
 *
 * @param {() => Promise<void>} task
 * @param {(error: unknown) => void} [onError]
 * @returns {() => void}
 */
export function createLatestTaskScheduler(task, onError = console.error) {
    let running = false;
    let pending = false;

    async function drain() {
        running = true;
        try {
            do {
                pending = false;
                try {
                    await task();
                } catch (error) {
                    onError(error);
                }
            } while (pending);
        } finally {
            running = false;
        }
    }

    return function scheduleLatest() {
        if (running) {
            pending = true;
            return;
        }

        void drain();
    };
}
