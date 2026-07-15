// @ts-check

/**
 * @typedef {(reason: string) => unknown | Promise<unknown>} LifecycleFlushHandler
 */

/**
 * @param {{
 *   windowObject: Pick<Window, 'addEventListener' | 'removeEventListener'>;
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
        /** @param {[string, { handler: LifecycleFlushHandler; priority: number }]} entry */
        const runHandler = ([name, { handler }]) => {
            try {
                return Promise.resolve(handler(reason)).then(() => {}).catch(error => {
                    logger.error(`Lifecycle flush handler failed: ${name}`, error);
                });
            } catch (error) {
                logger.error(`Lifecycle flush handler failed: ${name}`, error);
                return Promise.resolve();
            }
        };

        let chain = Promise.resolve();
        for (const entry of orderedHandlers) {
            chain = chain.then(() => runHandler(entry));
        }
        flushPromise = chain.finally(() => {
            flushPromise = null;
        });
        return flushPromise;
    }

    const onPageHide = () => void flush('pagehide');
    const onBeforeUnload = () => void flush('beforeunload');
    const onVisibilityChange = () => {
        if (documentObject.visibilityState === 'hidden') {
            void flush('visibilitychange:hidden');
        }
    };

    function install() {
        if (installed) {
            return;
        }

        installed = true;
        windowObject.addEventListener('pagehide', onPageHide);
        windowObject.addEventListener('beforeunload', onBeforeUnload);
        documentObject.addEventListener('visibilitychange', onVisibilityChange);
    }

    function uninstall() {
        if (!installed) {
            return;
        }

        installed = false;
        windowObject.removeEventListener('pagehide', onPageHide);
        windowObject.removeEventListener('beforeunload', onBeforeUnload);
        documentObject.removeEventListener('visibilitychange', onVisibilityChange);
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
