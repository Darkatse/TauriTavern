import { event_types, eventSource } from '../events.js';
import { t } from '../i18n.js';

const LIVE_REGION_ID = 'screen_reader_assistance_live_region';
const OWNER_ATTRIBUTE = 'data-tt-screen-reader-assistance';
const STALE_GENERATION_CLEAR_DELAY_MS = 1000;
const ANNOUNCEABLE_GENERATION_TYPES = new Set(['normal', 'regenerate', 'continue']);

let initialized = false;
let enabled = false;
let activeGeneration = false;
let staleGenerationTimer = null;
/** @type {HTMLElement | null} */
let liveRegion = null;

/** @type {Array<() => void>} */
const disposers = [];

function clearStaleGenerationTimer() {
    if (staleGenerationTimer !== null) {
        clearTimeout(staleGenerationTimer);
        staleGenerationTimer = null;
    }
}

function clearGenerationState() {
    clearStaleGenerationTimer();
    activeGeneration = false;
}

/**
 * @param {string} eventType
 * @param {Function} listener
 */
function listen(eventType, listener) {
    eventSource.on(eventType, listener);
    disposers.push(() => eventSource.removeListener(eventType, listener));
}

function requireLiveRegion() {
    if (!liveRegion) {
        throw new Error('Screen Reader Assistance live region is not installed');
    }
    return liveRegion;
}

function createLiveRegion() {
    if (!(document.body instanceof HTMLElement)) {
        throw new Error('Screen Reader Assistance requires document.body');
    }

    const existing = document.getElementById(LIVE_REGION_ID);
    if (existing) {
        throw new Error(`Screen Reader Assistance live region already exists: #${LIVE_REGION_ID}`);
    }

    liveRegion = document.createElement('div');
    liveRegion.id = LIVE_REGION_ID;
    liveRegion.setAttribute(OWNER_ATTRIBUTE, '');
    liveRegion.setAttribute('role', 'status');
    liveRegion.setAttribute('aria-live', 'polite');
    liveRegion.setAttribute('aria-atomic', 'true');
    liveRegion.style.position = 'absolute';
    liveRegion.style.width = '1px';
    liveRegion.style.height = '1px';
    liveRegion.style.padding = '0';
    liveRegion.style.margin = '-1px';
    liveRegion.style.overflow = 'hidden';
    liveRegion.style.clip = 'rect(0 0 0 0)';
    liveRegion.style.whiteSpace = 'nowrap';
    liveRegion.style.border = '0';
    document.body.append(liveRegion);

    disposers.push(() => {
        liveRegion?.remove();
        liveRegion = null;
    });
}

/**
 * @param {string} message
 */
function announce(message) {
    requireLiveRegion().textContent = message;
}

/**
 * @param {string | undefined | null} type
 * @param {object | undefined | null} params
 * @param {boolean} dryRun
 */
function shouldAnnounceGeneration(type, params, dryRun) {
    if (dryRun) {
        return false;
    }

    if (type === 'quiet' || type === 'swipe' || type === 'impersonate' || type === 'first_message') {
        return false;
    }

    if (params && 'quiet_prompt' in params && params.quiet_prompt && !params.quietToLoud) {
        return false;
    }

    return ANNOUNCEABLE_GENERATION_TYPES.has(type || 'normal');
}

/**
 * @param {string | undefined | null} type
 * @param {object | undefined | null} params
 * @param {boolean} dryRun
 */
function onGenerationStarted(type, params, dryRun) {
    clearGenerationState();
    activeGeneration = shouldAnnounceGeneration(type, params, dryRun);
    if (!activeGeneration) {
        return;
    }

    announce(t`AI is generating a response. Stop button is available.`);
}

function onGenerationEnded() {
    if (!activeGeneration) {
        return;
    }

    clearStaleGenerationTimer();
    staleGenerationTimer = setTimeout(() => {
        clearGenerationState();
    }, STALE_GENERATION_CLEAR_DELAY_MS);
}

/**
 * @param {number} _messageId
 * @param {string | undefined | null} type
 */
function onCharacterMessageRendered(_messageId, type) {
    if (!activeGeneration || type === 'swipe' || type === 'first_message') {
        return;
    }

    announce(t`AI response ready.`);
    clearGenerationState();
}

function onGenerationStopped() {
    if (!activeGeneration) {
        return;
    }

    announce(t`Generation stopped.`);
    clearGenerationState();
}

function onGenerationFailed() {
    if (!activeGeneration) {
        return;
    }

    announce(t`Generation failed.`);
    clearGenerationState();
}

/**
 * @param {string | undefined | null} status
 */
function onOnlineStatusChanged(status) {
    if (status === 'no_connection') {
        announce(t`API disconnected.`);
        return;
    }

    announce(t`API connected.`);
}

function installScreenReaderAssistance() {
    createLiveRegion();
    listen(event_types.GENERATION_STARTED, onGenerationStarted);
    listen(event_types.CHARACTER_MESSAGE_RENDERED, onCharacterMessageRendered);
    listen(event_types.GENERATION_ENDED, onGenerationEnded);
    listen(event_types.GENERATION_STOPPED, onGenerationStopped);
    listen(event_types.GENERATION_FAILED, onGenerationFailed);
    listen(event_types.ONLINE_STATUS_CHANGED, onOnlineStatusChanged);
    listen(event_types.CHAT_CHANGED, clearGenerationState);
}

function cleanupScreenReaderAssistance() {
    clearGenerationState();
    while (disposers.length > 0) {
        const dispose = disposers[disposers.length - 1];
        dispose();
        disposers.pop();
    }
}

export function initScreenReaderAssistance() {
    if (initialized) {
        return;
    }

    initialized = true;
    if (enabled) {
        installScreenReaderAssistance();
        eventSource.emit(event_types.SCREEN_READER_ASSISTANCE_CHANGED, true);
    }
}

/**
 * @param {boolean} value
 */
export function setScreenReaderAssistanceEnabled(value) {
    if (typeof value !== 'boolean') {
        throw new TypeError('Screen Reader Assistance enabled state must be a boolean');
    }

    if (enabled === value) {
        return;
    }

    enabled = value;
    if (!initialized) {
        return;
    }

    if (enabled) {
        installScreenReaderAssistance();
    } else {
        cleanupScreenReaderAssistance();
    }

    eventSource.emit(event_types.SCREEN_READER_ASSISTANCE_CHANGED, enabled);
}

export function isScreenReaderAssistanceEnabled() {
    return enabled;
}
