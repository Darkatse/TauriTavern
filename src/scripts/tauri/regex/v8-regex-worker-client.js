// @ts-check

export const REGEX_EXECUTION_TIMEOUT_MS = 1500;

export class V8RegexTimeoutError extends Error {
    /**
     * @param {string} scriptKey
     * @param {string} scriptName
     */
    constructor(scriptKey, scriptName) {
        super(`Regex script "${scriptName || 'Unnamed regex script'}" exceeded ${REGEX_EXECUTION_TIMEOUT_MS} ms`);
        this.name = 'V8RegexTimeoutError';
        this.scriptKey = scriptKey;
        this.scriptName = scriptName;
    }
}

/** @type {Worker | null} */
let worker = null;
/** @type {Promise<unknown>} */
let queue = Promise.resolve();

function getWorker() {
    worker ??= new Worker(new URL('./v8-regex-worker.js', import.meta.url), { type: 'module' });
    return worker;
}

/** @param {Worker} target */
function terminateWorker(target) {
    target.terminate();
    worker = null;
}

/**
 * @param {{ text: string, scripts: any[] }[]} tasks
 * @returns {Promise<{ tasks: { text: string }[] }>}
 */
function execute(tasks) {
    const target = getWorker();

    return new Promise((resolve, reject) => {
        let timeoutId = 0;

        const cleanup = () => {
            clearTimeout(timeoutId);
            target.onmessage = null;
            target.onerror = null;
        };

        target.onmessage = event => {
            const message = event.data;

            if (message.type === 'script-start') {
                clearTimeout(timeoutId);
                if (message.allowSlow) {
                    return;
                }
                timeoutId = setTimeout(() => {
                    cleanup();
                    terminateWorker(target);
                    reject(new V8RegexTimeoutError(message.scriptKey, message.scriptName));
                }, REGEX_EXECUTION_TIMEOUT_MS);
                return;
            }

            cleanup();
            if (message.type === 'result') {
                resolve({ tasks: message.tasks });
            } else {
                reject(new Error(message.message || 'V8 regex worker failed'));
            }
        };

        target.onerror = event => {
            cleanup();
            terminateWorker(target);
            reject(new Error(event.message || 'V8 regex worker failed'));
        };

        target.postMessage({ tasks });
    });
}

/**
 * Runs one batch at a time on the shared worker.
 * @param {{ text: string, scripts: any[] }[]} tasks
 * @returns {Promise<{ tasks: { text: string }[] }>}
 */
export function applyV8RegexBatch(tasks) {
    const result = queue.then(() => execute(tasks));
    queue = result.catch(() => {});
    return result;
}
