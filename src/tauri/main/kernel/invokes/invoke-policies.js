// @ts-check

import { fnv1a32 } from '../hash-utils.js';

/**
 * @typedef {import('./tauri-commands.js').TauriInvokeCommand} TauriInvokeCommand
 *
 * @typedef {(
 *   command: TauriInvokeCommand | string,
 *   args?: any,
 * ) => Promise<any>} InvokeTransport
 *
 * @typedef {{
 *   kind: 'dedupe';
 *   key?: ((args: any) => string) | undefined;
 *   cacheTtlMs?: number | undefined;
 *   cacheLimit?: number | undefined;
 *   maxConcurrent?: number | undefined;
 *   timeoutMs?: number | undefined;
 * }} DedupePolicy
 *
 * @typedef {{
 *   kind: 'writeBehind';
 *   key?: ((args: any) => string) | undefined;
 *   delayMs?: number | undefined;
 *   merge?: ((previousArgs: any, nextArgs: any) => any) | undefined;
 *   maxConcurrent?: number | undefined;
 *   timeoutMs?: number | undefined;
 * }} WriteBehindPolicy
 *
 * @typedef {DedupePolicy | WriteBehindPolicy} InvokePolicy
 * @typedef {Partial<Record<TauriInvokeCommand, InvokePolicy>> & Record<string, InvokePolicy>} HostInvokePolicies
 */

/**
 * @param {any} args
 * @returns {string}
 */
function readThumbnailAssetCacheKey(args) {
    const type = String(args?.thumbnailType ?? args?.thumbnail_type ?? '').trim().toLowerCase();
    const file = String(args?.file ?? '').trim();
    const animated = Boolean(args?.animated);
    return `${type}|${animated ? 1 : 0}|${file}`;
}

/**
 * @param {any} args
 * @returns {string}
 */
function countOpenAiTokensBatchKey(args) {
    const dto = args?.dto ?? args ?? {};
    const json = JSON.stringify(dto);
    return fnv1a32(json);
}

/** @param {any} _prev @param {any} next */
function takeLatest(_prev, next) {
    return next;
}

/**
 * Centralized invoke policies for the host kernel.
 *
 * Keep this module free of higher-level imports (services/routes/adapters).
 *
 * @param {{
 *   thumbnailBlobCacheLimit: number;
 * }} deps
 * @returns {HostInvokePolicies}
 */
export function createHostInvokePolicies({ thumbnailBlobCacheLimit }) {
    const thumbnailCacheLimit = Math.max(0, Math.floor(Number(thumbnailBlobCacheLimit) || 0));

    return {
        get_bootstrap_snapshot: {
            kind: 'dedupe',
            key: () => 'singleton',
        },
        get_sillytavern_settings: {
            kind: 'dedupe',
            key: () => 'singleton',
        },
        get_client_version: {
            kind: 'dedupe',
            key: () => 'singleton',
            cacheTtlMs: 5_000,
            cacheLimit: 1,
        },
        read_thumbnail_asset: {
            kind: 'dedupe',
            maxConcurrent: 2,
            cacheTtlMs: 30_000,
            cacheLimit: thumbnailCacheLimit,
            key: readThumbnailAssetCacheKey,
        },
        count_openai_tokens_batch: {
            kind: 'dedupe',
            maxConcurrent: 1,
            cacheTtlMs: 2_000,
            cacheLimit: 50,
            key: countOpenAiTokensBatchKey,
        },
        save_user_settings: {
            kind: 'writeBehind',
            delayMs: 300,
            maxConcurrent: 1,
            key: () => 'singleton',
            merge: takeLatest,
        },
    };
}
