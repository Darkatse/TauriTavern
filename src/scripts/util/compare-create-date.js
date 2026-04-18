/**
 * @typedef {{ primaryMs: number | null; fallbackMs: number | null; avatar?: string; name?: string }} CreateDateSortKey
 */

/**
 * Total-order comparator for create-date style sorting (ascending / oldest first).
 *
 * Primary sort key: `primaryMs` (parsed `create_date` timestamp).
 * Fallback sort key: `fallbackMs` (e.g., `date_added`), also used to break ties.
 * Deterministic tie-breakers: `avatar`, then `name`.
 *
 * @param {CreateDateSortKey} a
 * @param {CreateDateSortKey} b
 * @returns {number}
 */
export function compareCreateDateKeysAscending(a, b) {
    const aPrimary = a.primaryMs;
    const bPrimary = b.primaryMs;
    if (aPrimary !== null && bPrimary !== null && aPrimary !== bPrimary) {
        return aPrimary - bPrimary;
    }

    const aEffective = aPrimary ?? a.fallbackMs;
    const bEffective = bPrimary ?? b.fallbackMs;
    if (aEffective !== null && bEffective !== null && aEffective !== bEffective) {
        return aEffective - bEffective;
    }

    const aFallback = a.fallbackMs;
    const bFallback = b.fallbackMs;
    if (aFallback !== null && bFallback !== null && aFallback !== bFallback) {
        return aFallback - bFallback;
    }

    const aAvatar = String(a.avatar ?? '');
    const bAvatar = String(b.avatar ?? '');
    const avatarDiff = aAvatar.localeCompare(bAvatar);
    if (avatarDiff !== 0) {
        return avatarDiff;
    }

    const aName = String(a.name ?? '');
    const bName = String(b.name ?? '');
    return aName.localeCompare(bName);
}
