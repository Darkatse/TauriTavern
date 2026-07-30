// @ts-check

/**
 * @typedef {(reason: string) => unknown | Promise<unknown>} LifecycleFlushHandler
 */

export const TAURI_GRACEFUL_EXIT_EVENT = 'tauritavern-graceful-exit-requested';

/**
 * @param {{
 *   windowObject: Pick<Window, 'addEventListener' | 'removeEventListener' | '__TAURI__'>;
 *   documentObject: Pick<Document, 'addEventListener' | 'removeEventListener' | 'visibilityState'>;
 *   logger?: Pick<Console, 'error'>;
 * }} deps
 */
export function createLifecycleFlushService({ windowObject, documentObject, logger = console }) {
    /** @type {Map<string, { handler: LifecycleFlushHandler; priority: number }>} */
    const handlers = new Map();
    let installed = false;
    /** @type {Promise<void> | null} */
    let flushPromise = null;
    /** @type {(() => void) | null} */
    let nativeExitUnlisten = null;
    let nativeExitPending = false;

    /**
     * @param {string} name
     * @param {LifecycleFlushHandler} handler
     * @param {{ priority?: number }} [options]
     */
    function register(name, handler, { priority = 0 } = {}) {
        if (!name || typeof handler !== 'function') {
            throw new TypeError('Lifecycle flush handlers require a name and function');
        }
        if (!Number.isFinite(priority)) {
            throw new TypeError('Lifecycle flush handler priority must be finite');
        }

        handlers.set(name, { handler, priority });
        return () => {
            if (handlers.get(name)?.handler === handler) {
                handlers.delete(name);
            }
        };
    }

    /** @param {string} reason */
    function flush(reason) {
        if (flushPromise) {
            return flushPromise;
        }

        const orderedHandlers = Array.from(handlers.entries())
            .sort((left, right) => left[1].priority - right[1].priority);
        /** @type {unknown[]} */
        const failures = [];
        /** @param {[string, { handler: LifecycleFlushHandler; priority: number }]} entry */
        const runHandler = ([name, { handler }]) => {
            try {
                return Promise.resolve(handler(reason)).then(() => {}).catch(error => {
                    logger.error(`Lifecycle flush handler failed: ${name}`, error);
                    failures.push(error);
                });
            } catch (error) {
                logger.error(`Lifecycle flush handler failed: ${name}`, error);
                failures.push(error);
                return Promise.resolve();
            }
        };

        let chain = Promise.resolve();
        for (const entry of orderedHandlers) {
            chain = chain.then(() => runHandler(entry));
        }
        flushPromise = chain.then(() => {
            if (failures.length > 0) {
                throw failures[0];
            }
        }).finally(() => {
            flushPromise = null;
        });
        return flushPromise;
    }

    /** @param {string} reason */
    const flushInBackground = reason => void flush(reason).catch(error => {
        logger.error(`Lifecycle flush failed: ${reason}`, error);
    });

    const onPageHide = () => flushInBackground('pagehide');
    const onBeforeUnload = () => flushInBackground('beforeunload');
    const onVisibilityChange = () => {
        if (documentObject.visibilityState === 'hidden') {
            flushInBackground('visibilitychange:hidden');
        }
    };

    function installNativeExitHandler() {
        const tauriEvent = windowObject.__TAURI__?.event;
        const tauriWindow = windowObject.__TAURI__?.window;
        if (typeof tauriEvent?.listen !== 'function' || typeof tauriWindow?.getCurrentWindow !== 'function') {
            return;
        }

        void tauriEvent.listen(TAURI_GRACEFUL_EXIT_EVENT, async () => {
            if (nativeExitPending) {
                return;
            }

            nativeExitPending = true;
            try {
                await flush('tauri:exit-requested');
                await tauriWindow.getCurrentWindow().destroy();
            } catch (error) {
                nativeExitPending = false;
                logger.error('Lifecycle flush failed; keeping the app open', error);
            }
        }).then(/** @param {() => void} unlisten */ unlisten => {
            if (installed) {
                nativeExitUnlisten = unlisten;
            } else {
                unlisten();
            }
        }).catch(/** @param {unknown} error */ error => {
            logger.error('Failed to install the native exit handler', error);
        });
    }

    function install() {
        if (installed) {
            return;
        }

        installed = true;
        windowObject.addEventListener('pagehide', onPageHide);
        windowObject.addEventListener('beforeunload', onBeforeUnload);
        documentObject.addEventListener('visibilitychange', onVisibilityChange);
        installNativeExitHandler();
    }

    function uninstall() {
        if (!installed) {
            return;
        }

        installed = false;
        windowObject.removeEventListener('pagehide', onPageHide);
        windowObject.removeEventListener('beforeunload', onBeforeUnload);
        documentObject.removeEventListener('visibilitychange', onVisibilityChange);
        nativeExitUnlisten?.();
        nativeExitUnlisten = null;
        nativeExitPending = false;
    }

    return {
        register,
        flush,
        install,
        uninstall,
        waitForIdle: () => flushPromise ?? Promise.resolve(),
    };
}

/** @type {ReturnType<typeof createLifecycleFlushService> | undefined} */
let defaultService;

function getDefaultService() {
    defaultService ??= createLifecycleFlushService({
        windowObject: window,
        documentObject: document,
    });
    return defaultService;
}

/**
 * @param {string} name
 * @param {LifecycleFlushHandler} handler
 * @param {{ priority?: number }} [options]
 */
export function registerLifecycleFlushHandler(name, handler, options) {
    return getDefaultService().register(name, handler, options);
}

export function installLifecycleFlushHandlers() {
    getDefaultService().install();
}
