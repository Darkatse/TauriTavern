import { errorText } from './host-api';

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
    listRuns: (input: RunHistoryListInput) => ReturnType<TauriTavernAgentApi['listRuns']>;
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
 * never overwrites a newer one (last request wins).
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

    function commit(patch: Partial<RunHistorySnapshot>): void {
        if (disposed) {
            return;
        }
        snapshot = { ...snapshot, ...patch };
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
        commit({ loading: true, loadingMore: false, error: '' });
        try {
            const input = await buildListInput();
            const result = await deps.listRuns(input);
            if (disposed || epoch !== requestEpoch) {
                return;
            }
            commit({ runs: result.runs, nextCursor: result.nextCursor ?? null });
        } catch (error) {
            if (disposed || epoch !== requestEpoch) {
                return;
            }
            commit({ error: errorText(error), runs: [], nextCursor: null });
        } finally {
            if (!disposed && epoch === requestEpoch) {
                commit({ loading: false });
            }
        }
    }

    async function loadMore(): Promise<void> {
        if (snapshot.loading || snapshot.loadingMore || !snapshot.nextCursor) {
            return;
        }
        const epoch = requestEpoch;
        const before = snapshot.nextCursor;
        commit({ loadingMore: true, error: '' });
        try {
            const input = await buildListInput({ before });
            const result = await deps.listRuns(input);
            if (disposed || epoch !== requestEpoch) {
                return;
            }
            commit({
                runs: [...snapshot.runs, ...result.runs],
                nextCursor: result.nextCursor ?? null,
            });
        } catch (error) {
            if (disposed || epoch !== requestEpoch) {
                return;
            }
            commit({ error: errorText(error) });
        } finally {
            if (!disposed && epoch === requestEpoch) {
                commit({ loadingMore: false });
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
            commit({ filter: next });
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
