// TriviumDB 0.8.5 智能数据库高级特性与高性能指令集
// 完整释放自研 QuIVer ANN 索引、Rayon 并行批量检索、TQL 混合查询语言、AC 自动机倒排与知识图谱子图算力。

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::State;

use crate::infrastructure::trivium_store::{TriviumOpenOptions, TriviumStoreManager};
use crate::presentation::commands::helpers::log_command;
use crate::presentation::errors::CommandError;
use triviumdb::database::BatchSearchConfig;
use triviumdb::graph::reachability::{ReachabilityConfig, ReachabilityDirection};
use triviumdb::query::tql_executor::TqlValue;
use triviumdb::{EdgeDirection, Filter, SearchConfig};

// ════════════════════════════════════════════════
// DTO 定义
// ════════════════════════════════════════════════

/// 向量检索命中项
#[derive(Debug, Serialize, Deserialize)]
pub struct TriviumSearchHit {
    pub id: u64,
    pub score: f32,
    pub payload: Value,
}

/// 边数据结构
#[derive(Debug, Serialize, Deserialize)]
pub struct TriviumEdgeView {
    pub target_id: u64,
    pub label: String,
    pub weight: f32,
    pub metadata: Value,
}

/// 节点完整视图
#[derive(Debug, Serialize, Deserialize)]
pub struct TriviumNodeView {
    pub id: u64,
    pub vector: Vec<f32>,
    pub payload: Value,
    pub edges: Vec<TriviumEdgeView>,
}

/// 检索观测统计上下文（0.8.5 SOTA 级运行时性能观测）
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct TriviumSearchContextView {
    pub timings_ms: std::collections::HashMap<String, f64>,
    pub stage_counts: std::collections::HashMap<String, usize>,
    pub observations: std::collections::HashMap<String, u64>,
}

/// 高级认知检索结果包装（包含命中项与观测上下文）
#[derive(Debug, Serialize, Deserialize)]
pub struct TriviumAdvancedSearchResult {
    pub hits: Vec<TriviumSearchHit>,
    pub context: TriviumSearchContextView,
}

/// 子图节点视图
#[derive(Debug, Serialize, Deserialize)]
pub struct TriviumSubgraphNodeView {
    pub id: u64,
    pub payload: Value,
}

/// 子图边视图
#[derive(Debug, Serialize, Deserialize)]
pub struct TriviumSubgraphEdgeView {
    pub source_id: u64,
    pub target_id: u64,
    pub label: String,
    pub weight: f32,
    pub metadata: Value,
}

/// 子图拓扑计算结果（便于前端网络图与记忆脑图可视化）
#[derive(Debug, Serialize, Deserialize)]
pub struct TriviumSubgraphResultView {
    pub nodes: Vec<TriviumSubgraphNodeView>,
    pub edges: Vec<TriviumSubgraphEdgeView>,
}

/// 高级检索配置矩阵（对应 TriviumDB 0.8.5 SearchConfig）
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TriviumSearchConfig {
    pub top_k: Option<usize>,
    pub recall_k: Option<usize>,
    pub rerank_k: Option<usize>,
    pub expand_depth: Option<usize>,
    pub expand_labels: Option<Vec<String>>,
    pub max_edges_per_node: Option<usize>,
    pub min_edge_weight: Option<f32>,
    pub edge_direction: Option<String>,
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

// ════════════════════════════════════════════════
// 内部辅助转换
// ════════════════════════════════════════════════

fn map_trivium_error(context: &str, error: impl std::fmt::Display) -> CommandError {
    let msg = format!("{}: {}", context, error);
    tracing::error!("{}", msg);
    CommandError::InternalServerError(msg)
}

fn build_search_config(config: &TriviumSearchConfig) -> Result<SearchConfig, String> {
    let mut sc = SearchConfig::default();
    if let Some(v) = config.top_k {
        sc.top_k = v;
    }
    if let Some(v) = config.recall_k {
        sc.recall_k = v;
    }
    if let Some(v) = config.rerank_k {
        sc.rerank_k = v;
    }
    if let Some(v) = config.expand_depth {
        sc.expand_depth = v;
    }
    if let Some(ref v) = config.expand_labels {
        sc.expand_labels = Some(v.clone());
    }
    if let Some(v) = config.max_edges_per_node {
        sc.max_edges_per_node = v;
    }
    if let Some(v) = config.min_edge_weight {
        sc.min_edge_weight = v;
    }
    if let Some(ref dir) = config.edge_direction {
        sc.edge_direction = match dir.to_lowercase().as_str() {
            "incoming" => EdgeDirection::Incoming,
            "both" => EdgeDirection::Both,
            _ => EdgeDirection::Outgoing,
        };
    }
    if let Some(v) = config.min_score {
        sc.min_score = v;
    }
    if let Some(v) = config.teleport_alpha {
        sc.teleport_alpha = v;
    }
    if let Some(v) = config.enable_advanced_pipeline {
        sc.enable_advanced_pipeline = v;
    }
    if let Some(v) = config.enable_sparse_residual {
        sc.enable_sparse_residual = v;
    }
    if let Some(v) = config.fista_lambda {
        sc.fista_lambda = v;
    }
    if let Some(v) = config.fista_threshold {
        sc.fista_threshold = v;
    }
    if let Some(v) = config.enable_dpp {
        sc.enable_dpp = v;
    }
    if let Some(v) = config.dpp_quality_weight {
        sc.dpp_quality_weight = v;
    }
    if let Some(v) = config.enable_refractory_fatigue {
        sc.enable_refractory_fatigue = v;
    }
    if let Some(v) = config.enable_inverse_inhibition {
        sc.enable_inverse_inhibition = v;
    }
    if let Some(v) = config.lateral_inhibition_threshold {
        sc.lateral_inhibition_threshold = v;
    }
    if let Some(v) = config.force_brute_force {
        sc.force_brute_force = v;
    }
    if let Some(v) = config.enable_text_hybrid_search {
        sc.enable_text_hybrid_search = v;
    }
    if let Some(v) = config.text_boost {
        sc.text_boost = v;
    }
    if let Some(v) = config.bm25_k1 {
        sc.bm25_k1 = v;
    }
    if let Some(v) = config.bm25_b {
        sc.bm25_b = v;
    }
    if let Some(ref filter_val) = config.payload_filter {
        let filter = Filter::from_json(filter_val)
            .map_err(|e| format!("解析 Payload 过滤条件失败: {}", e))?;
        sc.payload_filter = Some(filter);
    }
    if let Some(ref bias) = config.diffusion_bias {
        sc.diffusion_bias = Some(bias.clone());
    }
    Ok(sc)
}

fn tql_value_to_json(val: TqlValue<f32>) -> Value {
    match val {
        TqlValue::Node(node) => serde_json::json!({
            "id": node.id,
            "vector": node.vector,
            "payload": node.payload,
            "numEdges": node.edges.len(),
        }),
        TqlValue::Int(i) => serde_json::json!(i),
        TqlValue::Float(f) => serde_json::json!(f),
        TqlValue::String(s) => serde_json::json!(s),
        TqlValue::Bool(b) => serde_json::json!(b),
        TqlValue::Path(p) => serde_json::json!(p),
        TqlValue::List(l) => serde_json::json!(l),
        TqlValue::Null => Value::Null,
    }
}

// ════════════════════════════════════════════════
// 指令：数据库生命周期与高级引擎配置
// ════════════════════════════════════════════════

/// 打开或预热指定命名空间的 TriviumDB（支持 Mmap/Rom 存储模式、SyncMode 等）
#[tauri::command]
pub async fn trivium_open(
    namespace: String,
    options: Option<TriviumOpenOptions>,
    store: State<'_, Arc<TriviumStoreManager>>,
) -> Result<Value, CommandError> {
    log_command(format!("trivium_open 命名空间={}", namespace));
    let opts = options.unwrap_or_default();

    let db_arc = store
        .open_or_get_with_options(&namespace, opts)
        .map_err(|e| map_trivium_error("打开数据库失败", e))?;

    let db = db_arc
        .lock()
        .map_err(|e| map_trivium_error("获取数据库锁失败", e))?;

    Ok(serde_json::json!({
        "namespace": namespace,
        "dim": db.dim(),
        "nodeCount": db.node_count(),
        "estimatedMemoryBytes": db.estimated_memory(),
    }))
}

/// 持久化指定命名空间的所有脏数据到磁盘
#[tauri::command]
pub async fn trivium_flush(
    namespace: String,
    store: State<'_, Arc<TriviumStoreManager>>,
) -> Result<(), CommandError> {
    log_command(format!("trivium_flush 命名空间={}", namespace));
    store
        .flush(&namespace)
        .map_err(|e| map_trivium_error("数据持久化落盘失败", e))
}

/// 执行数据库碎片压缩（合并重写 WAL、优化向量布局并回收墓碑内存）
#[tauri::command]
pub async fn trivium_compact(
    namespace: String,
    dim: Option<usize>,
    store: State<'_, Arc<TriviumStoreManager>>,
) -> Result<(), CommandError> {
    log_command(format!("trivium_compact 命名空间={}", namespace));
    let dim = dim.unwrap_or(1536);

    let db_arc = store
        .open_or_get(&namespace, dim)
        .map_err(|e| map_trivium_error("打开数据库失败", e))?;

    let mut db = db_arc
        .lock()
        .map_err(|e| map_trivium_error("获取数据库锁失败", e))?;

    db.compact()
        .map_err(|e| map_trivium_error("执行数据库压缩整理失败", e))
}

/// 主动触发构建或更新 QuIVer SOTA 向量图索引
#[tauri::command]
pub async fn trivium_build_quiver_index(
    namespace: String,
    dim: Option<usize>,
    store: State<'_, Arc<TriviumStoreManager>>,
) -> Result<(), CommandError> {
    log_command(format!("trivium_build_quiver_index 命名空间={}", namespace));
    let dim = dim.unwrap_or(1536);

    let db_arc = store
        .open_or_get(&namespace, dim)
        .map_err(|e| map_trivium_error("打开数据库失败", e))?;

    let db = db_arc
        .lock()
        .map_err(|e| map_trivium_error("获取数据库锁失败", e))?;

    db.build_quiver_index(None)
        .map_err(|e| map_trivium_error("构建 QuIVer 图索引失败", e))
}

/// 关闭指定命名空间的数据库
#[tauri::command]
pub async fn trivium_close(
    namespace: String,
    store: State<'_, Arc<TriviumStoreManager>>,
) -> Result<(), CommandError> {
    log_command(format!("trivium_close 命名空间={}", namespace));
    store
        .close(&namespace)
        .map_err(|e| map_trivium_error("关闭数据库失败", e))
}

/// 列出所有当前打开的命名空间
#[tauri::command]
pub async fn trivium_list_namespaces(
    store: State<'_, Arc<TriviumStoreManager>>,
) -> Result<Vec<String>, CommandError> {
    log_command("trivium_list_namespaces");
    store
        .list_namespaces()
        .map_err(|e| map_trivium_error("获取命名空间列表失败", e))
}

/// 获取指定命名空间的核心统计与健康状态
#[tauri::command]
pub async fn trivium_stats(
    namespace: String,
    dim: Option<usize>,
    store: State<'_, Arc<TriviumStoreManager>>,
) -> Result<Value, CommandError> {
    log_command(format!("trivium_stats 命名空间={}", namespace));
    let dim = dim.unwrap_or(1536);

    let db_arc = store
        .open_or_get(&namespace, dim)
        .map_err(|e| map_trivium_error("打开数据库失败", e))?;

    let db = db_arc
        .lock()
        .map_err(|e| map_trivium_error("获取数据库锁失败", e))?;

    let graph_stats = db.graph_stats();

    Ok(serde_json::json!({
        "namespace": namespace,
        "dim": db.dim(),
        "nodeCount": db.node_count(),
        "estimatedMemoryBytes": db.estimated_memory(),
        "graph": {
            "nodeCount": graph_stats.node_count,
            "edgeCount": graph_stats.edge_count,
        }
    }))
}

// ════════════════════════════════════════════════
// 指令：节点 CRUD 与 Upsert
// ════════════════════════════════════════════════

/// 插入单个节点
#[tauri::command]
pub async fn trivium_insert(
    namespace: String,
    dim: usize,
    vector: Vec<f32>,
    payload: Option<Value>,
    store: State<'_, Arc<TriviumStoreManager>>,
) -> Result<u64, CommandError> {
    let db_arc = store
        .open_or_get(&namespace, dim)
        .map_err(|e| map_trivium_error("打开数据库失败", e))?;

    let mut db = db_arc
        .lock()
        .map_err(|e| map_trivium_error("获取数据库锁失败", e))?;

    let payload = payload.unwrap_or(Value::Null);
    let id = db
        .insert(&vector, payload)
        .map_err(|e| map_trivium_error("插入节点失败", e))?;

    Ok(id)
}

/// 批量插入节点
#[tauri::command]
pub async fn trivium_batch_insert(
    namespace: String,
    dim: usize,
    vectors: Vec<Vec<f32>>,
    payloads: Vec<Value>,
    store: State<'_, Arc<TriviumStoreManager>>,
) -> Result<Vec<u64>, CommandError> {
    if vectors.len() != payloads.len() {
        return Err(CommandError::BadRequest(
            "向量列表长度与元数据列表长度必须一致".to_string(),
        ));
    }

    let db_arc = store
        .open_or_get(&namespace, dim)
        .map_err(|e| map_trivium_error("打开数据库失败", e))?;

    let mut db = db_arc
        .lock()
        .map_err(|e| map_trivium_error("获取数据库锁失败", e))?;

    let mut ids = Vec::with_capacity(vectors.len());
    for (v, p) in vectors.into_iter().zip(payloads) {
        let id = db
            .insert(&v, p)
            .map_err(|e| map_trivium_error("批量插入节点失败", e))?;
        ids.push(id);
    }

    Ok(ids)
}

/// 使用指定 ID 原子插入或更新节点（Upsert）
#[tauri::command]
pub async fn trivium_upsert_with_id(
    namespace: String,
    dim: usize,
    id: u64,
    vector: Vec<f32>,
    payload: Option<Value>,
    store: State<'_, Arc<TriviumStoreManager>>,
) -> Result<(), CommandError> {
    let db_arc = store
        .open_or_get(&namespace, dim)
        .map_err(|e| map_trivium_error("打开数据库失败", e))?;

    let mut db = db_arc
        .lock()
        .map_err(|e| map_trivium_error("获取数据库锁失败", e))?;

    let payload = payload.unwrap_or(Value::Null);
    db.upsert_with_id(id, &vector, payload)
        .map_err(|e| map_trivium_error("Upsert 节点失败", e))
}

/// 读取指定节点信息
#[tauri::command]
pub async fn trivium_get(
    namespace: String,
    dim: usize,
    id: u64,
    store: State<'_, Arc<TriviumStoreManager>>,
) -> Result<Option<TriviumNodeView>, CommandError> {
    let db_arc = store
        .open_or_get(&namespace, dim)
        .map_err(|e| map_trivium_error("打开数据库失败", e))?;

    let db = db_arc
        .lock()
        .map_err(|e| map_trivium_error("获取数据库锁失败", e))?;

    let view = db.get(id).map(|node| TriviumNodeView {
        id: node.id,
        vector: node.vector,
        payload: node.payload,
        edges: node
            .edges
            .into_iter()
            .map(|e| TriviumEdgeView {
                target_id: e.target_id,
                label: e.label,
                weight: e.weight,
                metadata: e.metadata,
            })
            .collect(),
    });

    Ok(view)
}

/// 全量更新节点的元数据 Payload
#[tauri::command]
pub async fn trivium_update_payload(
    namespace: String,
    dim: usize,
    id: u64,
    payload: Value,
    store: State<'_, Arc<TriviumStoreManager>>,
) -> Result<(), CommandError> {
    let db_arc = store
        .open_or_get(&namespace, dim)
        .map_err(|e| map_trivium_error("打开数据库失败", e))?;

    let mut db = db_arc
        .lock()
        .map_err(|e| map_trivium_error("获取数据库锁失败", e))?;

    db.update_payload(id, payload)
        .map_err(|e| map_trivium_error("更新节点元数据失败", e))
}

/// 局部 Patch 节点的元数据 Payload（支持 $set, $inc 等）
#[tauri::command]
pub async fn trivium_patch_payload(
    namespace: String,
    dim: usize,
    id: u64,
    patch: Value,
    store: State<'_, Arc<TriviumStoreManager>>,
) -> Result<(), CommandError> {
    let db_arc = store
        .open_or_get(&namespace, dim)
        .map_err(|e| map_trivium_error("打开数据库失败", e))?;

    let mut db = db_arc
        .lock()
        .map_err(|e| map_trivium_error("获取数据库锁失败", e))?;

    db.patch_payload(id, patch)
        .map_err(|e| map_trivium_error("局部更新节点元数据失败", e))
}

/// 更新节点的向量
#[tauri::command]
pub async fn trivium_update_vector(
    namespace: String,
    dim: usize,
    id: u64,
    vector: Vec<f32>,
    store: State<'_, Arc<TriviumStoreManager>>,
) -> Result<(), CommandError> {
    let db_arc = store
        .open_or_get(&namespace, dim)
        .map_err(|e| map_trivium_error("打开数据库失败", e))?;

    let db = db_arc
        .lock()
        .map_err(|e| map_trivium_error("获取数据库锁失败", e))?;

    db.update_vector(id, &vector)
        .map_err(|e| map_trivium_error("更新节点向量失败", e))
}

/// 删除节点（同时清理所有关联边与向量索引）
#[tauri::command]
pub async fn trivium_delete(
    namespace: String,
    dim: usize,
    id: u64,
    store: State<'_, Arc<TriviumStoreManager>>,
) -> Result<(), CommandError> {
    let db_arc = store
        .open_or_get(&namespace, dim)
        .map_err(|e| map_trivium_error("打开数据库失败", e))?;

    let mut db = db_arc
        .lock()
        .map_err(|e| map_trivium_error("获取数据库锁失败", e))?;

    db.delete(id)
        .map_err(|e| map_trivium_error("删除节点失败", e))
}

// ════════════════════════════════════════════════
// 指令：图谱关系拓扑、子图提取与寻路
// ════════════════════════════════════════════════

/// 在两个节点之间建立有向带权边
#[tauri::command]
pub async fn trivium_link(
    namespace: String,
    dim: usize,
    src: u64,
    dst: u64,
    label: Option<String>,
    weight: Option<f32>,
    store: State<'_, Arc<TriviumStoreManager>>,
) -> Result<(), CommandError> {
    let db_arc = store
        .open_or_get(&namespace, dim)
        .map_err(|e| map_trivium_error("打开数据库失败", e))?;

    let mut db = db_arc
        .lock()
        .map_err(|e| map_trivium_error("获取数据库锁失败", e))?;

    let label = label.unwrap_or_else(|| "related".to_string());
    let weight = weight.unwrap_or(1.0);

    db.link(src, dst, &label, weight)
        .map_err(|e| map_trivium_error("建立图谱边关系失败", e))
}

/// 移除两个节点之间的图谱边
#[tauri::command]
pub async fn trivium_unlink(
    namespace: String,
    dim: usize,
    src: u64,
    dst: u64,
    store: State<'_, Arc<TriviumStoreManager>>,
) -> Result<(), CommandError> {
    let db_arc = store
        .open_or_get(&namespace, dim)
        .map_err(|e| map_trivium_error("打开数据库失败", e))?;

    let mut db = db_arc
        .lock()
        .map_err(|e| map_trivium_error("获取数据库锁失败", e))?;

    db.unlink(src, dst)
        .map_err(|e| map_trivium_error("断开图谱边关系失败", e))
}

/// 计算两点间的最短路径
#[tauri::command]
pub async fn trivium_shortest_path(
    namespace: String,
    dim: usize,
    source: u64,
    target: u64,
    max_depth: Option<usize>,
    label: Option<String>,
    store: State<'_, Arc<TriviumStoreManager>>,
) -> Result<Option<Vec<u64>>, CommandError> {
    let db_arc = store
        .open_or_get(&namespace, dim)
        .map_err(|e| map_trivium_error("打开数据库失败", e))?;

    let db = db_arc
        .lock()
        .map_err(|e| map_trivium_error("获取数据库锁失败", e))?;

    let max_depth = max_depth.unwrap_or(6);
    let path = db.shortest_path(source, target, max_depth, label.as_deref());
    Ok(path)
}

/// 提取以指定节点为中心的关联子图（便于图谱可视化）
#[tauri::command]
pub async fn trivium_subgraph(
    namespace: String,
    dim: usize,
    id: u64,
    max_depth: Option<usize>,
    labels: Option<Vec<String>>,
    direction: Option<String>,
    store: State<'_, Arc<TriviumStoreManager>>,
) -> Result<TriviumSubgraphResultView, CommandError> {
    let db_arc = store
        .open_or_get(&namespace, dim)
        .map_err(|e| map_trivium_error("打开数据库失败", e))?;

    let db = db_arc
        .lock()
        .map_err(|e| map_trivium_error("获取数据库锁失败", e))?;

    let mut cfg = ReachabilityConfig::default();
    if let Some(md) = max_depth {
        cfg.max_depth = md;
    }
    cfg.labels = labels;
    if let Some(ref d) = direction {
        cfg.direction = match d.to_lowercase().as_str() {
            "incoming" => ReachabilityDirection::Incoming,
            "both" => ReachabilityDirection::Both,
            _ => ReachabilityDirection::Outgoing,
        };
    }

    let sub = db
        .query_subgraph(id, &cfg)
        .map_err(|e| map_trivium_error("查询关联子图拓扑失败", e))?;

    Ok(TriviumSubgraphResultView {
        nodes: sub
            .nodes
            .into_iter()
            .map(|n| TriviumSubgraphNodeView {
                id: n.id,
                payload: n.payload,
            })
            .collect(),
        edges: sub
            .edges
            .into_iter()
            .map(|e| TriviumSubgraphEdgeView {
                source_id: e.source_id,
                target_id: e.target_id,
                label: e.label,
                weight: e.weight,
                metadata: e.metadata,
            })
            .collect(),
    })
}

// ════════════════════════════════════════════════
// 指令：文本倒排检索与 AC 自动机关键词索引
// ════════════════════════════════════════════════

/// 为节点添加 BM25 全文检索文本
#[tauri::command]
pub async fn trivium_index_text(
    namespace: String,
    dim: usize,
    id: u64,
    text: String,
    store: State<'_, Arc<TriviumStoreManager>>,
) -> Result<(), CommandError> {
    let db_arc = store
        .open_or_get(&namespace, dim)
        .map_err(|e| map_trivium_error("打开数据库失败", e))?;

    let mut db = db_arc
        .lock()
        .map_err(|e| map_trivium_error("获取数据库锁失败", e))?;

    db.index_text(id, &text)
        .map_err(|e| map_trivium_error("建立文本索引失败", e))
}

/// 为节点建立 AC 自动机专有名词/实体精准关键词倒排锚点
#[tauri::command]
pub async fn trivium_index_keyword(
    namespace: String,
    dim: usize,
    id: u64,
    keyword: String,
    store: State<'_, Arc<TriviumStoreManager>>,
) -> Result<(), CommandError> {
    let db_arc = store
        .open_or_get(&namespace, dim)
        .map_err(|e| map_trivium_error("打开数据库失败", e))?;

    let mut db = db_arc
        .lock()
        .map_err(|e| map_trivium_error("获取数据库锁失败", e))?;

    db.index_keyword(id, &keyword)
        .map_err(|e| map_trivium_error("建立 AC 自动机关键词索引失败", e))
}

/// 编译生成全文倒排索引
#[tauri::command]
pub async fn trivium_build_text_index(
    namespace: String,
    dim: usize,
    store: State<'_, Arc<TriviumStoreManager>>,
) -> Result<(), CommandError> {
    let db_arc = store
        .open_or_get(&namespace, dim)
        .map_err(|e| map_trivium_error("打开数据库失败", e))?;

    let mut db = db_arc
        .lock()
        .map_err(|e| map_trivium_error("获取数据库锁失败", e))?;

    db.build_text_index()
        .map_err(|e| map_trivium_error("构建文本索引失败", e))
}

// ════════════════════════════════════════════════
// 指令：向量检索、Rayon 并行批量检索与高级认知混合管线
// ════════════════════════════════════════════════

/// 基础检索（支持向量召回、图扩散、Payload 预过滤与偏置）
#[tauri::command]
#[expect(
    clippy::too_many_arguments,
    reason = "Tauri command parameters intentionally preserve the flat invoke ABI"
)]
pub async fn trivium_search(
    namespace: String,
    dim: usize,
    vector: Option<Vec<f32>>,
    query_text: Option<String>,
    top_k: Option<usize>,
    expand_depth: Option<usize>,
    min_score: Option<f32>,
    filter: Option<Value>,
    diffusion_bias: Option<Vec<f32>>,
    store: State<'_, Arc<TriviumStoreManager>>,
) -> Result<Vec<TriviumSearchHit>, CommandError> {
    let db_arc = store
        .open_or_get(&namespace, dim)
        .map_err(|e| map_trivium_error("打开数据库失败", e))?;

    let db = db_arc
        .lock()
        .map_err(|e| map_trivium_error("获取数据库锁失败", e))?;

    let mut config = SearchConfig {
        top_k: top_k.unwrap_or(5),
        expand_depth: expand_depth.unwrap_or(2),
        min_score: min_score.unwrap_or(0.1),
        enable_advanced_pipeline: false,
        ..Default::default()
    };

    if let Some(ref filter_val) = filter {
        let parsed = Filter::from_json(filter_val)
            .map_err(|e| CommandError::BadRequest(format!("解析过滤条件失败: {}", e)))?;
        config.payload_filter = Some(parsed);
    }
    if let Some(bias) = diffusion_bias {
        config.diffusion_bias = Some(bias);
    }

    let hits = db
        .search_hybrid(query_text.as_deref(), vector.as_deref(), &config)
        .map_err(|e| map_trivium_error("混合检索执行失败", e))?;

    Ok(hits
        .into_iter()
        .map(|h| TriviumSearchHit {
            id: h.id,
            score: h.score,
            payload: h.payload,
        })
        .collect())
}

/// Rayon 多线程并行批量向量检索（极高并发加速，支持同时检索多个 Query）
#[tauri::command]
#[expect(
    clippy::too_many_arguments,
    reason = "Tauri command parameters intentionally preserve the flat invoke ABI"
)]
pub async fn trivium_search_batch(
    namespace: String,
    dim: usize,
    vectors: Vec<Vec<f32>>,
    top_k: Option<usize>,
    expand_depth: Option<usize>,
    min_score: Option<f32>,
    parallelism: Option<usize>,
    store: State<'_, Arc<TriviumStoreManager>>,
) -> Result<Vec<Vec<TriviumSearchHit>>, CommandError> {
    let db_arc = store
        .open_or_get(&namespace, dim)
        .map_err(|e| map_trivium_error("打开数据库失败", e))?;

    let db = db_arc
        .lock()
        .map_err(|e| map_trivium_error("获取数据库锁失败", e))?;

    let search_config = SearchConfig {
        top_k: top_k.unwrap_or(5),
        expand_depth: expand_depth.unwrap_or(2),
        min_score: min_score.unwrap_or(0.1),
        enable_advanced_pipeline: false,
        ..Default::default()
    };
    let batch_config = BatchSearchConfig {
        parallelism: parallelism.unwrap_or(0),
    };

    let batch_results = db
        .search_batch(&vectors, &search_config, &batch_config)
        .map_err(|e| map_trivium_error("批量向量并行检索失败", e))?;

    Ok(batch_results
        .into_iter()
        .map(|hits| {
            hits.into_iter()
                .map(|h| TriviumSearchHit {
                    id: h.id,
                    score: h.score,
                    payload: h.payload,
                })
                .collect()
        })
        .collect())
}

/// 完整高级认知管线检索（FISTA 寻隐 + SA-PPR 个性化扩散 + DPP 多样性采样 + 阶段耗时与 QuIVer 观测上下文）
#[tauri::command]
pub async fn trivium_search_advanced(
    namespace: String,
    dim: usize,
    vector: Option<Vec<f32>>,
    query_text: Option<String>,
    config: TriviumSearchConfig,
    store: State<'_, Arc<TriviumStoreManager>>,
) -> Result<TriviumAdvancedSearchResult, CommandError> {
    let db_arc = store
        .open_or_get(&namespace, dim)
        .map_err(|e| map_trivium_error("打开数据库失败", e))?;

    let db = db_arc
        .lock()
        .map_err(|e| map_trivium_error("获取数据库锁失败", e))?;

    let search_config = build_search_config(&config)
        .map_err(|e| CommandError::BadRequest(format!("检索配置错误: {}", e)))?;

    let (hits, ctx) = db
        .search_hybrid_with_context(
            query_text.as_deref(),
            vector.as_deref(),
            &search_config,
        )
        .map_err(|e| map_trivium_error("高级认知检索执行失败", e))?;

    let mut timings_ms = std::collections::HashMap::new();
    for (stage, duration) in ctx.stage_timings {
        timings_ms.insert(stage, duration.as_secs_f64() * 1000.0);
    }
    let mut stage_counts = std::collections::HashMap::new();
    for (stage, count) in ctx.stage_counts {
        stage_counts.insert(stage, count);
    }
    let mut observations = std::collections::HashMap::new();
    for (name, val) in ctx.observations {
        observations.insert(name, val);
    }

    Ok(TriviumAdvancedSearchResult {
        hits: hits
            .into_iter()
            .map(|h| TriviumSearchHit {
                id: h.id,
                score: h.score,
                payload: h.payload,
            })
            .collect(),
        context: TriviumSearchContextView {
            timings_ms,
            stage_counts,
            observations,
        },
    })
}

// ════════════════════════════════════════════════
// 指令：TQL 统一查询语言执行引擎
// ════════════════════════════════════════════════

/// 执行原生 TQL（Trivium Query Language）语句
#[tauri::command]
pub async fn trivium_query_tql(
    namespace: String,
    dim: Option<usize>,
    query: String,
    store: State<'_, Arc<TriviumStoreManager>>,
) -> Result<Value, CommandError> {
    log_command(format!("trivium_query_tql 命名空间={} 语句={}", namespace, query));
    let dim = dim.unwrap_or(1536);

    let db_arc = store
        .open_or_get(&namespace, dim)
        .map_err(|e| map_trivium_error("打开数据库失败", e))?;

    let trimmed = query.trim();
    let is_mutation = trimmed.starts_with("CREATE")
        || (trimmed.starts_with("MATCH")
            && (trimmed.contains("CREATE")
                || trimmed.contains("SET")
                || trimmed.contains("DELETE")));

    if is_mutation {
        let mut db = db_arc
            .lock()
            .map_err(|e| map_trivium_error("获取数据库锁失败", e))?;
        let res = db
            .tql_mut(trimmed)
            .map_err(|e| map_trivium_error("执行 TQL 变更操作失败", e))?;

        Ok(serde_json::json!({
            "type": "mutation",
            "affected": res.affected,
            "createdIds": res.created_ids,
        }))
    } else {
        let db = db_arc
            .lock()
            .map_err(|e| map_trivium_error("获取数据库锁失败", e))?;
        let res = db
            .tql(trimmed)
            .map_err(|e| map_trivium_error("执行 TQL 查询失败", e))?;

        let rows: Vec<serde_json::Map<String, Value>> = res
            .into_iter()
            .map(|row| {
                let mut map = serde_json::Map::new();
                for (k, v) in row {
                    map.insert(k, tql_value_to_json(v));
                }
                map
            })
            .collect();

        Ok(serde_json::json!({
            "type": "query",
            "rows": rows,
        }))
    }
}
