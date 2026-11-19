// @ts-check

const DEFAULT_STREAMING_FPS = 30;
const HIDDEN_MIN_INTERVAL_MS = 250;

/**
 * Validates the user-configured streaming FPS once per configured value: before a generation starts, or when a
 * streaming preview sees the setting change.
 * @param {unknown} configuredFps User-configured streaming FPS.
 * @returns {number} A finite FPS above zero.
 */
export function normalizeStreamingFps(configuredFps) {
    const fps = Number(configuredFps);
    if (Number.isFinite(fps) && fps > 0) {
        return fps;
    }

    console.warn(`Invalid streaming FPS "${String(configuredFps)}"; using the default ${DEFAULT_STREAMING_FPS} FPS.`);
    return DEFAULT_STREAMING_FPS;
}

/**
 * Resolves the interval for expensive streaming preview renders.
 * Network chunks and stream events remain unthrottled.
 * @param {object} options Render policy inputs.
 * @param {number} options.configuredFps Validated streaming FPS.
 * @param {boolean} options.hidden Whether the document is hidden.
 * @returns {number} Render interval in milliseconds.
 */
export function getStreamingRenderInterval({ configuredFps, hidden }) {
    const configuredInterval = 1000 / configuredFps;

    if (hidden) {
        return Math.max(configuredInterval, HIDDEN_MIN_INTERVAL_MS);
    }

    return configuredInterval;
}

/**
 * Paces the renders of one streaming preview by the streaming render interval. A scheduled render waits until the
 * interval has passed since this preview's previous render finished, then runs on the next animation frame, so a
 * hidden document does not render.
 * @param {() => unknown} readConfiguredFps Reads the user-configured streaming FPS.
 * @returns {(render: () => unknown) => () => void} Schedules a render and returns a function that cancels it.
 */
export function createStreamingRenderScheduler(readConfiguredFps) {
    let lastRender = Number.NEGATIVE_INFINITY;
    /** @type {unknown} */
    let configuredFps;
    let streamingFps = 0;
    return render => {
        /** @type {ReturnType<typeof setTimeout> | null} */
        let timer = null;
        /** @type {number | null} */
        let frame = null;
        const requestFrame = () => {
            frame = requestAnimationFrame(() => {
                frame = null;
                const finish = () => {
                    lastRender = performance.now();
                };
                let asyncRender = false;
                try {
                    const result = render();
                    if (result && typeof /** @type {{ then?: unknown }} */ (result).then === 'function') {
                        asyncRender = true;
                        Promise.resolve(result).then(finish, finish);
                    }
                } finally {
                    if (!asyncRender) finish();
                }
            });
        };
        const configured = readConfiguredFps();
        if (!streamingFps || !Object.is(configured, configuredFps)) {
            configuredFps = configured;
            streamingFps = normalizeStreamingFps(configured);
        }
        const interval = getStreamingRenderInterval({ configuredFps: streamingFps, hidden: document.hidden });
        const wait = lastRender + interval - performance.now();
        if (wait > 0) {
            timer = setTimeout(() => {
                timer = null;
                requestFrame();
            }, wait);
        } else {
            requestFrame();
        }
        return () => {
            if (timer !== null) clearTimeout(timer);
            if (frame !== null) cancelAnimationFrame(frame);
            timer = null;
            frame = null;
        };
    };
}
