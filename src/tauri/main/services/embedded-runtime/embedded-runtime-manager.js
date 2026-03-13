// @ts-check

import { compareEmbeddedRuntimeSlotRank, normalizeEmbeddedRuntimeProfile, normalizeEmbeddedRuntimeSlot, parseRootMarginPx } from './embedded-runtime-normalize.js';

/**
 * @typedef {import('./types.js').EmbeddedRuntimeState} EmbeddedRuntimeState
 * @typedef {import('./types.js').EmbeddedRuntimeSlot} EmbeddedRuntimeSlot
 * @typedef {import('./types.js').EmbeddedRuntimeProfile} EmbeddedRuntimeProfile
 */

/**
 * @param {{ profile: EmbeddedRuntimeProfile; now?: () => number; root?: HTMLElement | null }} options
 */
export function createEmbeddedRuntimeManager({ profile, now = () => Date.now(), root = null }) {
    const normalizedProfile = normalizeEmbeddedRuntimeProfile(profile);
    const rootMarginPx = parseRootMarginPx(normalizedProfile.rootMargin);
    const rootElement = root instanceof HTMLElement ? root : null;

    const profileConfig = Object.freeze({
        name: normalizedProfile.name,
        maxActiveWeight: normalizedProfile.maxActiveWeight,
        maxActiveIframes: normalizedProfile.maxActiveIframes,
        maxActiveSlots: normalizedProfile.maxActiveSlots,
        maxSoftParkedIframes: normalizedProfile.maxSoftParkedIframes,
        softParkTtlMs: normalizedProfile.softParkTtlMs,
        rootMargin: normalizedProfile.rootMargin,
        threshold: normalizedProfile.threshold,
    });

    /** @type {Map<string, { slot: ReturnType<typeof normalizeEmbeddedRuntimeSlot>; state: EmbeddedRuntimeState; parkReason: string; visible: boolean; inViewport: boolean; lastVisibleAt: number; lastTouchedAt: number; activatedAt: number; deactivatedAt: number; }>} */
    const slots = new Map();

    let reconcilePending = false;

    const counters = {
        hydrate: 0,
        dehydrate: 0,
        parkVisibility: 0,
        parkBudget: 0,
        register: 0,
        unregister: 0,
        reconcile: 0,
        lastReconcileMs: 0,
        lastReconcileAt: 0,
        budgetDeny: 0,
        parkReasonChange: 0,
    };

    const observer = new IntersectionObserver((entries) => {
        const ts = now();
        const rootBounds = rootElement ? rootElement.getBoundingClientRect() : null;
        const rootTop = Number(rootBounds?.top) || 0;
        const rootRight = Number(rootBounds?.right) || (Number(globalThis.innerWidth) || 0);
        const rootBottom = Number(rootBounds?.bottom) || (Number(globalThis.innerHeight) || 0);
        const rootLeft = Number(rootBounds?.left) || 0;
        let changed = false;
        for (const entry of entries) {
            const target = entry.target;
            if (!(target instanceof HTMLElement)) {
                continue;
            }

            const id = target.dataset.ttRuntimeSlotId;
            if (!id) {
                continue;
            }

            const record = slots.get(id);
            if (!record) {
                continue;
            }
            if (record.slot.visibilityMode !== 'intersection') {
                continue;
            }

            const isVisible = Boolean(entry.isIntersecting) && entry.intersectionRatio >= normalizedProfile.threshold;
            const rect = entry.boundingClientRect;
            const isInViewport = rect.bottom > rootTop && rect.top < rootBottom && rect.right > rootLeft && rect.left < rootRight;

            if (record.visible !== isVisible) {
                record.visible = isVisible;
                if (isVisible) {
                    record.lastVisibleAt = ts;
                }
                changed = true;
            }

            if (record.inViewport !== isInViewport) {
                record.inViewport = isInViewport;
                changed = true;
            }
        }

        if (changed) {
            requestReconcile('visibility');
        }
    }, {
        root: rootElement,
        rootMargin: normalizedProfile.rootMargin,
        threshold: normalizedProfile.threshold,
    });

    /**
     * @param {string} reason
     */
    function requestReconcile(reason) {
        if (reconcilePending) {
            return;
        }
        reconcilePending = true;
        requestAnimationFrame(() => {
            reconcilePending = false;
            reconcile(reason);
        });
    }

    /**
     * @param {string} reason
     */
    function reconcile(reason) {
        const startedAt = now();
        counters.reconcile += 1;
        counters.lastReconcileAt = startedAt;

        /** @type {Array<{ id: string; inViewport: boolean; visible: boolean; priority: number; lastVisibleAt: number; lastTouchedAt: number }>} */
        const desired = [];

        for (const [id, record] of slots.entries()) {
            if (!record.slot.element.isConnected) {
                unregister(id);
                continue;
            }

            const keepAliveWhenHidden = !normalizedProfile.parkWhenHiddenKinds.has(record.slot.kind);
            const wantsActive = record.visible || keepAliveWhenHidden;
            if (!wantsActive) {
                continue;
            }

            desired.push({
                id,
                inViewport: record.inViewport,
                visible: record.visible,
                priority: record.slot.priority,
                lastVisibleAt: record.lastVisibleAt,
                lastTouchedAt: record.lastTouchedAt,
            });
        }

        desired.sort(compareEmbeddedRuntimeSlotRank);

        let activeWeight = 0;
        let activeIframes = 0;
        let activeSlots = 0;

        /** @type {Set<string>} */
        const nextActive = new Set();

        for (const item of desired) {
            const record = slots.get(item.id);
            if (!record) {
                continue;
            }

            const slotWeight = record.slot.weight;
            const slotIframes = record.slot.iframeCount;

            if (normalizedProfile.maxActiveSlots > 0 && activeSlots + 1 > normalizedProfile.maxActiveSlots) {
                counters.budgetDeny += 1;
                continue;
            }
            if (normalizedProfile.maxActiveWeight > 0 && activeWeight + slotWeight > normalizedProfile.maxActiveWeight) {
                counters.budgetDeny += 1;
                continue;
            }
            if (normalizedProfile.maxActiveIframes > 0 && activeIframes + slotIframes > normalizedProfile.maxActiveIframes) {
                counters.budgetDeny += 1;
                continue;
            }

            nextActive.add(item.id);
            activeSlots += 1;
            activeWeight += slotWeight;
            activeIframes += slotIframes;
        }

        for (const [id, record] of slots.entries()) {
            if (record.state === 'disposed') {
                continue;
            }
            const shouldBeActive = nextActive.has(id);
            if (shouldBeActive && record.state !== 'active') {
                record.slot.hydrate(reason);
                record.state = 'active';
                record.parkReason = '';
                record.activatedAt = startedAt;
                counters.hydrate += 1;
                continue;
            }
            if (!shouldBeActive) {
                const parkReason = record.visible || !normalizedProfile.parkWhenHiddenKinds.has(record.slot.kind)
                    ? 'budget'
                    : 'visibility';

                if (record.state === 'active' || record.state === 'cold') {
                    record.slot.dehydrate(parkReason);
                    record.state = 'parked';
                    record.parkReason = parkReason;
                    record.deactivatedAt = startedAt;
                    counters.dehydrate += 1;
                    if (parkReason === 'budget') {
                        counters.parkBudget += 1;
                    } else {
                        counters.parkVisibility += 1;
                    }
                    continue;
                }

                if (record.state === 'parked' && record.parkReason !== parkReason) {
                    record.slot.dehydrate(parkReason);
                    record.parkReason = parkReason;
                    counters.parkReasonChange += 1;
                }
            }
        }

        counters.lastReconcileMs = now() - startedAt;
    }

    /**
     * @param {string} id
     */
    function unregister(id) {
        const record = slots.get(id);
        if (!record) {
            return;
        }

        if (record.slot.visibilityMode === 'intersection') {
            observer.unobserve(record.slot.visibilityTarget);
        }

        record.state = 'disposed';
        if (record.slot.dispose) {
            record.slot.dispose();
        }
        slots.delete(id);
        counters.unregister += 1;
    }

    /**
     * @param {string} id
     */
    function touch(id) {
        const record = slots.get(id);
        if (!record || record.state === 'disposed') {
            throw new Error(`EmbeddedRuntimeManager.touch(${id}): slot not found`);
        }

        record.lastTouchedAt = now();
        requestReconcile('touch');
    }

    /**
     * Marks a slot as "dirty" so the next reconcile can re-assert its desired
     * state and (re)hydrate it if selected active.
     *
     * This is primarily used for ER-3 self-heal when third-party code removes
     * an iframe DOM node without properly unmounting its runtime controller.
     *
     * @param {string} id
     */
    function invalidate(id) {
        const record = slots.get(id);
        if (!record || record.state === 'disposed') {
            throw new Error(`EmbeddedRuntimeManager.invalidate(${id}): slot not found`);
        }

        record.lastTouchedAt = now();
        if (record.state !== 'cold') {
            record.state = 'cold';
            record.parkReason = '';
        }

        requestReconcile('invalidate');
    }

    /**
     * @param {string} id
     * @param {boolean} visible
     */
    function setVisible(id, visible) {
        const record = slots.get(id);
        if (!record || record.state === 'disposed') {
            throw new Error(`EmbeddedRuntimeManager.setVisible(${id}): slot not found`);
        }
        if (record.slot.visibilityMode !== 'manual') {
            throw new Error(`EmbeddedRuntimeManager.setVisible(${id}): slot is not manual visibility`);
        }

        const next = Boolean(visible);
        if (record.visible === next && record.inViewport === next) {
            return;
        }

        record.visible = next;
        record.inViewport = next;
        if (next) {
            record.lastVisibleAt = now();
        }
        requestReconcile('manual-visibility');
    }

    /**
     * @param {EmbeddedRuntimeSlot} slot
     */
    function register(slot) {
        const normalizedSlot = normalizeEmbeddedRuntimeSlot(slot);
        if (slots.has(normalizedSlot.id)) {
            throw new Error(`EmbeddedRuntimeManager.register(${normalizedSlot.id}): id already registered`);
        }

        normalizedSlot.element.dataset.ttRuntimeSlotId = normalizedSlot.id;
        if (normalizedSlot.visibilityTarget !== normalizedSlot.element) {
            normalizedSlot.visibilityTarget.dataset.ttRuntimeSlotId = normalizedSlot.id;
        }

        let visible = false;
        let inViewport = false;
        if (normalizedSlot.visibilityMode === 'manual') {
            visible = normalizedSlot.initialVisible;
            inViewport = normalizedSlot.initialVisible;
        } else {
            const rect = normalizedSlot.visibilityTarget.getBoundingClientRect();
            const rootBounds = rootElement ? rootElement.getBoundingClientRect() : null;
            const rootTop = Number(rootBounds?.top) || 0;
            const rootRight = Number(rootBounds?.right) || (Number(globalThis.innerWidth) || 0);
            const rootBottom = Number(rootBounds?.bottom) || (Number(globalThis.innerHeight) || 0);
            const rootLeft = Number(rootBounds?.left) || 0;
            inViewport = rect.bottom > rootTop && rect.top < rootBottom && rect.right > rootLeft && rect.left < rootRight;
            const bounds = {
                top: rootTop - rootMarginPx.top,
                right: rootRight + rootMarginPx.right,
                bottom: rootBottom + rootMarginPx.bottom,
                left: rootLeft - rootMarginPx.left,
            };
            visible = rect.bottom > bounds.top && rect.top < bounds.bottom && rect.right > bounds.left && rect.left < bounds.right;
        }

        slots.set(normalizedSlot.id, {
            slot: normalizedSlot,
            state: 'cold',
            parkReason: '',
            visible,
            inViewport,
            lastVisibleAt: visible ? now() : 0,
            lastTouchedAt: 0,
            activatedAt: 0,
            deactivatedAt: 0,
        });
        if (normalizedSlot.visibilityMode === 'intersection') {
            observer.observe(normalizedSlot.visibilityTarget);
        }
        counters.register += 1;
        requestReconcile('register');

        return {
            id: normalizedSlot.id,
            unregister: () => unregister(normalizedSlot.id),
        };
    }

    function getPerfSnapshot() {
        let registered = 0;
        let active = 0;
        let parked = 0;
        let visible = 0;
        let inViewport = 0;
        let activeWeight = 0;
        let activeIframes = 0;

        for (const record of slots.values()) {
            registered += 1;
            if (record.visible) {
                visible += 1;
            }
            if (record.inViewport) {
                inViewport += 1;
            }
            if (record.state === 'active') {
                active += 1;
                activeWeight += record.slot.weight;
                activeIframes += record.slot.iframeCount;
            } else if (record.state === 'parked') {
                parked += 1;
            }
        }

        return {
            profile: normalizedProfile.name,
            registered,
            visible,
            inViewport,
            active,
            parked,
            activeWeight,
            activeIframes,
            counters: { ...counters },
        };
    }

    return {
        register,
        unregister,
        touch,
        invalidate,
        setVisible,
        reconcile: () => reconcile('manual'),
        getPerfSnapshot,
        get profile() {
            return normalizedProfile.name;
        },
        get profileConfig() {
            return profileConfig;
        },
    };
}
