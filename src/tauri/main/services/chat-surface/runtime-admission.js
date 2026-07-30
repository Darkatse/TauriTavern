// @ts-check

import { createResourceLease } from './resource-lease.js';

const DEFAULT_RUNTIME_LIMIT = 8;

/** @param {unknown} value */
function toError(value) {
    return value instanceof Error ? value : new Error(String(value));
}

/**
 * Owns runtime grants independently from message content lifetimes. Candidate
 * registration is synchronous; managed activation is serialized one grant per
 * animation frame so expensive browsing contexts cannot burst into existence.
 *
 * @param {{
 *   activate: (record: any, candidate: any, runtimeLease: ReturnType<typeof createResourceLease>) => void;
 *   assertCandidate: (record: any, candidate: any) => void;
 *   runScheduled: (operation: () => void) => void;
 *   onFault: (error: unknown) => unknown;
 *   schedule?: (callback: FrameRequestCallback) => number;
 *   cancel?: (handle: number) => void;
 * }} deps
 */
export function createRuntimeAdmission({
    activate,
    assertCandidate,
    runScheduled,
    onFault,
    schedule = callback => requestAnimationFrame(callback),
    cancel = handle => cancelAnimationFrame(handle),
}) {
    if (
        typeof activate !== 'function'
        || typeof assertCandidate !== 'function'
        || typeof runScheduled !== 'function'
        || typeof onFault !== 'function'
        || typeof schedule !== 'function'
        || typeof cancel !== 'function'
    ) {
        throw new TypeError('RuntimeAdmission requires activation, validation, mutation, fault and scheduler functions');
    }

    /** @type {'eager' | 'managed'} */
    let mode = 'eager';
    let maxActive = DEFAULT_RUNTIME_LIMIT;
    /** @type {Map<string, any>} */
    const entries = new Map();
    /** @type {Map<string, string[]>} */
    const entryIdsByMountKey = new Map();
    /** @type {string[]} */
    let demand = [];
    let suspended = false;
    let nextCandidateId = 1;
    /** @type {number | null} */
    let scheduled = null;
    /** @type {Error | null} */
    let fault = null;
    let disposed = false;

    function assertHealthy() {
        if (disposed) {
            throw new Error('RuntimeAdmission is disposed');
        }
        if (fault) {
            const error = /** @type {Error & { cause?: unknown }} */ (new Error('RuntimeAdmission is faulted'));
            error.cause = fault;
            throw error;
        }
    }

    /** @param {unknown} error */
    function fail(error) {
        fault ??= toError(error);
        onFault(fault);
        return fault;
    }

    function cancelScheduled() {
        if (scheduled === null) {
            return;
        }
        cancel(scheduled);
        scheduled = null;
    }

    /** @param {any} entry @param {string} reason */
    function revoke(entry, reason) {
        if (entry.status !== 'active') {
            return;
        }
        const lease = entry.runtimeLease;
        entry.runtimeLease = null;
        entry.status = 'pending';
        lease.close(reason);
    }

    /** @param {any} entry */
    function grant(entry) {
        if (entry.status !== 'pending') {
            throw new Error(`RuntimeAdmission cannot grant candidate in state ${entry.status}`);
        }
        assertCandidate(entry.record, entry.candidate);
        const runtimeLease = createResourceLease();
        entry.runtimeLease = runtimeLease;
        entry.status = 'active';
        try {
            activate(entry.record, entry.candidate, runtimeLease);
            assertCandidate(entry.record, entry.candidate);
        } catch (error) {
            entry.runtimeLease = null;
            entry.status = 'pending';
            try {
                runtimeLease.close('activation-failed');
            } catch (cleanupError) {
                const failure = /** @type {Error & { cause?: unknown; cleanupCause?: unknown }} */ (
                    new Error(`RuntimeAdmission failed to activate ${entry.id}`)
                );
                failure.cause = error;
                failure.cleanupCause = cleanupError;
                throw failure;
            }
            throw error;
        }
    }

    function desiredEntryIds() {
        const desired = [];
        for (const mountKey of demand) {
            for (const entryId of entryIdsByMountKey.get(mountKey) ?? []) {
                desired.push(entryId);
                if (desired.length === maxActive) {
                    return desired;
                }
            }
        }
        return desired;
    }

    function scheduleNext() {
        if (scheduled !== null || suspended || disposed || fault) {
            return;
        }
        const next = desiredEntryIds()
            .map(entryId => entries.get(entryId))
            .find(entry => entry?.status === 'pending');
        if (!next) {
            return;
        }
        scheduled = schedule(() => {
            scheduled = null;
            try {
                runScheduled(() => {
                    assertHealthy();
                    const desiredIds = new Set(desiredEntryIds());
                    const current = entries.get(next.id);
                    if (current?.status === 'pending' && desiredIds.has(current.id) && !suspended) {
                        grant(current);
                    }
                    reconcileManaged();
                });
            } catch (error) {
                fail(error);
            }
        });
    }

    function reconcileManaged() {
        if (mode !== 'managed') {
            return;
        }
        const desiredIds = new Set(desiredEntryIds());
        for (const entry of entries.values()) {
            if (entry.status === 'active' && !desiredIds.has(entry.id)) {
                revoke(entry, 'runtime-demand-revoked');
            }
        }
        if (suspended) {
            cancelScheduled();
            return;
        }
        scheduleNext();
    }

    /** @param {'eager' | 'managed'} nextMode @param {{ maxActive?: number }} [options] */
    function configure(nextMode, { maxActive: nextMaxActive = DEFAULT_RUNTIME_LIMIT } = {}) {
        assertHealthy();
        if (entries.size !== 0) {
            throw new Error('RuntimeAdmission policy can only change while the surface is empty');
        }
        if (nextMode !== 'eager' && nextMode !== 'managed') {
            throw new Error(`Unsupported RuntimeAdmission mode: ${String(nextMode)}`);
        }
        if (!Number.isInteger(nextMaxActive) || nextMaxActive < 1) {
            throw new TypeError('RuntimeAdmission maxActive must be a positive integer');
        }
        mode = nextMode;
        maxActive = nextMaxActive;
        demand = [];
        suspended = false;
        cancelScheduled();
    }

    /** @param {Array<{ record: any; candidate: any }>} candidates */
    function register(candidates) {
        assertHealthy();
        if (!Array.isArray(candidates)) {
            throw new TypeError('RuntimeAdmission candidates must be an array');
        }
        /** @type {any[]} */
        const registered = [];
        try {
            for (const { record, candidate } of candidates) {
                assertCandidate(record, candidate);
                const id = `runtime-candidate-${nextCandidateId}`;
                nextCandidateId += 1;
                const entry = {
                    id,
                    record,
                    candidate,
                    status: 'pending',
                    runtimeLease: null,
                };
                entries.set(id, entry);
                const mountEntries = entryIdsByMountKey.get(record.mountKey) ?? [];
                mountEntries.push(id);
                entryIdsByMountKey.set(record.mountKey, mountEntries);
                record.contentLease.add(() => unregister(id, 'content-released'));
                registered.push(entry);
            }
            if (mode === 'eager') {
                for (const entry of registered) {
                    grant(entry);
                }
            } else {
                reconcileManaged();
            }
        } catch (error) {
            throw fail(error);
        }
    }

    /** @param {string} entryId @param {string} reason */
    function unregister(entryId, reason) {
        const entry = entries.get(entryId);
        if (!entry) {
            return;
        }
        revoke(entry, reason);
        entry.status = 'disposed';
        entries.delete(entryId);
        const mountEntries = entryIdsByMountKey.get(entry.record.mountKey);
        if (mountEntries) {
            const index = mountEntries.indexOf(entryId);
            if (index >= 0) {
                mountEntries.splice(index, 1);
            }
            if (mountEntries.length === 0) {
                entryIdsByMountKey.delete(entry.record.mountKey);
            }
        }
        if (mode === 'managed') {
            reconcileManaged();
        }
    }

    /** @param {string[]} mountKeys @param {{ suspended?: boolean }} [options] */
    function setDemand(mountKeys, { suspended: nextSuspended = false } = {}) {
        assertHealthy();
        if (mode !== 'managed') {
            throw new Error('RuntimeAdmission demand is only valid in managed mode');
        }
        if (!Array.isArray(mountKeys) || mountKeys.some(key => typeof key !== 'string' || key.length === 0)) {
            throw new TypeError('RuntimeAdmission demand must be an array of mount keys');
        }
        if (new Set(mountKeys).size !== mountKeys.length) {
            throw new Error('RuntimeAdmission demand contains duplicate mount keys');
        }
        demand = mountKeys.slice();
        suspended = Boolean(nextSuspended);
        try {
            reconcileManaged();
        } catch (error) {
            throw fail(error);
        }
    }

    function snapshot() {
        let active = 0;
        let pending = 0;
        const candidates = [];
        for (const entry of entries.values()) {
            if (entry.status === 'active') {
                active += 1;
            } else {
                pending += 1;
            }
            candidates.push(Object.freeze({
                id: entry.id,
                participantId: entry.candidate.participantId,
                mountKey: entry.record.mountKey,
                messageId: entry.record.messageId,
                state: entry.status,
            }));
        }
        return Object.freeze({
            mode,
            maxActive,
            active,
            pending,
            scheduled: scheduled !== null,
            suspended,
            candidates: Object.freeze(candidates),
            fault,
        });
    }

    function resetEpoch() {
        if (entries.size !== 0) {
            throw new Error('RuntimeAdmission cannot reset while candidates remain registered');
        }
        cancelScheduled();
        demand = [];
        suspended = false;
        fault = null;
    }

    function dispose() {
        if (disposed) {
            return;
        }
        cancelScheduled();
        let firstFailure;
        for (const entry of [...entries.values()].reverse()) {
            try {
                unregister(entry.id, 'runtime-admission-disposed');
            } catch (error) {
                firstFailure ??= error;
            }
        }
        disposed = true;
        if (firstFailure !== undefined) {
            throw firstFailure;
        }
    }

    return Object.freeze({ configure, register, setDemand, resetEpoch, snapshot, dispose });
}
