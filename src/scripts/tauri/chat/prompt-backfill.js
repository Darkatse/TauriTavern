import { stripCommandErrorPrefixes } from '../../util/command-error-utils.js';
import {
    loadCharacterChatPayloadBeforePages,
    loadGroupChatPayloadBeforePages,
    normalizeChatFileName,
    resolveCharacterDirectoryId,
} from '../../chat-payload-transport.js';
import { isMobileRuntime } from './platform.js';
import {
    DEFAULT_CHAT_WINDOW_LINES_DESKTOP,
    DEFAULT_CHAT_WINDOW_LINES_MOBILE,
} from './windowed-defaults.js';

const CHARS_PER_TOKEN_RATIO = 3.35;
const TARGET_CONTEXT_UTILIZATION = 0.75;

const DEFAULT_MAX_PAGES_DESKTOP = 6;
const DEFAULT_MAX_PAGES_MOBILE = 4;

const DEFAULT_MAX_MESSAGES_DESKTOP = 800;
const DEFAULT_MAX_MESSAGES_MOBILE = 400;

const DEFAULT_PREFETCH_PAGES_DESKTOP = 3;
const DEFAULT_PREFETCH_PAGES_MOBILE = 2;

const DEFAULT_CACHE_PAGES_DESKTOP = 12;
const DEFAULT_CACHE_PAGES_MOBILE = 8;
const DEFAULT_CACHE_PAGES = isMobileRuntime()
    ? DEFAULT_CACHE_PAGES_MOBILE
    : DEFAULT_CACHE_PAGES_DESKTOP;

/** @type {Map<string, { messages: any[], cursor: any, hasMoreBefore: boolean }>} */
const beforePageCache = new Map();

function buildWindowCacheId(windowState) {
    if (windowState.kind === 'group') {
        return `group:${normalizeChatFileName(windowState.id)}`;
    }

    const characterId = resolveCharacterDirectoryId(windowState.characterName, windowState.avatarUrl);
    const chatId = normalizeChatFileName(windowState.fileName);
    return `character:${characterId}|${chatId}`;
}

function buildCursorSignature(cursor) {
    const offset = cursor?.offset ?? '';
    const size = cursor?.size ?? '';
    const modifiedMillis = cursor?.modifiedMillis ?? cursor?.modified_millis ?? '';
    return `${offset}:${size}:${modifiedMillis}`;
}

function buildBeforePageCacheKey(windowState, cursor, maxLines) {
    return `${buildWindowCacheId(windowState)}|${buildCursorSignature(cursor)}|${String(maxLines || '')}`;
}

function getCachedBeforePage(cacheKey) {
    if (!beforePageCache.has(cacheKey)) {
        return null;
    }

    const cached = beforePageCache.get(cacheKey);
    beforePageCache.delete(cacheKey);
    beforePageCache.set(cacheKey, cached);
    return cached;
}

function setCachedBeforePage(cacheKey, value) {
    beforePageCache.set(cacheKey, value);

    while (beforePageCache.size > DEFAULT_CACHE_PAGES) {
        const firstKey = beforePageCache.keys().next().value;
        beforePageCache.delete(firstKey);
    }
}

function extractErrorMessage(error) {
    if (!error) {
        return '';
    }

    if (typeof error === 'string') {
        return error;
    }

    if (typeof error?.message === 'string') {
        return error.message;
    }

    try {
        return JSON.stringify(error);
    } catch {
        return String(error);
    }
}

export function isWindowedCursorInvalidError(error) {
    const normalized = stripCommandErrorPrefixes(extractErrorMessage(error)).toLowerCase();
    if (!normalized.includes('cursor')) {
        return false;
    }

    return normalized.includes('signature mismatch')
        || normalized.includes('line boundary')
        || normalized.includes('out of bounds')
        || normalized.includes('before chat payload body');
}

function estimateChatChars(messages) {
    let total = 0;
    for (const message of messages) {
        const text = message?.mes;
        if (typeof text === 'string') {
            total += text.length;
        }
    }
    return total;
}

async function loadWindowedBeforePage(windowState, cursor, maxLines, prefetchPages = 1) {
    const cacheKey = buildBeforePageCacheKey(windowState, cursor, maxLines);
    const cached = getCachedBeforePage(cacheKey);
    if (cached) {
        return cached;
    }

    const resolvedPrefetchPages = Number(prefetchPages);
    if (!Number.isFinite(resolvedPrefetchPages) || resolvedPrefetchPages < 1) {
        throw new Error('Invalid windowed backfill prefetch page count');
    }

    const pages = windowState.kind === 'group'
        ? await loadGroupChatPayloadBeforePages({
            id: windowState.id,
            cursor,
            maxLines,
            maxPages: resolvedPrefetchPages,
        })
        : await loadCharacterChatPayloadBeforePages({
            characterName: windowState.characterName,
            avatarUrl: windowState.avatarUrl,
            fileName: windowState.fileName,
            cursor,
            maxLines,
            maxPages: resolvedPrefetchPages,
        });

    if (!Array.isArray(pages) || pages.length === 0) {
        throw new Error('Windowed payload before pages response is empty');
    }

    let nextCursor = cursor;
    for (const page of pages) {
        const key = buildBeforePageCacheKey(windowState, nextCursor, maxLines);
        setCachedBeforePage(key, page);
        nextCursor = page.cursor;

        if (!page.hasMoreBefore || !Array.isArray(page.messages) || page.messages.length === 0) {
            break;
        }
    }

    return pages[0];
}

/**
 * Builds an in-memory history array for a single Generate() run by reading additional windowed pages as needed.
 *
 * Notes:
 * - Does not mutate the global UI chat array.
 * - Does not normalize/patch message shape; call sites can normalize if needed.
 * - Throws on transport errors; call sites decide whether to degrade (e.g. cursor invalid).
 */
export async function buildGenerationChatWithBackfill({
    baseMessages,
    windowState,
    contextBudgetTokens,
    pageSizeLines,
    maxPages,
    maxMessages,
} = {}) {
    const sourceMessages = Array.isArray(baseMessages) ? baseMessages : [];
    if (!windowState?.cursor || !windowState?.hasMoreBefore) {
        return { chat: sourceMessages, added: [] };
    }

    const mobile = isMobileRuntime();
    const resolvedPageSize = Number(pageSizeLines)
        || (mobile ? DEFAULT_CHAT_WINDOW_LINES_MOBILE : DEFAULT_CHAT_WINDOW_LINES_DESKTOP);
    const resolvedMaxPages = Number(maxPages)
        || (mobile ? DEFAULT_MAX_PAGES_MOBILE : DEFAULT_MAX_PAGES_DESKTOP);
    const resolvedMaxMessages = Number(maxMessages)
        || (mobile ? DEFAULT_MAX_MESSAGES_MOBILE : DEFAULT_MAX_MESSAGES_DESKTOP);

    const budget = Number(contextBudgetTokens);
    const targetTokens = Number.isFinite(budget) && budget > 0
        ? budget * TARGET_CONTEXT_UTILIZATION
        : 0;

    let cursor = windowState.cursor;
    let hasMoreBefore = Boolean(windowState.hasMoreBefore);
    let pagesLoaded = 0;
    let addedCount = 0;
    let charsTotal = estimateChatChars(sourceMessages);

    /** @type {any[][]} */
    const pages = [];

    while (hasMoreBefore && pagesLoaded < resolvedMaxPages) {
        if (resolvedMaxMessages > 0 && sourceMessages.length + addedCount >= resolvedMaxMessages) {
            break;
        }

        const estimatedTokens = charsTotal / CHARS_PER_TOKEN_RATIO;
        if (targetTokens > 0 && estimatedTokens >= targetTokens) {
            break;
        }

        const remainingCapacity = resolvedMaxMessages > 0
            ? Math.max(0, resolvedMaxMessages - sourceMessages.length - addedCount)
            : resolvedPageSize;
        if (remainingCapacity === 0) {
            break;
        }

        const maxLines = Math.min(resolvedPageSize, remainingCapacity);
        const prefetchBatch = mobile ? DEFAULT_PREFETCH_PAGES_MOBILE : DEFAULT_PREFETCH_PAGES_DESKTOP;
        const remainingPages = resolvedMaxPages - pagesLoaded;
        const result = await loadWindowedBeforePage(
            windowState,
            cursor,
            maxLines,
            Math.min(prefetchBatch, remainingPages),
        );

        const pageMessages = result?.messages;
        if (!Array.isArray(pageMessages) || pageMessages.length === 0) {
            hasMoreBefore = false;
            cursor = result?.cursor ?? cursor;
            break;
        }

        pages.push(pageMessages);
        addedCount += pageMessages.length;
        charsTotal += estimateChatChars(pageMessages);
        pagesLoaded += 1;

        cursor = result.cursor;
        hasMoreBefore = Boolean(result.hasMoreBefore);
    }

    if (pages.length === 0) {
        return { chat: sourceMessages, added: [] };
    }

    const added = [];
    for (let pageIndex = pages.length - 1; pageIndex >= 0; pageIndex -= 1) {
        const page = pages[pageIndex];
        if (!Array.isArray(page)) {
            continue;
        }

        for (const message of page) {
            added.push(message);
        }
    }
    return {
        chat: [...added, ...sourceMessages],
        added,
    };
}
