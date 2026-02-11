const STATS_STORAGE_KEY = 'tauritavern.stats.v1';
let memoryStats = {};

function isPlainObject(value) {
    return value && typeof value === 'object' && !Array.isArray(value);
}

function getStorage() {
    try {
        return window?.localStorage ?? null;
    } catch {
        return null;
    }
}

function loadStats() {
    const storage = getStorage();

    if (!storage) {
        return memoryStats;
    }

    try {
        const raw = storage.getItem(STATS_STORAGE_KEY);
        if (!raw) {
            return memoryStats;
        }

        const parsed = JSON.parse(raw);
        if (!isPlainObject(parsed)) {
            return memoryStats;
        }

        memoryStats = parsed;
        return memoryStats;
    } catch (error) {
        console.warn('Failed to read local stats cache:', error);
        return memoryStats;
    }
}

function saveStats(stats) {
    const nextStats = isPlainObject(stats) ? stats : {};
    memoryStats = nextStats;

    const storage = getStorage();
    if (!storage) {
        return;
    }

    try {
        storage.setItem(STATS_STORAGE_KEY, JSON.stringify(nextStats));
    } catch (error) {
        console.warn('Failed to persist local stats cache:', error);
    }
}

export function registerStatsRoutes(router, _context, { jsonResponse }) {
    router.post('/api/stats/get', async () => {
        return jsonResponse(loadStats());
    });

    router.post('/api/stats/update', async ({ body }) => {
        saveStats(body);
        return jsonResponse({ ok: true });
    });

    router.post('/api/stats/recreate', async () => {
        // Tauri backend currently stores stats in lightweight local cache.
        // Recreate keeps compatibility and succeeds without destructive reset.
        return jsonResponse({ ok: true });
    });
}
