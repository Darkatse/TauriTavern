const DEFAULT_BOTTOM_THRESHOLD = 5;
const DEFAULT_USER_SCROLL_INTENT_TIMEOUT_MS = 1000;

export function isChatViewportAtBottom({ scrollHeight, clientHeight, scrollTop }, threshold = DEFAULT_BOTTOM_THRESHOLD) {
    return Math.abs(Number(scrollHeight) - Number(clientHeight) - Number(scrollTop)) < threshold;
}

export function createChatScrollIntentTracker({
    now = () => performance.now(),
    timeoutMs = DEFAULT_USER_SCROLL_INTENT_TIMEOUT_MS,
} = {}) {
    let activeUntil = Number.NEGATIVE_INFINITY;

    return Object.freeze({
        mark() {
            activeUntil = Number(now()) + timeoutMs;
        },
        clear() {
            activeUntil = Number.NEGATIVE_INFINITY;
        },
        isActive() {
            return Number(now()) < activeUntil;
        },
    });
}

export function createChatProgrammaticScrollTracker({
    requestFrame,
    cancelFrame,
    threshold = DEFAULT_BOTTOM_THRESHOLD,
}) {
    let expectedScrollTop = null;
    let expiryFrame = null;

    const clear = () => {
        expectedScrollTop = null;
        if (expiryFrame === null) {
            return;
        }
        cancelFrame(expiryFrame);
        expiryFrame = null;
    };

    return Object.freeze({
        mark(scrollTop) {
            clear();
            expectedScrollTop = Number(scrollTop);
            expiryFrame = requestFrame(() => {
                expectedScrollTop = null;
                expiryFrame = null;
            });
        },
        consumeIfMatches(scrollTop) {
            if (expectedScrollTop === null || Math.abs(Number(scrollTop) - expectedScrollTop) >= threshold) {
                return false;
            }
            clear();
            return true;
        },
        clear,
    });
}

export function createChatScrollController({
    readViewport,
    scrollToBottom,
    requestFrame,
    cancelFrame,
    canAutoScroll = () => true,
    bottomThreshold = DEFAULT_BOTTOM_THRESHOLD,
}) {
    let generationDepth = 0;
    let generationFollowsOutput = true;
    let pendingGenerationFollowsOutput = null;
    let pendingFrame = null;
    let pendingFrameForced = false;

    const cancelPending = () => {
        if (pendingFrame === null) {
            pendingFrameForced = false;
            return;
        }
        cancelFrame(pendingFrame);
        pendingFrame = null;
        pendingFrameForced = false;
    };

    const isAtBottom = () => isChatViewportAtBottom(readViewport(), bottomThreshold);

    return Object.freeze({
        captureGenerationIntent() {
            pendingGenerationFollowsOutput = isAtBottom();
            return pendingGenerationFollowsOutput;
        },
        clearGenerationIntent() {
            pendingGenerationFollowsOutput = null;
        },
        beginGeneration() {
            if (generationDepth === 0) {
                generationFollowsOutput = typeof pendingGenerationFollowsOutput === 'boolean'
                    ? pendingGenerationFollowsOutput
                    : isAtBottom();
                pendingGenerationFollowsOutput = null;
                if (!generationFollowsOutput) {
                    cancelPending();
                }
            }
            generationDepth += 1;
        },
        endGeneration() {
            generationDepth = Math.max(0, generationDepth - 1);
        },
        onViewportChanged({ userInitiated = true } = {}) {
            const atBottom = isAtBottom();
            if (generationDepth === 0) {
                generationFollowsOutput = atBottom;
            } else if (!atBottom && userInitiated) {
                generationFollowsOutput = false;
                cancelPending();
            }
        },
        requestScroll({ waitForFrame = false, force = false } = {}) {
            if (!canAutoScroll()) {
                cancelPending();
                return false;
            }
            if (!force && !generationFollowsOutput) {
                if (!pendingFrameForced) {
                    cancelPending();
                }
                return false;
            }
            if (pendingFrame !== null && pendingFrameForced && !force) {
                return true;
            }

            cancelPending();
            if (!waitForFrame) {
                scrollToBottom();
                return true;
            }

            pendingFrame = requestFrame(() => {
                pendingFrame = null;
                pendingFrameForced = false;
                if (canAutoScroll() && (force || generationFollowsOutput)) {
                    scrollToBottom();
                }
            });
            pendingFrameForced = force;
            return true;
        },
        cancelPending,
        shouldFollowOutput() {
            return generationFollowsOutput;
        },
    });
}
