//! Host database protocol. Engine types stay in the adapter.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

// The JS API uses safe integer IDs; widen the wire format only for a real consumer.
pub type NodeId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StorageMode {
    Mmap,
    Rom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SyncMode {
    Normal,
    Full,
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Outgoing,
    Incoming,
    Both,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseOpenOptions {
    pub dim: Option<usize>,
    pub storage_mode: Option<StorageMode>,
    pub sync_mode: Option<SyncMode>,
    pub load_text_index: Option<bool>,
    pub auto_build_quiver: Option<bool>,
    pub memory_limit_mb: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseConfig {
    pub dim: usize,
    pub storage_mode: StorageMode,
    pub sync_mode: SyncMode,
    pub load_text_index: bool,
    pub auto_build_quiver: bool,
    pub memory_limit_mb: usize,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchOptions {
    pub top_k: Option<usize>,
    pub recall_k: Option<usize>,
    pub rerank_k: Option<usize>,
    pub expand_depth: Option<usize>,
    pub expand_labels: Option<Vec<String>>,
    pub max_edges_per_node: Option<usize>,
    pub min_edge_weight: Option<f32>,
    pub edge_direction: Option<Direction>,
    pub min_score: Option<f32>,
    pub teleport_alpha: Option<f32>,
    pub enable_advanced_pipeline: Option<bool>,
    pub enable_sparse_residual: Option<bool>,
    pub fista_lambda: Option<f32>,
    pub fista_threshold: Option<f32>,
    pub enable_dpp: Option<bool>,
    pub dpp_quality_weight: Option<f32>,
    pub enable_refractory_fatigue: Option<bool>,
    pub enable_inverse_inhibition: Option<bool>,
    pub lateral_inhibition_threshold: Option<usize>,
    pub force_brute_force: Option<bool>,
    pub enable_text_hybrid_search: Option<bool>,
    pub text_boost: Option<f32>,
    pub bm25_k1: Option<f32>,
    pub bm25_b: Option<f32>,
    pub payload_filter: Option<Value>,
    pub diffusion_bias: Option<Vec<f32>>,
}

#[derive(Debug, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum DatabaseRequest {
    Open {
        namespace: String,
        #[serde(default)]
        options: DatabaseOpenOptions,
    },
    Close {
        namespace: String,
    },
    ListNamespaces,
    Execute {
        namespace: String,
        operation: Box<DatabaseOperation>,
    },
    /// Internal lifecycle requests used by the host, not additional public methods.
    FlushAll,
    CloseAll,
}

#[derive(Debug, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum DatabaseOperation {
    Insert {
        vector: Vec<f32>,
        #[serde(default)]
        payload: Value,
    },
    BatchInsert {
        vectors: Vec<Vec<f32>>,
        payloads: Vec<Value>,
    },
    Upsert {
        id: NodeId,
        vector: Vec<f32>,
        #[serde(default)]
        payload: Value,
    },
    Get {
        id: NodeId,
    },
    UpdatePayload {
        id: NodeId,
        payload: Value,
    },
    PatchPayload {
        id: NodeId,
        patch: Value,
    },
    UpdateVector {
        id: NodeId,
        vector: Vec<f32>,
    },
    Delete {
        id: NodeId,
    },
    Link {
        src: NodeId,
        dst: NodeId,
        label: String,
        weight: f32,
    },
    Unlink {
        src: NodeId,
        dst: NodeId,
    },
    ShortestPath {
        source: NodeId,
        target: NodeId,
        max_depth: Option<usize>,
        label: Option<String>,
    },
    Subgraph {
        id: NodeId,
        max_depth: Option<usize>,
        labels: Option<Vec<String>>,
        direction: Option<Direction>,
    },
    IndexText {
        id: NodeId,
        text: String,
    },
    IndexKeyword {
        id: NodeId,
        keyword: String,
    },
    BuildTextIndex,
    Search {
        vector: Option<Vec<f32>>,
        query_text: Option<String>,
        #[serde(default)]
        config: Box<SearchOptions>,
    },
    SearchBatch {
        vectors: Vec<Vec<f32>>,
        #[serde(default)]
        config: Box<SearchOptions>,
        parallelism: Option<usize>,
    },
    SearchAdvanced {
        vector: Option<Vec<f32>>,
        query_text: Option<String>,
        #[serde(default)]
        config: Box<SearchOptions>,
    },
    Query {
        query: String,
        #[serde(default)]
        params: BTreeMap<String, Value>,
    },
    BuildQuiverIndex,
    Compact,
    Flush,
    Stats,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenDatabase {
    pub namespace: String,
    pub dim: usize,
    pub options: DatabaseConfig,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseStats {
    pub namespace: String,
    pub dim: usize,
    pub node_count: usize,
    pub estimated_memory_bytes: usize,
    pub graph: GraphStats,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphStats {
    pub node_count: usize,
    pub edge_count: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseNode {
    pub id: NodeId,
    pub vector: Vec<f32>,
    pub payload: Value,
    pub edges: Vec<DatabaseEdge>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseEdge {
    pub target_id: NodeId,
    pub label: String,
    pub weight: f32,
    pub metadata: Value,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubgraphNode {
    pub id: NodeId,
    pub payload: Value,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubgraphEdge {
    pub source_id: NodeId,
    pub target_id: NodeId,
    pub label: String,
    pub weight: f32,
    pub metadata: Value,
}

#[derive(Debug, Serialize)]
pub struct Subgraph {
    pub nodes: Vec<SubgraphNode>,
    pub edges: Vec<SubgraphEdge>,
}

#[derive(Debug, Serialize)]
pub struct SearchHit {
    pub id: NodeId,
    pub score: f32,
    pub payload: Value,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchContext {
    pub timings_ms: BTreeMap<String, f64>,
    pub stage_counts: BTreeMap<String, usize>,
    pub observations: BTreeMap<String, u64>,
}

#[derive(Debug, Serialize)]
pub struct AdvancedSearchResult {
    pub hits: Vec<SearchHit>,
    pub context: SearchContext,
}

/// Query cells mirror the native TQL result types.
#[derive(Debug, Serialize)]
#[serde(tag = "type", content = "value", rename_all = "camelCase")]
pub enum QueryValue {
    Node(DatabaseNode),
    Edge(SubgraphEdge),
    Integer(i64),
    Float(f64),
    String(String),
    Bool(bool),
    Path(Vec<NodeId>),
    List(Vec<Value>),
    Null,
}

#[derive(Debug, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum QueryResult {
    Query {
        rows: Vec<BTreeMap<String, QueryValue>>,
    },
    Mutation {
        affected: usize,
        created_ids: Vec<NodeId>,
    },
}

/// Each request has one documented result shape; no JSON object assembly in commands.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum DatabaseResponse {
    Unit,
    Open(OpenDatabase),
    Namespaces(Vec<String>),
    Id(NodeId),
    Ids(Vec<NodeId>),
    Node(Option<DatabaseNode>),
    Path(Option<Vec<NodeId>>),
    Subgraph(Subgraph),
    Hits(Vec<SearchHit>),
    BatchHits(Vec<Vec<SearchHit>>),
    Advanced(AdvancedSearchResult),
    Query(QueryResult),
    Stats(DatabaseStats),
}
