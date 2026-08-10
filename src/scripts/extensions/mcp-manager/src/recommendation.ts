const EXA_SEARCH = {
    displayName: 'Exa Search',
    endpoint: 'https://mcp.exa.ai/mcp',
};

const HANDLED_MARKER = {
    namespace: 'tauritavern.mcp-manager',
    key: 'exa-search-recommendation-handled',
};

type McpServerList = Awaited<ReturnType<TauriTavernMcpApi['servers']['list']>>;
type RecommendationStore = Pick<TauriTavernExtensionStoreApi, 'tryGetJson' | 'setJson'>;

export async function ensureExaRecommendation(
    initial: McpServerList,
    create: TauriTavernMcpApi['servers']['create'],
    store: RecommendationStore,
): Promise<{ initial: McpServerList; error?: unknown }> {
    let next = initial;
    try {
        if ((await store.tryGetJson(HANDLED_MARKER)).found) {
            return { initial };
        }

        if (!initial.servers.some(server => server.endpoint === EXA_SEARCH.endpoint)) {
            const server = await create(EXA_SEARCH);
            next = { ...initial, servers: [...initial.servers, server] };
        }

        await store.setJson({ ...HANDLED_MARKER, value: true });
        return { initial: next };
    } catch (error) {
        return { initial: next, error };
    }
}
