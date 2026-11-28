use std::collections::{HashMap, hash_map::Entry};
use std::sync::{Mutex, MutexGuard};

use tokio::sync::watch;

use crate::errors::ApplicationError;

/// A request ID is registered at most once while its request runs. Dropping the
/// `RegisteredRequest` unregisters it; `cancel` unregisters it and sends `true`.
#[derive(Default)]
pub(crate) struct CancellationRegistry {
    active: Mutex<HashMap<String, watch::Sender<bool>>>,
}

impl CancellationRegistry {
    pub(crate) fn register(
        &self,
        request_id: &str,
    ) -> Result<RegisteredRequest<'_>, ApplicationError> {
        let (sender, cancel) = watch::channel(false);
        match self.active().entry(request_id.to_string()) {
            Entry::Occupied(_) => Err(ApplicationError::Conflict(format!(
                "Request already exists: {request_id}"
            ))),
            Entry::Vacant(entry) => {
                entry.insert(sender);
                Ok(RegisteredRequest {
                    registry: self,
                    request_id: request_id.to_string(),
                    cancel,
                })
            }
        }
    }

    pub(crate) fn cancel(&self, request_id: &str) -> bool {
        let Some(sender) = self.active().remove(request_id) else {
            return false;
        };

        let _ = sender.send(true);
        true
    }

    fn active(&self) -> MutexGuard<'_, HashMap<String, watch::Sender<bool>>> {
        self.active
            .lock()
            .expect("request cancellation lock poisoned")
    }
}

/// A request the page can cancel by its ID until this is dropped.
pub struct RegisteredRequest<'a> {
    registry: &'a CancellationRegistry,
    request_id: String,
    cancel: watch::Receiver<bool>,
}

impl RegisteredRequest<'_> {
    pub(crate) fn id(&self) -> &str {
        &self.request_id
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        *self.cancel.borrow()
    }

    pub(crate) fn receiver(&self) -> watch::Receiver<bool> {
        self.cancel.clone()
    }
}

impl Drop for RegisteredRequest<'_> {
    fn drop(&mut self) {
        self.registry.active().remove(&self.request_id);
    }
}
