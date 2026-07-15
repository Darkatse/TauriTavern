const HIDDEN_MIN_INTERVAL_MS = 250;

/**
 * Resolves the interval for expensive streaming preview renders.
 * Network chunks and stream events remain unthrottled.
 * @param {object} options Render policy inputs.
 * @param {number} options.configuredFps User-configured streaming FPS.
 * @param {boolean} options.hidden Whether the document is hidden.
 * @returns {number} Render interval in milliseconds.
 */
export function getStreamingRenderInterval({ configuredFps, hidden }) {
    const fps = Number(configuredFps);
    const configuredInterval = Number.isFinite(fps) && fps > 0 ? 1000 / fps : 1000;

    if (hidden) {
        return Math.max(configuredInterval, HIDDEN_MIN_INTERVAL_MS);
    }

    return configuredInterval;
}

/**
 * Skips only exact no-op innerHTML writes. Final and fade-in renders preserve the original commit path.
 */
export function shouldCommitStreamingMessage({ currentHtml, nextHtml, final, fadeIn }) {
    return Boolean(final) || Boolean(fadeIn) || currentHtml !== nextHtml;
}
