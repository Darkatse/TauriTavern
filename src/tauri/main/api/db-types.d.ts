export type Json = null | boolean | number | string | Json[] | { [key: string]: Json };
/** Non-negative JavaScript safe integer. */
export type NodeId = number;
export type Direction = 'outgoing' | 'incoming' | 'both';

export interface OpenOptions {
    dim?: number;
    storageMode?: 'mmap' | 'rom';
    syncMode?: 'normal' | 'full' | 'off';
    /** Preload preference. Existing text indexes are always loaded to preserve them. */
    loadTextIndex?: boolean;
    autoBuildQuiver?: boolean;
    memoryLimitMb?: number;
}

export interface SearchOptions {
    topK?: number;
    recallK?: number;
    rerankK?: number;
    expandDepth?: number;
    expandLabels?: string[];
    maxEdgesPerNode?: number;
    minEdgeWeight?: number;
    edgeDirection?: Direction;
    minScore?: number;
    teleportAlpha?: number;
    enableAdvancedPipeline?: boolean;
    enableSparseResidual?: boolean;
    fistaLambda?: number;
    fistaThreshold?: number;
    enableDpp?: boolean;
    dppQualityWeight?: number;
    enableRefractoryFatigue?: boolean;
    enableInverseInhibition?: boolean;
    lateralInhibitionThreshold?: number;
    forceBruteForce?: boolean;
    enableTextHybridSearch?: boolean;
    textBoost?: number;
    bm25K1?: number;
    bm25B?: number;
    payloadFilter?: Json;
    diffusionBias?: number[];
}

export interface Edge { targetId: NodeId; label: string; weight: number; metadata: Json }
export interface Node { id: NodeId; vector: number[]; payload: Json; edges: Edge[] }
export interface SubgraphEdge extends Edge { sourceId: NodeId }
export interface Subgraph { nodes: Array<{ id: NodeId; payload: Json }>; edges: SubgraphEdge[] }
export interface Hit { id: NodeId; score: number; payload: Json }
export interface SearchContext {
    timingsMs: Record<string, number>;
    stageCounts: Record<string, number>;
    observations: Record<string, number>;
}
export interface AdvancedSearchResult { hits: Hit[]; context: SearchContext }
export interface Stats {
    namespace: string;
    dim: number;
    nodeCount: number;
    estimatedMemoryBytes: number;
    graph: { nodeCount: number; edgeCount: number };
}

export type QueryValue =
    | { type: 'node'; value: Node }
    | { type: 'edge'; value: SubgraphEdge }
    | { type: 'integer'; value: number }
    | { type: 'float'; value: number }
    | { type: 'string'; value: string }
    | { type: 'bool'; value: boolean }
    | { type: 'path'; value: NodeId[] }
    | { type: 'list'; value: Json[] }
    | { type: 'null' };
export type QueryResult =
    | { type: 'query'; rows: Array<Record<string, QueryValue>> }
    | { type: 'mutation'; affected: number; createdIds: NodeId[] };

export interface DatabaseHandle {
    readonly namespace: string;
    readonly dim: number;
    readonly options: Readonly<Required<OpenOptions>>;
    insert(vector: number[], payload?: Json): Promise<NodeId>;
    batchInsert(vectors: number[][], payloads: Json[]): Promise<NodeId[]>;
    upsert(id: NodeId, vector: number[], payload?: Json): Promise<void>;
    get(id: NodeId): Promise<Node | null>;
    updatePayload(id: NodeId, payload: Json): Promise<void>;
    patchPayload(id: NodeId, patch: Json): Promise<void>;
    updateVector(id: NodeId, vector: number[]): Promise<void>;
    delete(id: NodeId): Promise<void>;
    link(src: NodeId, dst: NodeId, label?: string, weight?: number): Promise<void>;
    unlink(src: NodeId, dst: NodeId): Promise<void>;
    shortestPath(source: NodeId, target: NodeId, options?: { maxDepth?: number; label?: string }): Promise<NodeId[] | null>;
    subgraph(id: NodeId, options?: { maxDepth?: number; labels?: string[]; direction?: Direction }): Promise<Subgraph>;
    indexText(id: NodeId, text: string): Promise<void>;
    indexKeyword(id: NodeId, keyword: string): Promise<void>;
    buildTextIndex(): Promise<void>;
    search(vector?: number[] | null, options?: SearchOptions & { queryText?: string; filter?: Json }): Promise<Hit[]>;
    searchBatch(vectors: number[][], options?: SearchOptions & { parallelism?: number }): Promise<Hit[][]>;
    searchAdvanced(vector?: number[] | null, config?: SearchOptions & { queryText?: string }): Promise<AdvancedSearchResult>;
    query(query: string, params?: Record<string, Json>): Promise<QueryResult>;
    buildQuiverIndex(): Promise<void>;
    compact(): Promise<void>;
    flush(): Promise<void>;
    close(): Promise<void>;
    stats(): Promise<Stats>;
}

export interface DatabaseApi {
    open(namespace: string, options?: OpenOptions): Promise<DatabaseHandle>;
    /** Only currently open namespaces. */
    listNamespaces(): Promise<string[]>;
}
