// @ts-check

export {};

/**
 * Last applied lifecycle result; cold also means invalidated. Actual residency
 * is supplied by the slot when its renderer can change resources independently.
 * @typedef {'cold' | 'active' | 'parked' | 'disposed'} EmbeddedRuntimeAppliedState
 */

/**
 * @typedef {'intersection' | 'manual'} EmbeddedRuntimeVisibilityMode
 */

/**
 * @typedef {object} EmbeddedRuntimeSlot
 * @property {string} id
 * @property {string} kind
 * @property {HTMLElement} element
 * @property {number} [priority]
 * @property {number} [weight]
 * @property {number} [iframeCount]
 * @property {EmbeddedRuntimeVisibilityMode} [visibilityMode]
 * @property {boolean} [initialVisible]
 * @property {HTMLElement} [visibilityTarget]
 * @property {() => boolean} [isResident] Actual residency when the renderer can change it or defer a transition.
 * @property {() => boolean} [canHydrate] Whether a missing resource can be restored now; omitted for always-ready slots.
 * @property {() => void} [refresh] Refresh externally owned resource data without moving its DOM.
 * @property {(reason: string) => void} hydrate
 * @property {(reason: string) => void} dehydrate
 * @property {() => void} [dispose]
 */

/**
 * @typedef {object} EmbeddedRuntimeProfile
 * @property {string} name
 * @property {number} maxActiveWeight
 * @property {number} maxActiveIframes
 * @property {number} maxActiveSlots
 * @property {number} maxSoftParkedIframes
 * @property {number} softParkTtlMs
 * @property {ReadonlyArray<string>} parkWhenHiddenKinds
 * @property {string} [rootMargin]
 * @property {number} [threshold]
 */

/**
 * @typedef {object} EmbeddedRuntimePerfSnapshot
 * @property {string} profile
 * @property {number} registered
 * @property {number} visible
 * @property {number} inViewport
 * @property {number} active
 * @property {number} parked
 * @property {number} activeWeight
 * @property {number} activeIframes
 * @property {{ hydrate: number; dehydrate: number; parkVisibility: number; parkBudget: number; parkReasonChange: number; register: number; unregister: number; reconcile: number; lastReconcileMs: number; lastReconcileAt: number; budgetDeny: number }} counters
 */
