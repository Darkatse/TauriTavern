use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};
use sha2::{Digest, Sha256};
use tokio::sync::OnceCell;

use tt_domain::errors::DomainError;
use tt_ports::repositories::vector_repository::{
    VectorMatch, VectorRecord, VectorRepository, VectorScope,
};

const METADATA: TableDefinition<&str, &[u8]> = TableDefinition::new("metadata_v1");
const VECTORS: TableDefinition<&str, &[u8]> = TableDefinition::new("vectors_v1");
const SCOPES: TableDefinition<&str, u32> = TableDefinition::new("scopes_v1");

pub struct RedbVectorRepository {
    database_path: PathBuf,
    database: OnceCell<Arc<Database>>,
}

impl RedbVectorRepository {
    pub fn new(database_path: PathBuf) -> Self {
        Self {
            database_path,
            database: OnceCell::new(),
        }
    }

    async fn database(&self) -> Result<Arc<Database>, DomainError> {
        let database_path = self.database_path.clone();
        self.database
            .get_or_try_init(|| async move {
                tokio::task::spawn_blocking(move || {
                    if let Some(parent) = database_path.parent() {
                        std::fs::create_dir_all(parent).map_err(|error| {
                            storage_error("create vector storage directory", error)
                        })?;
                    }

                    let database = Database::create(&database_path)
                        .map_err(|error| storage_error("open vector database", error))?;
                    let transaction = database
                        .begin_write()
                        .map_err(|error| storage_error("initialize vector database", error))?;
                    {
                        transaction
                            .open_table(METADATA)
                            .map_err(|error| storage_error("initialize metadata table", error))?;
                        transaction
                            .open_table(VECTORS)
                            .map_err(|error| storage_error("initialize vector table", error))?;
                        transaction
                            .open_table(SCOPES)
                            .map_err(|error| storage_error("initialize scope table", error))?;
                    }
                    transaction
                        .commit()
                        .map_err(|error| storage_error("commit vector database schema", error))?;
                    Ok(Arc::new(database))
                })
                .await
                .map_err(|error| {
                    DomainError::InternalError(format!(
                        "Vector database initialization task failed: {error}"
                    ))
                })?
            })
            .await
            .cloned()
    }

    async fn run_blocking<T, F>(&self, operation: F) -> Result<T, DomainError>
    where
        T: Send + 'static,
        F: FnOnce(Arc<Database>) -> Result<T, DomainError> + Send + 'static,
    {
        let database = self.database().await?;
        tokio::task::spawn_blocking(move || operation(database))
            .await
            .map_err(|error| {
                DomainError::InternalError(format!("Vector storage task failed: {error}"))
            })?
    }
}

#[async_trait]
impl VectorRepository for RedbVectorRepository {
    async fn list_hashes(&self, scope: &VectorScope) -> Result<Vec<i64>, DomainError> {
        let prefix = scope_prefix(scope);
        self.run_blocking(move |database| {
            let transaction = database
                .begin_read()
                .map_err(|error| storage_error("read vector hashes", error))?;
            let table = transaction
                .open_table(METADATA)
                .map_err(|error| storage_error("open vector metadata", error))?;

            let mut hashes = Vec::new();
            let end = prefix_end(&prefix);
            for entry in table
                .range(prefix.as_str()..end.as_str())
                .map_err(|error| storage_error("scan vector metadata", error))?
            {
                let (_, value) =
                    entry.map_err(|error| storage_error("read vector metadata", error))?;
                let metadata = serde_json::from_slice::<
                    tt_ports::repositories::vector_repository::VectorMetadata,
                >(value.value())
                .map_err(|error| storage_error("decode vector metadata", error))?;
                hashes.push(metadata.hash);
            }
            Ok(hashes)
        })
        .await
    }

    async fn upsert(
        &self,
        scope: &VectorScope,
        records: Vec<VectorRecord>,
    ) -> Result<(), DomainError> {
        if records.is_empty() {
            return Ok(());
        }

        let dimension = validate_records(&records)?;
        let prefix = scope_prefix(scope);
        self.run_blocking(move |database| {
            let transaction = database
                .begin_write()
                .map_err(|error| storage_error("begin vector upsert", error))?;
            {
                let mut scopes = transaction
                    .open_table(SCOPES)
                    .map_err(|error| storage_error("open vector scopes", error))?;
                if let Some(stored) = scopes
                    .get(prefix.as_str())
                    .map_err(|error| storage_error("read vector dimension", error))?
                {
                    if stored.value() != dimension {
                        return Err(DomainError::Conflict(format!(
                            "Embedding dimension changed for the selected vector source: stored {}, received {dimension}",
                            stored.value()
                        )));
                    }
                } else {
                    scopes
                        .insert(prefix.as_str(), dimension)
                        .map_err(|error| storage_error("store vector dimension", error))?;
                }

                let mut metadata_table = transaction
                    .open_table(METADATA)
                    .map_err(|error| storage_error("open vector metadata", error))?;
                let mut vector_table = transaction
                    .open_table(VECTORS)
                    .map_err(|error| storage_error("open vector values", error))?;

                for record in records {
                    let key = format!("{}{}", prefix, item_id(&record));
                    let metadata = serde_json::to_vec(&record.metadata)
                        .map_err(|error| storage_error("encode vector metadata", error))?;
                    let embedding = encode_embedding(&record.embedding);
                    metadata_table
                        .insert(key.as_str(), metadata.as_slice())
                        .map_err(|error| storage_error("store vector metadata", error))?;
                    vector_table
                        .insert(key.as_str(), embedding.as_slice())
                        .map_err(|error| storage_error("store vector value", error))?;
                }
            }
            transaction
                .commit()
                .map_err(|error| storage_error("commit vector upsert", error))
        })
        .await
    }

    async fn delete_hashes(&self, scope: &VectorScope, hashes: &[i64]) -> Result<(), DomainError> {
        if hashes.is_empty() {
            return Ok(());
        }

        let prefix = scope_prefix(scope);
        let hashes = hashes.iter().copied().collect::<HashSet<_>>();
        self.run_blocking(move |database| {
            let transaction = database
                .begin_write()
                .map_err(|error| storage_error("begin vector delete", error))?;
            {
                let mut metadata_table = transaction
                    .open_table(METADATA)
                    .map_err(|error| storage_error("open vector metadata", error))?;
                let end = prefix_end(&prefix);
                let keys = metadata_table
                    .range(prefix.as_str()..end.as_str())
                    .map_err(|error| storage_error("scan vector metadata", error))?
                    .map(|entry| {
                        let (key, value) =
                            entry.map_err(|error| storage_error("read vector metadata", error))?;
                        let metadata = serde_json::from_slice::<
                            tt_ports::repositories::vector_repository::VectorMetadata,
                        >(value.value())
                        .map_err(|error| storage_error("decode vector metadata", error))?;
                        Ok((key.value().to_string(), metadata.hash))
                    })
                    .collect::<Result<Vec<_>, DomainError>>()?;

                let mut vector_table = transaction
                    .open_table(VECTORS)
                    .map_err(|error| storage_error("open vector values", error))?;
                for (key, hash) in keys {
                    if hashes.contains(&hash) {
                        metadata_table
                            .remove(key.as_str())
                            .map_err(|error| storage_error("delete vector metadata", error))?;
                        vector_table
                            .remove(key.as_str())
                            .map_err(|error| storage_error("delete vector value", error))?;
                    }
                }
            }
            transaction
                .commit()
                .map_err(|error| storage_error("commit vector delete", error))
        })
        .await
    }

    async fn query(
        &self,
        scope: &VectorScope,
        embedding: Vec<f32>,
        limit: usize,
    ) -> Result<Vec<VectorMatch>, DomainError> {
        validate_embedding(&embedding)?;
        let prefix = scope_prefix(scope);
        self.run_blocking(move |database| {
            let transaction = database
                .begin_read()
                .map_err(|error| storage_error("begin vector query", error))?;
            let scopes = transaction
                .open_table(SCOPES)
                .map_err(|error| storage_error("open vector scopes", error))?;
            let Some(stored_dimension) = scopes
                .get(prefix.as_str())
                .map_err(|error| storage_error("read vector dimension", error))?
            else {
                return Ok(Vec::new());
            };
            if stored_dimension.value() as usize != embedding.len() {
                return Err(DomainError::Conflict(format!(
                    "Embedding dimension changed for the selected vector source: stored {}, received {}",
                    stored_dimension.value(),
                    embedding.len()
                )));
            }

            let vectors = transaction
                .open_table(VECTORS)
                .map_err(|error| storage_error("open vector values", error))?;
            let mut scored = Vec::new();
            let end = prefix_end(&prefix);
            for entry in vectors
                .range(prefix.as_str()..end.as_str())
                .map_err(|error| storage_error("scan vector values", error))?
            {
                let (key, value) =
                    entry.map_err(|error| storage_error("read vector value", error))?;
                let stored = decode_embedding(value.value())?;
                if stored.len() != embedding.len() {
                    return Err(DomainError::InternalError(format!(
                        "Vector index contains an invalid dimension for key {}",
                        key.value()
                    )));
                }
                let score = stored
                    .iter()
                    .zip(&embedding)
                    .map(|(left, right)| left * right)
                    .sum::<f32>();
                scored.push((key.value().to_string(), score));
            }

            // ponytail: exact scan/sort is the deliberate baseline; add ANN only after
            // measured collection-scale latency warrants its index lifecycle complexity.
            scored.sort_by(|left, right| right.1.total_cmp(&left.1));
            scored.truncate(limit);

            let metadata = transaction
                .open_table(METADATA)
                .map_err(|error| storage_error("open vector metadata", error))?;
            scored
                .into_iter()
                .map(|(key, score)| {
                    let value = metadata
                        .get(key.as_str())
                        .map_err(|error| storage_error("read vector metadata", error))?
                        .ok_or_else(|| {
                            DomainError::InternalError(format!(
                                "Vector index is missing metadata for key {key}"
                            ))
                        })?;
                    let metadata = serde_json::from_slice(value.value())
                        .map_err(|error| storage_error("decode vector metadata", error))?;
                    Ok(VectorMatch { metadata, score })
                })
                .collect()
        })
        .await
    }

    async fn purge_collection(&self, collection_id: &str) -> Result<(), DomainError> {
        let prefix = collection_prefix(collection_id);
        self.run_blocking(move |database| {
            let transaction = database
                .begin_write()
                .map_err(|error| storage_error("begin vector collection purge", error))?;
            {
                let end = prefix_end(&prefix);
                let mut metadata = transaction
                    .open_table(METADATA)
                    .map_err(|error| storage_error("open vector metadata", error))?;
                metadata
                    .retain_in(prefix.as_str()..end.as_str(), |_, _| false)
                    .map_err(|error| storage_error("purge vector metadata", error))?;

                let mut vectors = transaction
                    .open_table(VECTORS)
                    .map_err(|error| storage_error("open vector values", error))?;
                vectors
                    .retain_in(prefix.as_str()..end.as_str(), |_, _| false)
                    .map_err(|error| storage_error("purge vector values", error))?;

                let mut scopes = transaction
                    .open_table(SCOPES)
                    .map_err(|error| storage_error("open vector scopes", error))?;
                scopes
                    .retain_in(prefix.as_str()..end.as_str(), |_, _| false)
                    .map_err(|error| storage_error("purge vector scopes", error))?;
            }
            transaction
                .commit()
                .map_err(|error| storage_error("commit vector collection purge", error))
        })
        .await
    }

    async fn purge_all(&self) -> Result<(), DomainError> {
        self.run_blocking(move |database| {
            let transaction = database
                .begin_write()
                .map_err(|error| storage_error("begin vector purge", error))?;
            {
                transaction
                    .open_table(METADATA)
                    .map_err(|error| storage_error("open vector metadata", error))?
                    .retain(|_, _| false)
                    .map_err(|error| storage_error("purge vector metadata", error))?;
                transaction
                    .open_table(VECTORS)
                    .map_err(|error| storage_error("open vector values", error))?
                    .retain(|_, _| false)
                    .map_err(|error| storage_error("purge vector values", error))?;
                transaction
                    .open_table(SCOPES)
                    .map_err(|error| storage_error("open vector scopes", error))?
                    .retain(|_, _| false)
                    .map_err(|error| storage_error("purge vector scopes", error))?;
            }
            transaction
                .commit()
                .map_err(|error| storage_error("commit vector purge", error))
        })
        .await
    }
}

fn validate_records(records: &[VectorRecord]) -> Result<u32, DomainError> {
    let dimension = records[0].embedding.len();
    if dimension > u32::MAX as usize {
        return Err(DomainError::InvalidData(
            "Embedding dimension is too large".to_string(),
        ));
    }
    for record in records {
        validate_embedding(&record.embedding)?;
        if record.embedding.len() != dimension {
            return Err(DomainError::InvalidData(
                "Embedding batch contains mixed dimensions".to_string(),
            ));
        }
    }
    Ok(dimension as u32)
}

fn validate_embedding(embedding: &[f32]) -> Result<(), DomainError> {
    if embedding.is_empty() {
        return Err(DomainError::InvalidData(
            "Embedding must not be empty".to_string(),
        ));
    }
    if embedding.iter().any(|value| !value.is_finite()) {
        return Err(DomainError::InvalidData(
            "Embedding contains a non-finite value".to_string(),
        ));
    }
    if embedding
        .iter()
        .map(|value| f64::from(*value).powi(2))
        .sum::<f64>()
        <= f64::EPSILON
    {
        return Err(DomainError::InvalidData(
            "Embedding must not be a zero vector".to_string(),
        ));
    }
    Ok(())
}

fn collection_prefix(collection_id: &str) -> String {
    format!("c/{}/", sha256_hex(collection_id.as_bytes()))
}

fn scope_prefix(scope: &VectorScope) -> String {
    let mut identity = Sha256::new();
    identity.update((scope.source.len() as u64).to_le_bytes());
    identity.update(scope.source.as_bytes());
    identity.update((scope.profile.len() as u64).to_le_bytes());
    identity.update(scope.profile.as_bytes());
    format!(
        "{}s/{}/",
        collection_prefix(&scope.collection_id),
        hex_digest(identity.finalize())
    )
}

fn item_id(record: &VectorRecord) -> String {
    let mut identity = Sha256::new();
    identity.update(record.metadata.hash.to_le_bytes());
    identity.update(record.metadata.index.to_le_bytes());
    identity.update(record.metadata.text.as_bytes());
    hex_digest(identity.finalize())
}

fn prefix_end(prefix: &str) -> String {
    format!("{prefix}~")
}

fn sha256_hex(value: &[u8]) -> String {
    hex_digest(Sha256::digest(value))
}

fn hex_digest(digest: impl AsRef<[u8]>) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = digest.as_ref();
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn encode_embedding(embedding: &[f32]) -> Vec<u8> {
    embedding
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

fn decode_embedding(bytes: &[u8]) -> Result<Vec<f32>, DomainError> {
    let chunks = bytes.chunks_exact(4);
    if !chunks.remainder().is_empty() {
        return Err(DomainError::InternalError(
            "Vector index contains malformed embedding bytes".to_string(),
        ));
    }
    let embedding = chunks
        .map(|chunk| f32::from_le_bytes(chunk.try_into().expect("four-byte chunk")))
        .collect::<Vec<_>>();
    validate_embedding(&embedding)?;
    Ok(embedding)
}

fn storage_error(action: &str, error: impl std::fmt::Display) -> DomainError {
    DomainError::InternalError(format!("Failed to {action}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tt_ports::repositories::vector_repository::VectorMetadata;
    use uuid::Uuid;

    struct TempDatabase(PathBuf);

    impl TempDatabase {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("tauritavern-vector-{}", Uuid::new_v4()));
            Self(root.join("index.redb"))
        }
    }

    impl Drop for TempDatabase {
        fn drop(&mut self) {
            if let Some(root) = self.0.parent() {
                let _ = std::fs::remove_dir_all(root);
            }
        }
    }

    fn record(hash: i64, text: &str, embedding: Vec<f32>) -> VectorRecord {
        VectorRecord {
            metadata: VectorMetadata {
                hash,
                text: text.to_string(),
                index: 0,
            },
            embedding,
        }
    }

    #[tokio::test]
    async fn exact_query_is_idempotent_and_purge_is_collection_scoped() {
        let temp = TempDatabase::new();
        let repository = RedbVectorRepository::new(temp.0.clone());
        let scope = VectorScope {
            collection_id: "chat-a".to_string(),
            source: "test".to_string(),
            profile: "model-a".to_string(),
        };
        let other_scope = VectorScope {
            collection_id: "chat-b".to_string(),
            ..scope.clone()
        };

        repository
            .upsert(
                &scope,
                vec![
                    record(1, "east", vec![1.0, 0.0]),
                    record(2, "north", vec![0.0, 1.0]),
                ],
            )
            .await
            .unwrap();
        repository
            .upsert(&scope, vec![record(1, "east", vec![1.0, 0.0])])
            .await
            .unwrap();
        repository
            .upsert(&other_scope, vec![record(3, "west", vec![-1.0, 0.0])])
            .await
            .unwrap();

        assert_eq!(repository.list_hashes(&scope).await.unwrap().len(), 2);
        let matches = repository.query(&scope, vec![1.0, 0.0], 2).await.unwrap();
        assert_eq!(matches[0].metadata.hash, 1);
        assert_eq!(matches[0].score, 1.0);

        assert!(matches!(
            repository
                .upsert(&scope, vec![record(4, "wrong", vec![1.0, 0.0, 0.0])])
                .await,
            Err(DomainError::Conflict(_))
        ));
        assert_eq!(repository.list_hashes(&scope).await.unwrap().len(), 2);

        repository.delete_hashes(&scope, &[1]).await.unwrap();
        assert_eq!(repository.list_hashes(&scope).await.unwrap(), vec![2]);

        repository.purge_collection("chat-a").await.unwrap();
        assert!(repository.list_hashes(&scope).await.unwrap().is_empty());
        assert_eq!(repository.list_hashes(&other_scope).await.unwrap(), vec![3]);

        drop(repository);
        let reopened = RedbVectorRepository::new(temp.0.clone());
        assert_eq!(reopened.list_hashes(&other_scope).await.unwrap(), vec![3]);
    }

    #[tokio::test]
    async fn concurrent_writers_commit_complete_transactions() {
        let temp = TempDatabase::new();
        let repository = Arc::new(RedbVectorRepository::new(temp.0.clone()));
        let first = VectorScope {
            collection_id: "first".to_string(),
            source: "test".to_string(),
            profile: "model".to_string(),
        };
        let second = VectorScope {
            collection_id: "second".to_string(),
            ..first.clone()
        };

        tokio::try_join!(
            repository.upsert(&first, vec![record(1, "one", vec![1.0, 0.0])]),
            repository.upsert(&second, vec![record(2, "two", vec![0.0, 1.0])]),
        )
        .unwrap();

        assert_eq!(repository.list_hashes(&first).await.unwrap(), vec![1]);
        assert_eq!(repository.list_hashes(&second).await.unwrap(), vec![2]);
    }
}
