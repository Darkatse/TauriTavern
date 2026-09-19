use triviumdb::graph::reachability::{ReachabilityConfig, ReachabilityDirection};
use triviumdb::{BatchSearchConfig, Database, TriviumError};
use tt_contracts::database::*;

use crate::{options::search_config, tql};

pub(crate) fn execute(
    db: &mut Database<f32>,
    namespace: &str,
    operation: DatabaseOperation,
) -> Result<DatabaseResponse, TriviumError> {
    use DatabaseOperation as Op;
    use DatabaseResponse as Response;

    match operation {
        Op::Insert { vector, payload } => {
            return Ok(Response::Id(db.insert(&vector, payload)?));
        }
        Op::BatchInsert { vectors, payloads } => {
            if vectors.len() != payloads.len() {
                return Err(TriviumError::InvalidInput(
                    "vectors and payloads must have the same length".into(),
                ));
            }
            let mut tx = db.begin_tx();
            for (vector, payload) in vectors.iter().zip(payloads) {
                tx.insert(vector, payload);
            }
            return Ok(Response::Ids(tx.commit()?));
        }
        Op::Upsert {
            id,
            vector,
            payload,
        } => db.upsert_with_id(id, &vector, payload)?,
        Op::Get { id } => {
            return Ok(Response::Node(db.get(id).map(|value| {
                node(value.id, value.vector, value.payload, value.edges)
            })));
        }
        Op::UpdatePayload { id, payload } => db.update_payload(id, payload)?,
        Op::PatchPayload { id, patch } => db.patch_payload(id, patch)?,
        Op::UpdateVector { id, vector } => db.update_vector(id, &vector)?,
        Op::Delete { id } => db.delete(id)?,
        Op::Link {
            src,
            dst,
            label,
            weight,
        } => db.link(src, dst, &label, weight)?,
        Op::Unlink { src, dst } => db.unlink(src, dst)?,
        Op::ShortestPath {
            source,
            target,
            max_depth,
            label,
        } => {
            return Ok(Response::Path(db.shortest_path(
                source,
                target,
                max_depth.unwrap_or(6),
                label.as_deref(),
            )));
        }
        Op::Subgraph {
            id,
            max_depth,
            labels,
            direction,
        } => {
            let defaults = ReachabilityConfig::default();
            let config = ReachabilityConfig {
                max_depth: max_depth.unwrap_or(defaults.max_depth),
                labels,
                direction: match direction {
                    Some(Direction::Incoming) => ReachabilityDirection::Incoming,
                    Some(Direction::Both) => ReachabilityDirection::Both,
                    Some(Direction::Outgoing) => ReachabilityDirection::Outgoing,
                    None => defaults.direction,
                },
                ..defaults
            };
            let result = db.query_subgraph(id, &config)?;
            return Ok(Response::Subgraph(Subgraph {
                nodes: result
                    .nodes
                    .into_iter()
                    .map(|value| SubgraphNode {
                        id: value.id,
                        payload: value.payload,
                    })
                    .collect(),
                edges: result
                    .edges
                    .into_iter()
                    .map(|value| SubgraphEdge {
                        source_id: value.source_id,
                        target_id: value.target_id,
                        label: value.label,
                        weight: value.weight,
                        metadata: value.metadata,
                    })
                    .collect(),
            }));
        }
        Op::IndexText { id, text } => db.index_text(id, &text)?,
        Op::IndexKeyword { id, keyword } => db.index_keyword(id, &keyword)?,
        Op::BuildTextIndex => {
            db.build_text_index()?;
            db.flush()?;
        }
        Op::Search {
            vector,
            query_text,
            config,
        } => {
            let config = search_config(*config, false, query_text.is_some())?;
            return Ok(Response::Hits(hits(db.search_hybrid(
                query_text.as_deref(),
                vector.as_deref(),
                &config,
            )?)));
        }
        Op::SearchBatch {
            vectors,
            config,
            parallelism,
        } => {
            let config = search_config(*config, false, false)?;
            let batch = BatchSearchConfig {
                parallelism: parallelism.unwrap_or(0),
            };
            return Ok(Response::BatchHits(
                db.search_batch(&vectors, &config, &batch)?
                    .into_iter()
                    .map(hits)
                    .collect(),
            ));
        }
        Op::SearchAdvanced {
            vector,
            query_text,
            config,
        } => {
            let config = search_config(*config, true, query_text.is_some())?;
            let (results, context) =
                db.search_hybrid_with_context(query_text.as_deref(), vector.as_deref(), &config)?;
            return Ok(Response::Advanced(AdvancedSearchResult {
                hits: hits(results),
                context: SearchContext {
                    timings_ms: context
                        .stage_timings
                        .into_iter()
                        .map(|(key, time)| (key, time.as_secs_f64() * 1000.0))
                        .collect(),
                    stage_counts: context.stage_counts.into_iter().collect(),
                    observations: context.observations.into_iter().collect(),
                },
            }));
        }
        Op::Query { query, params } => return tql::query(db, &query, &params).map(Response::Query),
        Op::BuildQuiverIndex => db.build_quiver_index(None)?,
        Op::Compact => db.compact()?,
        Op::Flush => db.flush()?,
        Op::Stats => {
            let graph = db.graph_stats();
            return Ok(Response::Stats(DatabaseStats {
                namespace: namespace.into(),
                dim: db.dim(),
                node_count: db.node_count(),
                estimated_memory_bytes: db.estimated_memory(),
                graph: GraphStats {
                    node_count: graph.node_count,
                    edge_count: graph.edge_count,
                },
            }));
        }
    }
    Ok(Response::Unit)
}

pub(crate) fn node(
    id: u64,
    vector: Vec<f32>,
    payload: serde_json::Value,
    edges: Vec<triviumdb::Edge>,
) -> DatabaseNode {
    DatabaseNode {
        id,
        vector,
        payload,
        edges: edges
            .into_iter()
            .map(|edge| DatabaseEdge {
                target_id: edge.target_id,
                label: edge.label,
                weight: edge.weight,
                metadata: edge.metadata,
            })
            .collect(),
    }
}

fn hits(results: Vec<triviumdb::SearchHit>) -> Vec<SearchHit> {
    results
        .into_iter()
        .map(|hit| SearchHit {
            id: hit.id,
            score: hit.score,
            payload: hit.payload,
        })
        .collect()
}
