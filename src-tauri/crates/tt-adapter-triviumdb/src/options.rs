use triviumdb::database::Config;
use triviumdb::{EdgeDirection, Filter, SearchConfig, TriviumError};
use tt_contracts::database::{
    DatabaseConfig, DatabaseOpenOptions, Direction, SearchOptions, StorageMode, SyncMode,
};

pub(crate) fn resolve_open_options(
    requested: DatabaseOpenOptions,
    current: Option<&DatabaseConfig>,
) -> DatabaseConfig {
    let defaults = DatabaseConfig {
        dim: 1536,
        storage_mode: StorageMode::Mmap,
        sync_mode: SyncMode::Normal,
        load_text_index: true,
        auto_build_quiver: true,
        memory_limit_mb: 0,
    };
    let base = current.unwrap_or(&defaults);
    DatabaseConfig {
        dim: requested.dim.unwrap_or(base.dim),
        storage_mode: requested.storage_mode.unwrap_or(base.storage_mode),
        sync_mode: requested.sync_mode.unwrap_or(base.sync_mode),
        load_text_index: requested.load_text_index.unwrap_or(base.load_text_index),
        auto_build_quiver: requested
            .auto_build_quiver
            .unwrap_or(base.auto_build_quiver),
        memory_limit_mb: requested.memory_limit_mb.unwrap_or(base.memory_limit_mb),
    }
}

pub(crate) fn open_config(options: &DatabaseConfig) -> Result<Config, TriviumError> {
    Ok(Config {
        dim: options.dim,
        storage_mode: match options.storage_mode {
            StorageMode::Mmap => triviumdb::database::StorageMode::Mmap,
            StorageMode::Rom => triviumdb::database::StorageMode::Rom,
        },
        sync_mode: match options.sync_mode {
            SyncMode::Normal => triviumdb::storage::wal::SyncMode::Normal,
            SyncMode::Full => triviumdb::storage::wal::SyncMode::Full,
            SyncMode::Off => triviumdb::storage::wal::SyncMode::Off,
        },
        load_text_index: options.load_text_index,
        auto_build_quiver: options.auto_build_quiver,
        memory_limit: options
            .memory_limit_mb
            .checked_mul(1024 * 1024)
            .ok_or_else(|| TriviumError::InvalidInput("memoryLimitMb is too large".into()))?,
        ..Config::default()
    })
}

pub(crate) fn search_config(
    options: SearchOptions,
    advanced: bool,
    has_text: bool,
) -> Result<SearchConfig, TriviumError> {
    let defaults = SearchConfig::default();
    Ok(SearchConfig {
        top_k: options.top_k.unwrap_or(5),
        recall_k: options.recall_k.unwrap_or(defaults.recall_k),
        rerank_k: options.rerank_k.unwrap_or(defaults.rerank_k),
        expand_depth: options.expand_depth.unwrap_or(defaults.expand_depth),
        expand_labels: options.expand_labels,
        max_edges_per_node: options
            .max_edges_per_node
            .unwrap_or(defaults.max_edges_per_node),
        min_edge_weight: options.min_edge_weight.unwrap_or(defaults.min_edge_weight),
        edge_direction: match options.edge_direction {
            Some(Direction::Incoming) => EdgeDirection::Incoming,
            Some(Direction::Both) => EdgeDirection::Both,
            Some(Direction::Outgoing) => EdgeDirection::Outgoing,
            None => defaults.edge_direction,
        },
        min_score: options.min_score.unwrap_or(defaults.min_score),
        teleport_alpha: options.teleport_alpha.unwrap_or(defaults.teleport_alpha),
        enable_advanced_pipeline: options.enable_advanced_pipeline.unwrap_or(advanced),
        enable_sparse_residual: options
            .enable_sparse_residual
            .unwrap_or(defaults.enable_sparse_residual),
        fista_lambda: options.fista_lambda.unwrap_or(defaults.fista_lambda),
        fista_threshold: options.fista_threshold.unwrap_or(defaults.fista_threshold),
        enable_dpp: options.enable_dpp.unwrap_or(defaults.enable_dpp),
        dpp_quality_weight: options
            .dpp_quality_weight
            .unwrap_or(defaults.dpp_quality_weight),
        enable_refractory_fatigue: options
            .enable_refractory_fatigue
            .unwrap_or(defaults.enable_refractory_fatigue),
        enable_inverse_inhibition: options
            .enable_inverse_inhibition
            .unwrap_or(defaults.enable_inverse_inhibition),
        lateral_inhibition_threshold: options
            .lateral_inhibition_threshold
            .unwrap_or(defaults.lateral_inhibition_threshold),
        force_brute_force: options
            .force_brute_force
            .unwrap_or(defaults.force_brute_force),
        enable_text_hybrid_search: options.enable_text_hybrid_search.unwrap_or(has_text),
        text_boost: options.text_boost.unwrap_or(defaults.text_boost),
        bm25_k1: options.bm25_k1.unwrap_or(defaults.bm25_k1),
        bm25_b: options.bm25_b.unwrap_or(defaults.bm25_b),
        payload_filter: options
            .payload_filter
            .as_ref()
            .map(Filter::from_json)
            .transpose()
            .map_err(TriviumError::InvalidInput)?,
        diffusion_bias: options.diffusion_bias,
    })
}
