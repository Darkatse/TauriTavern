use std::fmt;

use tokio::sync::watch;

#[derive(Clone, Debug, PartialEq, Eq)]
enum BackendStatus {
    Initializing,
    Ready,
    Failed(String),
}

pub(crate) struct BackendReadiness {
    status: watch::Sender<BackendStatus>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum BackendReadinessError {
    Failed(String),
    Closed,
}

impl BackendReadiness {
    pub(crate) fn new() -> Self {
        let (status, _receiver) = watch::channel(BackendStatus::Initializing);
        Self { status }
    }

    pub(crate) fn mark_ready(&self) {
        self.status.send_replace(BackendStatus::Ready);
    }

    pub(crate) fn mark_failed(&self, message: impl Into<String>) {
        self.status
            .send_replace(BackendStatus::Failed(message.into()));
    }

    pub(crate) async fn wait_ready(&self) -> Result<(), BackendReadinessError> {
        let mut status = self.status.subscribe();

        loop {
            match status.borrow_and_update().clone() {
                BackendStatus::Initializing => {}
                BackendStatus::Ready => return Ok(()),
                BackendStatus::Failed(message) => {
                    return Err(BackendReadinessError::Failed(message));
                }
            }

            if status.changed().await.is_err() {
                return Err(BackendReadinessError::Closed);
            }
        }
    }
}

impl fmt::Display for BackendReadinessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Failed(message) => write!(formatter, "{message}"),
            Self::Closed => write!(
                formatter,
                "Backend readiness channel closed before initialization completed"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[tokio::test]
    async fn late_waiter_returns_after_ready() {
        let readiness = BackendReadiness::new();

        readiness.mark_ready();

        assert_eq!(readiness.wait_ready().await, Ok(()));
    }

    #[tokio::test]
    async fn waiter_resolves_when_marked_ready() {
        let readiness = Arc::new(BackendReadiness::new());
        let wait = {
            let readiness = readiness.clone();
            tokio::spawn(async move { readiness.wait_ready().await })
        };

        tokio::task::yield_now().await;
        readiness.mark_ready();

        assert_eq!(wait.await.unwrap(), Ok(()));
    }

    #[tokio::test]
    async fn failed_status_returns_message() {
        let readiness = BackendReadiness::new();

        readiness.mark_failed("startup failed");

        assert_eq!(
            readiness.wait_ready().await,
            Err(BackendReadinessError::Failed("startup failed".to_string()))
        );
    }
}
