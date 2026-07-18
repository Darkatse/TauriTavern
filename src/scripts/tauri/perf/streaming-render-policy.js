// @ts-check

const DEFAULT_STREAMING_FPS = 30;
const HIDDEN_MIN_INTERVAL_MS = 250;

/**
 * Validates the user-configured streaming FPS once before generation starts.
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
 * Decides whether canonical streaming HTML should be committed to the DOM.
 * @param {object} options Commit policy inputs.
 * @param {string|null} options.lastCommittedHtml Last canonical HTML committed by the state owner.
 * @param {string} options.nextHtml Next canonical HTML to render.
 * @param {boolean} options.final Whether this is the final render.
 * @param {boolean} options.fadeIn Whether the existing fade-in path must run.
 * @returns {boolean} Whether the caller should commit and update its canonical cache.
 */
export function shouldCommitStreamingMessage({ lastCommittedHtml, nextHtml, final, fadeIn }) {
    return Boolean(final) || Boolean(fadeIn) || lastCommittedHtml !== nextHtml;
}
