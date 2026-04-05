// TriviumDB 扩展指令集
// 为前端插件提供向量检索、图谱操作和数据持久化的 Tauri 命令接口。

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::State;

use crate::infrastructure::trivium_store::TriviumStoreManager;
use crate::presentation::commands::helpers::log_command;
use crate::presentation::errors::CommandError;

// ════════════════════════════════════════════════
// DTO 定义
// ════════════════════════════════════════════════

/// 向量检索命中结果
#[derive(Debug, Serialize, Deserialize)]
pub struct TriviumSearchHit {
    pub id: u64,
    pub score: f32,
    pub payload: Value,
}

/// 节点视图（用于查询结果返回）
#[derive(Debug, Serialize, Deserialize)]
pub struct TriviumNodeView {
    pub id: u64,
    pub vector: Vec<f32>,
    pub payload: Value,
    pub num_edges: usize,
}

/// 高级检索配置（对应 TriviumDB 的 SearchConfig）
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TriviumSearchConfig {
    pub top_k: Option<usize>,
    pub expand_depth: Option<usize>,
    pub min_score: Option<f32>,
    pub teleport_alpha: Option<f32>,
    pub enable_advanced_pipeline: Option<bool>,
    pub enable_sparse_residual: Option<bool>,
    pub fista_lambda: Option<f32>,
    pub fista_threshold: Option<f32>,
    pub enable_dpp: Option<bool>,
    pub dpp_quality_weight: Option<f32>,
    pub enable_text_hybrid_search: Option<bool>,
    pub text_boost: Option<f32>,
}

// ════════════════════════════════════════════════
// 辅助函数
// ════════════════════════════════════════════════

fn map_trivium_error(context: &str, error: impl std::fmt::Display) -> CommandError {
    let msg = format!("{}: {}", context, error);
    tracing::error!("{}", msg);
    CommandError::InternalServerError(msg)
}

fn build_search_config(config: &TriviumSearchConfig) -> triviumdb::database::SearchConfig {
    let mut sc = triviumdb::database::SearchConfig::default();
    if let Some(v) = config.top_k { sc.top_k = v; }
    if let Some(v) = config.expand_depth { sc.expand_depth = v; }
    if let Some(v) = config.min_score { sc.min_score = v; }
    if let Some(v) = config.teleport_alpha { sc.teleport_alpha = v; }
    if let Some(v) = config.enable_advanced_pipeline { sc.enable_advanced_pipeline = v; }
    if let Some(v) = config.enable_sparse_residual { sc.enable_sparse_residual = v; }
    if let Some(v) = config.fista_lambda { sc.fista_lambda = v; }
    if let Some(v) = config.fista_threshold { sc.fista_threshold = v; }
    if let Some(v) = config.enable_dpp { sc.enable_dpp = v; }
    if let Some(v) = config.dpp_quality_weight { sc.dpp_quality_weight = v; }
    if let Some(v) = config.enable_text_hybrid_search { sc.enable_text_hybrid_search = v; }
    if let Some(v) = config.text_boost { sc.text_boost = v; }
    sc
}

// ════════════════════════════════════════════════
// 指令：数据库生命周期
// ════════════════════════════════════════════════

/// 打开（或获取已打开的）TriviumDB 命名空间
#[tauri::command]
pub async fn trivium_open(
    namespace: String,
    dim: Option<usize>,
    store: State<'_, Arc<TriviumStoreManager>>,
) -> Result<Value, CommandError> {
    log_command(format!("trivium_open namespace={}", namespace));
    let dim = dim.unwrap_or(1536);

    let db = store
        .open_or_get(&namespace, dim)
        .map_err(|e| map_trivium_error("打开数据库失败", e))?;

    let db = db
        .lock()
        .map_err(|e| map_trivium_error("获取数据库锁失败", e))?;

    Ok(serde_json::json!({
        "namespace": namespace,
        "dim": db.dim(),
        "nodeCount": db.node_count(),
    }))
}

/// 持久化指定命名空间数据
#[tauri::command]
pub async fn trivium_flush(
    namespace: String,
    store: State<'_, Arc<TriviumStoreManager>>,
) -> Result<(), CommandError> {
    log_command(format!("trivium_flush namespace={}", namespace));
    store
        .flush(&namespace)
        .map_err(|e| map_trivium_error("持久化失败", e))
}

/// 关闭指定命名空间
#[tauri::command]
pub async fn trivium_close(
    namespace: String,
    store: State<'_, Arc<TriviumStoreManager>>,
) -> Result<(), CommandError> {
    log_command(format!("trivium_close namespace={}", namespace));
    store
        .close(&namespace)
        .map_err(|e| map_trivium_error("关闭数据库失败", e))
}

// ════════════════════════════════════════════════
// 指令：节点 CRUD
// ════════════════════════════════════════════════

/// 插入节点（自动分配 ID）
#[tauri::command]
pub async fn trivium_insert(
    namespace: String,
    dim: Option<usize>,
    vector: Vec<f32>,
    payload: Value,
    store: State<'_, Arc<TriviumStoreManager>>,
) -> Result<u64, CommandError> {
    log_command(format!("trivium_insert namespace={}", namespace));
    let dim = dim.unwrap_or(1536);

    let db = store
        .open_or_get(&namespace, dim)
        .map_err(|e| map_trivium_error("打开数据库失败", e))?;
    let mut db = db
        .lock()
        .map_err(|e| map_trivium_error("获取数据库锁失败", e))?;

    db.insert(&vector, payload)
        .map_err(|e| map_trivium_error("插入节点失败", e))
}

/// 批量插入节点
#[tauri::command]
pub async fn trivium_batch_insert(
    namespace: String,
    dim: Option<usize>,
    vectors: Vec<Vec<f32>>,
    payloads: Vec<Value>,
    store: State<'_, Arc<TriviumStoreManager>>,
) -> Result<Vec<u64>, CommandError> {
    log_command(format!(
        "trivium_batch_insert namespace={} count={}",
        namespace,
        vectors.len()
    ));
    let dim = dim.unwrap_or(1536);

    if vectors.len() != payloads.len() {
        return Err(CommandError::BadRequest(
            "向量数组和 payload 数组的长度必须一致".to_string(),
        ));
    }

    let db = store
        .open_or_get(&namespace, dim)
        .map_err(|e| map_trivium_error("打开数据库失败", e))?;
    let mut db = db
        .lock()
        .map_err(|e| map_trivium_error("获取数据库锁失败", e))?;

    let mut ids = Vec::with_capacity(vectors.len());
    for (vector, payload) in vectors.iter().zip(payloads.into_iter()) {
        let id = db
            .insert(vector, payload)
            .map_err(|e| map_trivium_error("批量插入节点失败", e))?;
        ids.push(id);
    }
    Ok(ids)
}

/// 获取节点信息
#[tauri::command]
pub async fn trivium_get(
    namespace: String,
    dim: Option<usize>,
    id: u64,
    store: State<'_, Arc<TriviumStoreManager>>,
) -> Result<Option<TriviumNodeView>, CommandError> {
    log_command(format!("trivium_get namespace={} id={}", namespace, id));
    let dim = dim.unwrap_or(1536);

    let db = store
        .open_or_get(&namespace, dim)
        .map_err(|e| map_trivium_error("打开数据库失败", e))?;
    let db = db
        .lock()
        .map_err(|e| map_trivium_error("获取数据库锁失败", e))?;

    let payload = db.get_payload(id);
    match payload {
        Some(p) => {
            let edges = db.get_edges(id);
            Ok(Some(TriviumNodeView {
                id,
                vector: vec![], // 不返回原始向量以节省带宽
                payload: p,
                num_edges: edges.len(),
            }))
        }
        None => Ok(None),
    }
}

/// 更新节点的 payload
#[tauri::command]
pub async fn trivium_update_payload(
    namespace: String,
    dim: Option<usize>,
    id: u64,
    payload: Value,
    store: State<'_, Arc<TriviumStoreManager>>,
) -> Result<(), CommandError> {
    log_command(format!(
        "trivium_update_payload namespace={} id={}",
        namespace, id
    ));
    let dim = dim.unwrap_or(1536);

    let db = store
        .open_or_get(&namespace, dim)
        .map_err(|e| map_trivium_error("打开数据库失败", e))?;
    let mut db = db
        .lock()
        .map_err(|e| map_trivium_error("获取数据库锁失败", e))?;

    db.update_payload(id, payload)
        .map_err(|e| map_trivium_error("更新 payload 失败", e))
}

/// 更新节点的向量
#[tauri::command]
pub async fn trivium_update_vector(
    namespace: String,
    dim: Option<usize>,
    id: u64,
    vector: Vec<f32>,
    store: State<'_, Arc<TriviumStoreManager>>,
) -> Result<(), CommandError> {
    log_command(format!(
        "trivium_update_vector namespace={} id={}",
        namespace, id
    ));
    let dim = dim.unwrap_or(1536);

    let db = store
        .open_or_get(&namespace, dim)
        .map_err(|e| map_trivium_error("打开数据库失败", e))?;
    let mut db = db
        .lock()
        .map_err(|e| map_trivium_error("获取数据库锁失败", e))?;

    db.update_vector(id, &vector)
        .map_err(|e| map_trivium_error("更新向量失败", e))
}

/// 删除节点
#[tauri::command]
pub async fn trivium_delete(
    namespace: String,
    dim: Option<usize>,
    id: u64,
    store: State<'_, Arc<TriviumStoreManager>>,
) -> Result<(), CommandError> {
    log_command(format!(
        "trivium_delete namespace={} id={}",
        namespace, id
    ));
    let dim = dim.unwrap_or(1536);

    let db = store
        .open_or_get(&namespace, dim)
        .map_err(|e| map_trivium_error("打开数据库失败", e))?;
    let mut db = db
        .lock()
        .map_err(|e| map_trivium_error("获取数据库锁失败", e))?;

    db.delete(id)
        .map_err(|e| map_trivium_error("删除节点失败", e))
}

// ════════════════════════════════════════════════
// 指令：图谱操作
// ════════════════════════════════════════════════

/// 在两个节点之间建立有向带权边
#[tauri::command]
pub async fn trivium_link(
    namespace: String,
    dim: Option<usize>,
    src: u64,
    dst: u64,
    label: Option<String>,
    weight: Option<f32>,
    store: State<'_, Arc<TriviumStoreManager>>,
) -> Result<(), CommandError> {
    log_command(format!(
        "trivium_link namespace={} {}->{}", namespace, src, dst
    ));
    let dim = dim.unwrap_or(1536);

    let db = store
        .open_or_get(&namespace, dim)
        .map_err(|e| map_trivium_error("打开数据库失败", e))?;
    let mut db = db
        .lock()
        .map_err(|e| map_trivium_error("获取数据库锁失败", e))?;

    let label = label.as_deref().unwrap_or("related");
    let weight = weight.unwrap_or(1.0);

    db.link(src, dst, label, weight)
        .map_err(|e| map_trivium_error("建立边失败", e))
}

/// 移除两个节点之间的边
#[tauri::command]
pub async fn trivium_unlink(
    namespace: String,
    dim: Option<usize>,
    src: u64,
    dst: u64,
    store: State<'_, Arc<TriviumStoreManager>>,
) -> Result<(), CommandError> {
    log_command(format!(
        "trivium_unlink namespace={} {}->{}", namespace, src, dst
    ));
    let dim = dim.unwrap_or(1536);

    let db = store
        .open_or_get(&namespace, dim)
        .map_err(|e| map_trivium_error("打开数据库失败", e))?;
    let mut db = db
        .lock()
        .map_err(|e| map_trivium_error("获取数据库锁失败", e))?;

    db.unlink(src, dst)
        .map_err(|e| map_trivium_error("移除边失败", e))
}

// ════════════════════════════════════════════════
// 指令：检索
// ════════════════════════════════════════════════

/// 基础向量检索（向量锚定 + 图谱扩散）
#[tauri::command]
pub async fn trivium_search(
    namespace: String,
    dim: Option<usize>,
    vector: Vec<f32>,
    top_k: Option<usize>,
    expand_depth: Option<usize>,
    min_score: Option<f32>,
    store: State<'_, Arc<TriviumStoreManager>>,
) -> Result<Vec<TriviumSearchHit>, CommandError> {
    log_command(format!("trivium_search namespace={}", namespace));
    let dim = dim.unwrap_or(1536);

    let db = store
        .open_or_get(&namespace, dim)
        .map_err(|e| map_trivium_error("打开数据库失败", e))?;
    let db = db
        .lock()
        .map_err(|e| map_trivium_error("获取数据库锁失败", e))?;

    let results = db
        .search(
            &vector,
            top_k.unwrap_or(5),
            expand_depth.unwrap_or(0),
            min_score.unwrap_or(0.1),
        )
        .map_err(|e| map_trivium_error("检索失败", e))?;

    Ok(results
        .into_iter()
        .map(|hit| TriviumSearchHit {
            id: hit.id,
            score: hit.score,
            payload: hit.payload,
        })
        .collect())
}

/// 认知管线高级检索
#[tauri::command]
pub async fn trivium_search_advanced(
    namespace: String,
    dim: Option<usize>,
    vector: Vec<f32>,
    config: TriviumSearchConfig,
    store: State<'_, Arc<TriviumStoreManager>>,
) -> Result<Vec<TriviumSearchHit>, CommandError> {
    log_command(format!(
        "trivium_search_advanced namespace={}",
        namespace
    ));
    let dim = dim.unwrap_or(1536);

    let db = store
        .open_or_get(&namespace, dim)
        .map_err(|e| map_trivium_error("打开数据库失败", e))?;
    let db = db
        .lock()
        .map_err(|e| map_trivium_error("获取数据库锁失败", e))?;

    let sc = build_search_config(&config);
    let results = db
        .search_advanced(&vector, &sc)
        .map_err(|e| map_trivium_error("高级检索失败", e))?;

    Ok(results
        .into_iter()
        .map(|hit| TriviumSearchHit {
            id: hit.id,
            score: hit.score,
            payload: hit.payload,
        })
        .collect())
}

/// 文本索引：为节点建立 BM25 文本索引
#[tauri::command]
pub async fn trivium_index_text(
    namespace: String,
    dim: Option<usize>,
    id: u64,
    text: String,
    store: State<'_, Arc<TriviumStoreManager>>,
) -> Result<(), CommandError> {
    log_command(format!(
        "trivium_index_text namespace={} id={}",
        namespace, id
    ));
    let dim = dim.unwrap_or(1536);

    let db = store
        .open_or_get(&namespace, dim)
        .map_err(|e| map_trivium_error("打开数据库失败", e))?;
    let mut db = db
        .lock()
        .map_err(|e| map_trivium_error("获取数据库锁失败", e))?;

    db.index_text(id, &text)
        .map_err(|e| map_trivium_error("建立文本索引失败", e))
}

/// 关键词索引：为节点建立 AC 自动机精确匹配索引
#[tauri::command]
pub async fn trivium_index_keyword(
    namespace: String,
    dim: Option<usize>,
    id: u64,
    keyword: String,
    store: State<'_, Arc<TriviumStoreManager>>,
) -> Result<(), CommandError> {
    log_command(format!(
        "trivium_index_keyword namespace={} id={}",
        namespace, id
    ));
    let dim = dim.unwrap_or(1536);

    let db = store
        .open_or_get(&namespace, dim)
        .map_err(|e| map_trivium_error("打开数据库失败", e))?;
    let mut db = db
        .lock()
        .map_err(|e| map_trivium_error("获取数据库锁失败", e))?;

    db.index_keyword(id, &keyword)
        .map_err(|e| map_trivium_error("建立关键词索引失败", e))
}

/// 编译文本索引（批量操作后需要调用）
#[tauri::command]
pub async fn trivium_build_text_index(
    namespace: String,
    dim: Option<usize>,
    store: State<'_, Arc<TriviumStoreManager>>,
) -> Result<(), CommandError> {
    log_command(format!(
        "trivium_build_text_index namespace={}",
        namespace
    ));
    let dim = dim.unwrap_or(1536);

    let db = store
        .open_or_get(&namespace, dim)
        .map_err(|e| map_trivium_error("打开数据库失败", e))?;
    let mut db = db
        .lock()
        .map_err(|e| map_trivium_error("获取数据库锁失败", e))?;

    db.build_text_index()
        .map_err(|e| map_trivium_error("编译文本索引失败", e))
}

// ════════════════════════════════════════════════
// 指令：统计与信息
// ════════════════════════════════════════════════

/// 获取数据库统计信息
#[tauri::command]
pub async fn trivium_stats(
    namespace: String,
    dim: Option<usize>,
    store: State<'_, Arc<TriviumStoreManager>>,
) -> Result<Value, CommandError> {
    log_command(format!("trivium_stats namespace={}", namespace));
    let dim = dim.unwrap_or(1536);

    let db = store
        .open_or_get(&namespace, dim)
        .map_err(|e| map_trivium_error("打开数据库失败", e))?;
    let db = db
        .lock()
        .map_err(|e| map_trivium_error("获取数据库锁失败", e))?;

    Ok(serde_json::json!({
        "namespace": namespace,
        "dim": db.dim(),
        "nodeCount": db.node_count(),
        "estimatedMemoryBytes": db.estimated_memory(),
    }))
}

/// 列出所有已打开的命名空间
#[tauri::command]
pub async fn trivium_list_namespaces(
    store: State<'_, Arc<TriviumStoreManager>>,
) -> Result<Vec<String>, CommandError> {
    log_command("trivium_list_namespaces");
    store
        .list_namespaces()
        .map_err(|e| map_trivium_error("列出命名空间失败", e))
}
