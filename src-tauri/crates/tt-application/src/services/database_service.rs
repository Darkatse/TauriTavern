use std::sync::Arc;

use tokio::sync::{OwnedRwLockWriteGuard, RwLock};
use tt_contracts::database::{DatabaseRequest, DatabaseResponse};
use tt_domain::errors::DomainError;
use tt_ports::database::DatabaseBackend;

pub struct DatabaseService {
    backend: Arc<dyn DatabaseBackend>,
    maintenance: Arc<RwLock<()>>,
}

impl DatabaseService {
    pub fn new(backend: Arc<dyn DatabaseBackend>) -> Self {
        Self {
            backend,
            maintenance: Arc::new(RwLock::new(())),
        }
    }

    pub async fn execute(&self, request: DatabaseRequest) -> Result<DatabaseResponse, DomainError> {
        if matches!(
            request,
            DatabaseRequest::FlushAll | DatabaseRequest::CloseAll
        ) {
            let _guard = self.maintenance.write().await;
            self.backend.execute(request).await
        } else {
            let _guard = self.maintenance.read().await;
            self.backend.execute(request).await
        }
    }

    /// Keep the guard through archive IO: flushing alone does not freeze live files.
    pub async fn prepare_archive(
        &self,
        importing: bool,
    ) -> Result<OwnedRwLockWriteGuard<()>, DomainError> {
        let guard = self.maintenance.clone().write_owned().await;
        let request = if importing {
            DatabaseRequest::CloseAll
        } else {
            DatabaseRequest::FlushAll
        };
        self.backend.execute(request).await?;
        Ok(guard)
    }
}

#[async_trait::async_trait]
impl tt_ports::database::DatabaseFileAccess for DatabaseService {
    async fn prepare_sync(
        &self,
        receiving: bool,
    ) -> Result<OwnedRwLockWriteGuard<()>, DomainError> {
        // Concurrent transfers in opposite directions must not wait on each other's lock.
        let guard = self.maintenance.clone().try_write_owned().map_err(|_| {
            DomainError::InvalidData(
                "Database is busy; retry synchronization when the current operation finishes"
                    .into(),
            )
        })?;
        self.backend
            .execute(if receiving {
                DatabaseRequest::CloseAll
            } else {
                DatabaseRequest::FlushAll
            })
            .await?;
        Ok(guard)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::{Future, poll_fn};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::task::Poll;
    use tokio::sync::Semaphore;
    use tt_ports::database::DatabaseFileAccess;

    struct BlockingBackend {
        entered: AtomicUsize,
        finish: Semaphore,
    }

    #[async_trait::async_trait]
    impl DatabaseBackend for BlockingBackend {
        async fn execute(&self, request: DatabaseRequest) -> Result<DatabaseResponse, DomainError> {
            if matches!(request, DatabaseRequest::ListNamespaces) {
                self.entered.fetch_add(1, Ordering::SeqCst);
                self.finish.acquire().await.unwrap().forget();
                return Ok(DatabaseResponse::Namespaces(vec!["memory".into()]));
            }
            Ok(DatabaseResponse::Unit)
        }
    }

    #[tokio::test]
    async fn file_maintenance_excludes_operations_and_conflicting_sync_fails_without_waiting() {
        let backend = Arc::new(BlockingBackend {
            entered: AtomicUsize::new(0),
            finish: Semaphore::new(0),
        });
        let service = DatabaseService::new(backend.clone());
        let mut first = Box::pin(service.execute(DatabaseRequest::ListNamespaces));
        assert!(
            poll_fn(|cx| Poll::Ready(first.as_mut().poll(cx)))
                .await
                .is_pending()
        );
        let mut archive = Box::pin(service.prepare_archive(false));
        assert!(
            poll_fn(|cx| Poll::Ready(archive.as_mut().poll(cx)))
                .await
                .is_pending()
        );
        backend.finish.add_permits(1);
        first.await.unwrap();
        let guard = archive.await.unwrap();
        let mut conflicting_sync = Box::pin(service.prepare_sync(true));
        assert!(matches!(
            poll_fn(|cx| Poll::Ready(conflicting_sync.as_mut().poll(cx))).await,
            Poll::Ready(Err(_))
        ));
        let mut second = Box::pin(service.execute(DatabaseRequest::ListNamespaces));
        assert!(
            poll_fn(|cx| Poll::Ready(second.as_mut().poll(cx)))
                .await
                .is_pending()
        );
        assert_eq!(backend.entered.load(Ordering::SeqCst), 1);
        drop(guard);
        backend.finish.add_permits(1);
        second.await.unwrap();
        assert_eq!(backend.entered.load(Ordering::SeqCst), 2);
        let guard = service.prepare_sync(false).await.unwrap();
        let mut during_sync = Box::pin(service.execute(DatabaseRequest::ListNamespaces));
        assert!(
            poll_fn(|cx| Poll::Ready(during_sync.as_mut().poll(cx)))
                .await
                .is_pending()
        );
        drop(guard);
        backend.finish.add_permits(1);
        during_sync.await.unwrap();
    }
}
