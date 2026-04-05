/**
 * Creates a "single-flight" wrapper for generation calls.
 *
 * Goals:
 * - Prevent re-entrancy: wait for the previous generation to fully settle.
 * - Serialize concurrent callers: multiple scripts calling `generate()` at once
 *   should run in order, not in parallel.
 *
 * @template {(...args: any[]) => any} TGenerate
 * @param {{
 *   waitForIdle: () => Promise<void>;
 *   generate: TGenerate;
 * }} deps
 * @returns {(...args: Parameters<TGenerate>) => Promise<Awaited<ReturnType<TGenerate>>>}
 */
export function createSafeGenerate({ waitForIdle, generate }) {
    let queue = Promise.resolve();

    return function safeGenerate(...args) {
        const run = queue
            .catch(() => {})
            .then(async () => {
                await waitForIdle();
                return generate(...args);
            });

        queue = run.catch(() => {});
        return run;
    };
}

