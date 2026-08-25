import { errorText } from './host-api.js';

const RUN_HISTORY_PAGE_LIMIT = 50;
const TERMINAL_RUN_STATUSES: readonly TauriTavernAgentRunStatus[] = Object.freeze([
    'completed',
    'partial_success',
    'cancelled',
    'failed',
]);

export type RunHistoryFilter = 'all' | 'current';

export type RunHistoryListInput = {
    statuses: TauriTavernAgentRunStatus[];
    limit: number;
    before?: TauriTavernAgentRunListCursor;
    chatRef?: TauriTavernChatRef;
    stableChatId?: string;
};

export type RunHistorySnapshot = {
    runs: TauriTavernAgentRunSummary[];
    nextCursor: TauriTavernAgentRunListCursor | null;
    loading: boolean;
    loadingMore: boolean;
    filter: RunHistoryFilter;
    error: string;
};

export type RunHistoryControllerDeps = {
    listRuns: (input: RunHistoryListInput) => Promise<unknown>;
    currentChatRunFilter: () => Promise<{ chatRef: TauriTavernChatRef; stableChatId: string }>;
    openRun: (run: TauriTavernAgentRunSummary) => void;
};

export type RunHistoryController = {
    getSnapshot: () => RunHistorySnapshot;
    subscribe: (listener: () => void) => () => void;
    refresh: () => Promise<void>;
    loadMore: () => Promise<void>;
    setFilter: (filter: string) => Promise<void>;
    openRun: (run: TauriTavernAgentRunSummary) => void;
    dispose: () => void;
};

/**
 * Mount-local owner of the run history list. Every refresh/loadMore runs
 * under a request epoch so a slower older response (e.g. a previous filter)
 * never overwrites a newer one (Fix 5, last-request-wins).
 */
export function createRunHistoryController(deps: RunHistoryControllerDeps): RunHistoryController {
    let snapshot: RunHistorySnapshot = {
        runs: [],
        nextCursor: null,
        loading: false,
        loadingMore: false,
        filter: 'all',
        error: '',
    };
    const listeners = new Set<() => void>();
    let disposed = false;
    let requestEpoch = 0;

    function commit(next: RunHistorySnapshot): void {
        if (disposed) {
            return;
        }
        snapshot = next;
        for (const listener of listeners) {
            listener();
        }
    }

    async function buildListInput(extra: { before?: TauriTavernAgentRunListCursor } = {}): Promise<RunHistoryListInput> {
        const input: RunHistoryListInput = {
            statuses: [...TERMINAL_RUN_STATUSES],
            limit: RUN_HISTORY_PAGE_LIMIT,
            ...extra,
        };
        if (snapshot.filter === 'current') {
            const currentChat = await deps.currentChatRunFilter();
            input.chatRef = currentChat.chatRef;
            input.stableChatId = currentChat.stableChatId;
        }
        return input;
    }

    async function refresh(): Promise<void> {
        const epoch = ++requestEpoch;
        commit({ ...snapshot, loading: true, loadingMore: false, error: '' });
        try {
            const input = await buildListInput();
            const result = normalizeRunHistoryResult(await deps.listRuns(input));
            if (disposed || epoch !== requestEpoch) {
                return;
            }
            commit({ ...snapshot, runs: result.runs, nextCursor: result.nextCursor });
        } catch (error) {
            if (disposed || epoch !== requestEpoch) {
                return;
            }
            commit({ ...snapshot, error: errorText(error), runs: [], nextCursor: null });
        } finally {
            if (!disposed && epoch === requestEpoch) {
                commit({ ...snapshot, loading: false });
            }
        }
    }

    async function loadMore(): Promise<void> {
        if (snapshot.loading || snapshot.loadingMore || !snapshot.nextCursor) {
            return;
        }
        const epoch = requestEpoch;
        const before = snapshot.nextCursor;
        commit({ ...snapshot, loadingMore: true, error: '' });
        try {
            const input = await buildListInput({ before });
            const result = normalizeRunHistoryResult(await deps.listRuns(input));
            if (disposed || epoch !== requestEpoch) {
                return;
            }
            commit({ ...snapshot, runs: [...snapshot.runs, ...result.runs], nextCursor: result.nextCursor });
        } catch (error) {
            if (disposed || epoch !== requestEpoch) {
                return;
            }
            commit({ ...snapshot, error: errorText(error) });
        } finally {
            if (!disposed && epoch === requestEpoch) {
                commit({ ...snapshot, loadingMore: false });
            }
        }
    }

    return {
        getSnapshot: () => snapshot,
        subscribe(listener) {
            listeners.add(listener);
            return () => {
                listeners.delete(listener);
            };
        },
        refresh,
        loadMore,
        async setFilter(filter: string): Promise<void> {
            const next: RunHistoryFilter = filter === 'current' ? 'current' : 'all';
            if (next === snapshot.filter) {
                return;
            }
            commit({ ...snapshot, filter: next });
            await refresh();
        },
        openRun(run) {
            deps.openRun(run);
        },
        dispose(): void {
            if (disposed) {
                return;
            }
            disposed = true;
            listeners.clear();
        },
    };
}

function normalizeRunHistoryResult(value: unknown): {
    runs: TauriTavernAgentRunSummary[];
    nextCursor: TauriTavernAgentRunListCursor | null;
} {
    if (!plainObject(value)) {
        throw new Error('agent.run_history_result_invalid: result must be an object');
    }
    if (!Array.isArray(value.runs)) {
        throw new Error('agent.run_history_result_invalid: result.runs must be an array');
    }
    if (value.nextCursor != null && !plainObject(value.nextCursor)) {
        throw new Error('agent.run_history_result_invalid: result.nextCursor must be an object or null');
    }
    return {
        runs: value.runs as TauriTavernAgentRunSummary[],
        nextCursor: (value.nextCursor || null) as TauriTavernAgentRunListCursor | null,
    };
}

function plainObject(value: unknown): value is Record<string, unknown> {
    if (!value || typeof value !== 'object' || Array.isArray(value)) {
        return false;
    }
    const prototype = Object.getPrototypeOf(value) as unknown;
    return prototype === Object.prototype || prototype === null;
}
