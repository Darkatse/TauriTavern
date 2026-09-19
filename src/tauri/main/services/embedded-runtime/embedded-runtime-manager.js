// @ts-check

import { compareEmbeddedRuntimeSlotRank, normalizeEmbeddedRuntimeProfile, normalizeEmbeddedRuntimeSlot, parseRootMarginPx } from './embedded-runtime-normalize.js';

/**
 * @typedef {import('./types.js').EmbeddedRuntimeAppliedState} EmbeddedRuntimeAppliedState
 * @typedef {import('./types.js').EmbeddedRuntimeSlot} EmbeddedRuntimeSlot
 * @typedef {import('./types.js').EmbeddedRuntimeProfile} EmbeddedRuntimeProfile
 * @typedef {{ slot: ReturnType<typeof normalizeEmbeddedRuntimeSlot>; appliedState: EmbeddedRuntimeAppliedState; parkReason: string; visible: boolean; inViewport: boolean; lastVisibleAt: number; lastTouchedAt: number }} SlotRecord
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

    /** @type {Map<string, SlotRecord>} */
    const slots = new Map();

    /** @type {number | null} */
    let pendingFrame = null;

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
     * Cancels a pending requestAnimationFrame reconcile, if any.
     */
    function cancelPendingReconcile() {
        if (pendingFrame === null) return;
        cancelAnimationFrame(pendingFrame);
        pendingFrame = null;
    }

    /** @param {string} reason */
    function requestReconcile(reason) {
        if (pendingFrame !== null) return;
        pendingFrame = requestAnimationFrame(() => {
            pendingFrame = null;
            reconcile(reason);
        });
    }

    /** @param {SlotRecord} record */
    function isResident(record) {
        return record.appliedState !== 'disposed'
            && (record.slot.isResident ? record.slot.isResident() : record.appliedState === 'active');
    }

    /** @param {SlotRecord} record @param {string} reason */
    function park(record, reason) {
        if (!isResident(record) && record.appliedState === 'parked' && record.parkReason === reason) {
            return;
        }
        record.slot.dehydrate(reason);
        if (record.slot.isResident?.()) {
            // Source capture can defer/refuse eviction. The retained page still
            // consumes its budget; do not report it as parked or grant its space.
            record.appliedState = 'active';
            record.parkReason = '';
            return;
        }
        if (record.appliedState === 'parked') {
            counters.parkReasonChange += 1;
        } else {
            counters.dehydrate += 1;
            if (reason === 'budget') {
                counters.parkBudget += 1;
            } else {
                counters.parkVisibility += 1;
            }
        }
        record.appliedState = 'parked';
        record.parkReason = reason;
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
            if (record.appliedState === 'disposed') {
                continue;
            }
            if (!record.slot.element.isConnected) {
                unregister(id);
                continue;
            }

            const keepAliveWhenHidden = !normalizedProfile.parkWhenHiddenKinds.has(record.slot.kind);
            const wantsActive = record.visible || keepAliveWhenHidden;
            if (!wantsActive) {
                continue;
            }

            // A missing page may still be reading its source or have failed.
            // It cannot displace a healthy resident until it is restorable.
            if (!isResident(record) && record.slot.canHydrate?.() === false) continue;

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

        const plannedUsage = { slots: 0, weight: 0, iframes: 0 };
        const residentUsage = { slots: 0, weight: 0, iframes: 0 };

        /** @param {typeof plannedUsage} usage @param {SlotRecord['slot']} slot */
        const fitsBudget = (usage, slot) => (
            (normalizedProfile.maxActiveSlots <= 0 || usage.slots + 1 <= normalizedProfile.maxActiveSlots)
            && (normalizedProfile.maxActiveWeight <= 0 || usage.weight + slot.weight <= normalizedProfile.maxActiveWeight)
            && (normalizedProfile.maxActiveIframes <= 0 || usage.iframes + slot.iframeCount <= normalizedProfile.maxActiveIframes)
        );

        /** @type {Set<string>} */
        const nextActive = new Set();

        for (const item of desired) {
            const record = slots.get(item.id);
            if (!record) {
                continue;
            }

            if (!fitsBudget(plannedUsage, record.slot)) {
                counters.budgetDeny += 1;
                continue;
            }

            nextActive.add(item.id);
            plannedUsage.slots += 1;
            plannedUsage.weight += record.slot.weight;
            plannedUsage.iframes += record.slot.iframeCount;
        }

        // Reclaim first, then budget against actual residency. In particular,
        // a renderer-created page can already be live before its first hydrate.
        for (const [id, record] of slots.entries()) {
            if (record.appliedState !== 'disposed' && !nextActive.has(id)) {
                const parkReason = record.visible || !normalizedProfile.parkWhenHiddenKinds.has(record.slot.kind)
                    ? 'budget'
                    : 'visibility';
                park(record, parkReason);
            }
        }

        for (const record of slots.values()) {
            if (isResident(record)) {
                residentUsage.weight += record.slot.weight;
                residentUsage.iframes += record.slot.iframeCount;
                residentUsage.slots += 1;
            }
        }

        for (const id of nextActive) {
            const record = /** @type {SlotRecord} */ (slots.get(id));
            const alreadyResident = isResident(record);
            if (alreadyResident && record.appliedState === 'active') {
                continue;
            }
            if (!alreadyResident && !fitsBudget(residentUsage, record.slot)) {
                counters.budgetDeny += 1;
                park(record, 'budget');
                continue;
            }
            record.slot.hydrate(reason);
            if (record.slot.isResident?.() === false) {
                record.appliedState = 'cold';
                record.parkReason = '';
                continue;
            }
            record.appliedState = 'active';
            record.parkReason = '';
            counters.hydrate += 1;
            if (!alreadyResident) {
                residentUsage.weight += record.slot.weight;
                residentUsage.iframes += record.slot.iframeCount;
                residentUsage.slots += 1;
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

        record.appliedState = 'disposed';
        if (record.slot.dispose) {
            record.slot.dispose();
        }
        slots.delete(id);
        counters.unregister += 1;
        requestReconcile('unregister');
    }

    /**
     * @param {string} id
     */
    function touch(id) {
        const record = slots.get(id);
        if (!record || record.appliedState === 'disposed') {
            throw new Error(`EmbeddedRuntimeManager.touch(${id}): slot not found`);
        }

        record.lastTouchedAt = now();
        requestReconcile('touch');
    }

    /**
     * Marks a slot as "dirty" so the next reconcile can re-assert its desired
     * state and (re)hydrate it if selected active.
     *
     * Refresh externally owned resource data synchronously, before a renderer
     * can remove it again. Only touch() changes interaction priority.
     *
     * @param {string} id
     */
    function invalidate(id) {
        const record = slots.get(id);
        if (!record || record.appliedState === 'disposed') {
            throw new Error(`EmbeddedRuntimeManager.invalidate(${id}): slot not found`);
        }

        record.slot.refresh?.();
        if (record.appliedState !== 'cold') {
            record.appliedState = 'cold';
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
        if (!record || record.appliedState === 'disposed') {
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
            appliedState: 'cold',
            parkReason: '',
            visible,
            inViewport,
            lastVisibleAt: visible ? now() : 0,
            lastTouchedAt: 0,
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
            if (isResident(record)) {
                active += 1;
                activeWeight += record.slot.weight;
                activeIframes += record.slot.iframeCount;
            } else if (record.appliedState === 'parked') {
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
        reconcile: () => {
            cancelPendingReconcile();
            reconcile('manual');
        },
        getPerfSnapshot,
        get profile() {
            return normalizedProfile.name;
        },
        get profileConfig() {
            return profileConfig;
        },
    };
}
