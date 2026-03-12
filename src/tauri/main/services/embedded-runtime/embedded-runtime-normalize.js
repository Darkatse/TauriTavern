// @ts-check

/**
 * @typedef {import('./types.js').EmbeddedRuntimeSlot} EmbeddedRuntimeSlot
 * @typedef {import('./types.js').EmbeddedRuntimeProfile} EmbeddedRuntimeProfile
 */

/**
 * @param {unknown} value
 * @param {number} min
 * @param {number} max
 * @param {number} fallback
 */
function clampNumber(value, min, max, fallback) {
    const n = Number(value);
    if (!Number.isFinite(n)) {
        return fallback;
    }
    if (n < min) {
        return min;
    }
    if (n > max) {
        return max;
    }
    return n;
}

/**
 * @param {unknown} value
 * @returns {string | null}
 */
function coerceNonEmptyString(value) {
    const s = String(value ?? '').trim();
    return s ? s : null;
}

/**
 * @param {unknown} value
 * @param {number} fallback
 * @returns {number}
 */
function normalizeSlotNumber(value, fallback) {
    const n = Number(value);
    return Number.isFinite(n) ? n : fallback;
}

/**
 * @param {string} raw
 * @returns {{ top: number; right: number; bottom: number; left: number }}
 */
export function parseRootMarginPx(raw) {
    const text = String(raw || '').trim();
    const parts = text.split(/\s+/).filter(Boolean);
    if (parts.length === 0) {
        return { top: 0, right: 0, bottom: 0, left: 0 };
    }
    if (parts.length > 4) {
        throw new Error(`Invalid rootMargin: ${raw}`);
    }

    /** @param {string} part */
    const toPx = (part) => {
        const m = String(part).trim().match(/^(-?\d+(?:\.\d+)?)px$/i);
        if (!m) {
            throw new Error(`rootMargin only supports px: ${raw}`);
        }
        return Number(m[1]);
    };

    const values = parts.map(toPx);

    if (values.length === 1) {
        const v = /** @type {number} */ (values[0]);
        return { top: v, right: v, bottom: v, left: v };
    }
    if (values.length === 2) {
        const v = /** @type {number} */ (values[0]);
        const h = /** @type {number} */ (values[1]);
        return { top: v, right: h, bottom: v, left: h };
    }
    if (values.length === 3) {
        const t = /** @type {number} */ (values[0]);
        const h = /** @type {number} */ (values[1]);
        const b = /** @type {number} */ (values[2]);
        return { top: t, right: h, bottom: b, left: h };
    }

    const top = /** @type {number} */ (values[0]);
    const right = /** @type {number} */ (values[1]);
    const bottom = /** @type {number} */ (values[2]);
    const left = /** @type {number} */ (values[3]);
    return { top, right, bottom, left };
}

/**
 * @param {EmbeddedRuntimeSlot} slot
 */
export function normalizeEmbeddedRuntimeSlot(slot) {
    if (!slot || typeof slot !== 'object') {
        throw new Error('EmbeddedRuntimeManager.register(slot): slot is required');
    }

    const id = coerceNonEmptyString(slot.id);
    if (!id) {
        throw new Error('EmbeddedRuntimeManager.register(slot): slot.id is required');
    }

    const kind = coerceNonEmptyString(slot.kind);
    if (!kind) {
        throw new Error(`EmbeddedRuntimeManager.register(${id}): slot.kind is required`);
    }

    if (!(slot.element instanceof HTMLElement)) {
        throw new Error(`EmbeddedRuntimeManager.register(${id}): slot.element must be an HTMLElement`);
    }

    if (typeof slot.hydrate !== 'function' || typeof slot.dehydrate !== 'function') {
        throw new Error(`EmbeddedRuntimeManager.register(${id}): slot.hydrate/slot.dehydrate must be functions`);
    }

    const visibilityMode = slot.visibilityMode === 'manual' ? 'manual' : 'intersection';
    const visibilityTarget = slot.visibilityTarget instanceof HTMLElement ? slot.visibilityTarget : slot.element;

    return {
        id,
        kind,
        element: slot.element,
        visibilityTarget,
        visibilityMode,
        initialVisible: Boolean(slot.initialVisible),
        priority: clampNumber(normalizeSlotNumber(slot.priority, 0), -1000, 1000, 0),
        weight: clampNumber(normalizeSlotNumber(slot.weight, 1), 0, 10_000, 1),
        iframeCount: clampNumber(normalizeSlotNumber(slot.iframeCount, 0), 0, 1000, 0),
        hydrate: slot.hydrate,
        dehydrate: slot.dehydrate,
        dispose: typeof slot.dispose === 'function' ? slot.dispose : null,
    };
}

/**
 * @param {EmbeddedRuntimeProfile} profile
 */
export function normalizeEmbeddedRuntimeProfile(profile) {
    if (!profile || typeof profile !== 'object') {
        throw new Error('EmbeddedRuntimeManager requires a profile');
    }

    const name = coerceNonEmptyString(profile.name);
    if (!name) {
        throw new Error('EmbeddedRuntimeProfile.name is required');
    }

    const maxActiveWeight = clampNumber(profile.maxActiveWeight, 0, 1_000_000, 0);
    const maxActiveIframes = clampNumber(profile.maxActiveIframes, 0, 10_000, 0);
    const maxActiveSlots = clampNumber(profile.maxActiveSlots, 0, 100_000, 0);

    const parkWhenHiddenKinds = Array.isArray(profile.parkWhenHiddenKinds)
        ? profile.parkWhenHiddenKinds.map((k) => String(k || '').trim()).filter(Boolean)
        : [];

    return {
        name,
        maxActiveWeight,
        maxActiveIframes,
        maxActiveSlots,
        parkWhenHiddenKinds: new Set(parkWhenHiddenKinds),
        rootMargin: typeof profile.rootMargin === 'string' ? profile.rootMargin : '200px 0px',
        threshold: clampNumber(profile.threshold, 0, 1, 0.01),
    };
}

/**
 * @param {{ id: string; inViewport: boolean; visible: boolean; priority: number; lastVisibleAt: number; lastTouchedAt: number }} a
 * @param {{ id: string; inViewport: boolean; visible: boolean; priority: number; lastVisibleAt: number; lastTouchedAt: number }} b
 */
export function compareEmbeddedRuntimeSlotRank(a, b) {
    if (a.inViewport !== b.inViewport) {
        return a.inViewport ? -1 : 1;
    }
    if (a.visible !== b.visible) {
        return a.visible ? -1 : 1;
    }
    if (a.priority !== b.priority) {
        return b.priority - a.priority;
    }
    if (a.lastTouchedAt !== b.lastTouchedAt) {
        return b.lastTouchedAt - a.lastTouchedAt;
    }
    if (a.lastVisibleAt !== b.lastVisibleAt) {
        return b.lastVisibleAt - a.lastVisibleAt;
    }
    return a.id < b.id ? -1 : a.id > b.id ? 1 : 0;
}

