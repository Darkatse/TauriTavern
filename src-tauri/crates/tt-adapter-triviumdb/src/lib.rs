//! Native database ownership and blocking execution, independent of the Tauri host.

mod error;
mod operations;
mod options;
mod tql;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tokio::sync::Mutex as AsyncMutex;
use triviumdb::Database;
use tt_contracts::database::{
    DatabaseConfig, DatabaseOpenOptions, DatabaseRequest, DatabaseResponse, OpenDatabase,
};
use tt_domain::errors::DomainError;
use tt_ports::database::DatabaseBackend;

use error::engine_error;

struct OpenStore {
    database: Database<f32>,
    options: DatabaseConfig,
}

type Slot = Arc<AsyncMutex<Option<OpenStore>>>;

pub struct TriviumDatabaseBackend {
    root: PathBuf,
    // Slots survive close so a concurrent reopen cannot acquire a second owner.
    instances: Mutex<BTreeMap<String, Slot>>,
}

impl TriviumDatabaseBackend {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            instances: Mutex::new(BTreeMap::new()),
        }
    }

    fn slot(&self, namespace: &str) -> Result<Slot, DomainError> {
        if namespace.is_empty()
            || namespace.len() > 128
            || !namespace.bytes().all(|ch| {
                ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == b'-' || ch == b'_'
            })
        {
            return Err(DomainError::InvalidData(
                "Database namespace must contain 1–128 lowercase ASCII letters, digits, '-' or '_'"
                    .into(),
            ));
        }
        let mut instances = self
            .instances
            .lock()
            .map_err(|_| DomainError::InternalError("Database registry lock poisoned".into()))?;
        Ok(instances.entry(namespace.into()).or_default().clone())
    }

    fn slots(&self) -> Result<Vec<(String, Slot)>, DomainError> {
        let instances = self
            .instances
            .lock()
            .map_err(|_| DomainError::InternalError("Database registry lock poisoned".into()))?;
        Ok(instances
            .iter()
            .map(|(name, slot)| (name.clone(), slot.clone()))
            .collect())
    }

    async fn run(
        slot: Slot,
        operation: impl FnOnce(&mut Option<OpenStore>) -> Result<DatabaseResponse, DomainError>
        + Send
        + 'static,
    ) -> Result<DatabaseResponse, DomainError> {
        // ponytail: serialize each namespace; use native concurrent reads if measured
        // contention warrants it. Wait asynchronously, before occupying a blocking thread.
        let mut store = slot.lock_owned().await;
        tokio::task::spawn_blocking(move || operation(&mut store))
            .await
            .map_err(|error| DomainError::InternalError(format!("Database task failed: {error}")))?
    }

    async fn open(
        &self,
        namespace: String,
        requested: DatabaseOpenOptions,
    ) -> Result<DatabaseResponse, DomainError> {
        let slot = self.slot(&namespace)?;
        // Prefix avoids Windows device names such as CON; lowercase avoids case aliases.
        let path = self
            .root
            .join(format!("db-{namespace}"))
            .join("database.tdb");
        Self::run(slot, move |store| {
            let requested_dim = requested.dim;
            let mut resolved = options::resolve_open_options(
                requested,
                store.as_ref().map(|store| &store.options),
            );
            if let Some(current) = store {
                // Preloading only applies when a native instance is first opened.
                resolved.load_text_index = current.options.load_text_index;
                if current.options != resolved {
                    return Err(DomainError::Conflict(format!(
                        "Database {namespace} is already open with different options; \
                         close it before changing configuration"
                    )));
                }
            } else {
                // 0.8.8 flush writes even an unloaded text index. Preserve existing
                // sidecars regardless of the caller's preload preference.
                if path.with_extension("tdb.text").exists() {
                    resolved.load_text_index = true;
                }
                let config = options::open_config(&resolved)
                    .map_err(|error| engine_error(&namespace, error))?;
                let path = path.to_str().ok_or_else(|| {
                    DomainError::InvalidData("Database path must be UTF-8".into())
                })?;
                let database = Database::open_with_config(path, config)
                    .map_err(|error| engine_error(&namespace, error))?;
                if requested_dim.is_some_and(|dim| dim != database.dim()) {
                    return Err(DomainError::Conflict(format!(
                        "Database {namespace} has dimension {}, requested {}",
                        database.dim(),
                        config.dim
                    )));
                }
                resolved.dim = database.dim();
                *store = Some(OpenStore {
                    database,
                    options: resolved.clone(),
                });
            }
            Ok(DatabaseResponse::Open(OpenDatabase {
                namespace,
                dim: resolved.dim,
                options: resolved,
            }))
        })
        .await
    }

    async fn maintain_all(&self, close: bool) -> Result<DatabaseResponse, DomainError> {
        let mut first_error = None;
        for (namespace, slot) in self.slots()? {
            let result = Self::run(slot, move |store| maintain(store, &namespace, close)).await;
            if let Err(error) = result {
                first_error.get_or_insert(error);
            }
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(DatabaseResponse::Unit),
        }
    }
}

fn maintain(
    store: &mut Option<OpenStore>,
    namespace: &str,
    close: bool,
) -> Result<DatabaseResponse, DomainError> {
    if let Some(current) = store {
        if close {
            current
                .database
                .close()
                .map_err(|error| engine_error(namespace, error))?;
            *store = None;
        } else {
            current
                .database
                .flush()
                .map_err(|error| engine_error(namespace, error))?;
        }
    }
    Ok(DatabaseResponse::Unit)
}

#[async_trait]
impl DatabaseBackend for TriviumDatabaseBackend {
    async fn execute(&self, request: DatabaseRequest) -> Result<DatabaseResponse, DomainError> {
        match request {
            DatabaseRequest::Open { namespace, options } => self.open(namespace, options).await,
            DatabaseRequest::Close { namespace } => {
                let slot = self.slot(&namespace)?;
                Self::run(slot, move |store| maintain(store, &namespace, true)).await
            }
            DatabaseRequest::ListNamespaces => {
                let mut names = Vec::new();
                for (name, slot) in self.slots()? {
                    if slot.lock().await.is_some() {
                        names.push(name);
                    }
                }
                Ok(DatabaseResponse::Namespaces(names))
            }
            DatabaseRequest::Execute {
                namespace,
                operation,
            } => {
                let slot = self.slot(&namespace)?;
                Self::run(slot, move |store| {
                    let store = store.as_mut().ok_or_else(|| {
                        DomainError::NotFound(format!("Database {namespace} is not open"))
                    })?;
                    operations::execute(&mut store.database, &namespace, *operation)
                        .map_err(|error| engine_error(&namespace, error))
                })
                .await
            }
            DatabaseRequest::FlushAll => self.maintain_all(false).await,
            DatabaseRequest::CloseAll => self.maintain_all(true).await,
        }
    }
}

#[cfg(test)]
mod tests;
